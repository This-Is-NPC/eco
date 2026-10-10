//! Any audio or video file ffmpeg decodes, as 16 kHz mono frames through a pipe:
//! the decoded audio never touches the disk.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use futures::stream::BoxStream;
use serde_json::Value;
use tokio::process::Command;

use crate::adapters::pipe;
use crate::ports::{AudioError, AudioSource, Frame, SAMPLE_RATE};

/// A file's audio track, decoded as fast as ffmpeg goes.
pub struct FfmpegSource {
    path: PathBuf,
}

impl FfmpegSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// The input options that read `path` only as a local file: behind `file:`, a
/// name that begins with `-` is not an option and one with `:` is not a URL, and
/// no other protocol may open, not even from inside the file.
fn local_input(path: &Path) -> [OsString; 4] {
    let mut url = OsString::from("file:");
    url.push(path);
    [
        "-protocol_whitelist".into(),
        "file".into(),
        "-i".into(),
        url,
    ]
}

impl AudioSource for FfmpegSource {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        let mut command = Command::new("ffmpeg");
        command
            .args(["-nostdin", "-v", "error"])
            .args(local_input(&self.path))
            .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string()])
            .args(["-f", "s16le", "-"]);
        pipe::frames(command)
    }
}

/// What ffprobe reads of a media file.
pub struct Probe {
    /// How long it plays, in seconds.
    pub duration: f64,
    /// When it was recorded, in seconds since the epoch, from its `creation_time`
    /// tag (the container's, else a stream's); none when it has none.
    pub recorded: Option<f64>,
}

/// Why ffprobe read no media in a file.
#[derive(Debug, PartialEq)]
pub enum Unprobed {
    /// ffprobe is not installed, or does not run (a shim with no version
    /// behind it), so neither does ffmpeg.
    Missing,
    /// It is not audio or video.
    NotMedia,
}

/// What ffprobe reads of `path`.
pub async fn probe(path: &Path) -> Result<Probe, Unprobed> {
    probe_with(Path::new("ffprobe"), path).await
}

/// What the program `ffprobe` reads of `path`.
async fn probe_with(ffprobe: &Path, path: &Path) -> Result<Probe, Unprobed> {
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries"])
        .arg("format=duration:format_tags=creation_time:stream_tags=creation_time")
        .args(["-of", "json"])
        .args(local_input(path))
        .output()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Unprobed::Missing,
            _ => Unprobed::NotMedia,
        })?;
    if let Some(probe) = read_probe(&output.stdout) {
        return Ok(probe);
    }
    // A file it cannot read and a program that cannot run both fail: only a
    // working ffprobe answers -version.
    let runs = Command::new(ffprobe)
        .arg("-version")
        .output()
        .await
        .is_ok_and(|version| version.status.success());
    Err(if runs {
        Unprobed::NotMedia
    } else {
        Unprobed::Missing
    })
}

/// The probe in ffprobe's JSON; none without a duration.
fn read_probe(json: &[u8]) -> Option<Probe> {
    let read: Value = serde_json::from_slice(json).ok()?;
    let duration = read["format"]["duration"].as_str()?.parse().ok()?;
    let streams = read["streams"].as_array().into_iter().flatten();
    let recorded = std::iter::once(&read["format"])
        .chain(streams)
        .filter_map(|part| part["tags"]["creation_time"].as_str())
        .find_map(|tag| DateTime::parse_from_rfc3339(tag).ok())
        .map(|at| at.timestamp_micros() as f64 / 1e6);
    Some(Probe { duration, recorded })
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;
    #[cfg(unix)]
    use crate::adapters::audio_file::tests::write_wav;
    #[cfg(unix)]
    use crate::ports::FRAME_SAMPLES;

    // ffmpeg is a requirement of the Linux package only so far: the tests
    // that run it to make or read a file run on Linux.

    /// A WAV at another rate and in stereo comes out as 16 kHz mono frames.
    #[cfg(unix)]
    #[tokio::test]
    async fn decodes_any_file_to_frames() {
        let directory = tempfile::tempdir().unwrap();
        let wav = directory.path().join("one-second.wav");
        write_wav(&wav, &vec![1000; SAMPLE_RATE as usize], SAMPLE_RATE);
        let converted = directory.path().join("stereo-44k.ogg");
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&wav)
            .args(["-ac", "2", "-ar", "44100"])
            .arg(&converted)
            .status()
            .unwrap();
        assert!(status.success());

        let frames: Vec<Frame> = FfmpegSource::new(converted.clone())
            .frames()
            .map(|frame| frame.unwrap())
            .collect()
            .await;
        let samples: usize = frames.iter().map(Vec::len).sum();
        assert!(frames.iter().all(|f| f.len() == FRAME_SAMPLES));
        // One second, give or take the codec's padding, in whole frames.
        assert!(
            (31..=34).contains(&frames.len()),
            "{} frames, {samples} samples",
            frames.len()
        );
        let Ok(probe) = probe(&converted).await else {
            panic!("ffprobe read no media");
        };
        assert!((0.95..1.1).contains(&probe.duration), "{}", probe.duration);
        assert_eq!(probe.recorded, None);
    }

    /// A file's `creation_time` tag is when it was recorded.
    #[cfg(unix)]
    #[tokio::test]
    async fn reads_when_a_file_was_recorded() {
        let directory = tempfile::tempdir().unwrap();
        let wav = directory.path().join("one-second.wav");
        write_wav(&wav, &vec![1000; SAMPLE_RATE as usize], SAMPLE_RATE);
        let tagged = directory.path().join("tagged.m4a");
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&wav)
            .args(["-metadata", "creation_time=2026-09-30T09:15:00Z"])
            .arg(&tagged)
            .status()
            .unwrap();
        assert!(status.success());
        let recorded = probe(&tagged).await.map(|probe| probe.recorded);
        assert_eq!(recorded, Ok(Some(1_790_759_700.0)));
    }

    /// A relative name that begins with `-` reaches ffmpeg and ffprobe as a file.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_name_like_an_option_is_a_file() {
        let directory = tempfile::tempdir().unwrap();
        write_wav(
            &directory.path().join("-y.wav"),
            &vec![1000; SAMPLE_RATE as usize],
            SAMPLE_RATE,
        );
        let relative = Path::new("-y.wav");
        let probed = Command::new("ffprobe")
            .current_dir(directory.path())
            .args(["-v", "error", "-show_entries", "format=duration"])
            .args(local_input(relative))
            .output()
            .await
            .unwrap();
        assert!(probed.status.success(), "{probed:?}");
        assert!(String::from_utf8_lossy(&probed.stdout).contains("duration=1.0"));
        let decoded = Command::new("ffmpeg")
            .current_dir(directory.path())
            .args(["-nostdin", "-v", "error"])
            .args(local_input(relative))
            .args(["-f", "s16le", "-"])
            .output()
            .await
            .unwrap();
        assert!(decoded.status.success(), "{decoded:?}");
        assert_eq!(decoded.stdout.len(), SAMPLE_RATE as usize * 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_broken_file_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let broken = directory.path().join("broken.mp4");
        std::fs::write(&broken, b"not a video").unwrap();
        let frames: Vec<_> = FfmpegSource::new(broken.clone()).frames().collect().await;
        assert!(
            matches!(frames.as_slice(), [Err(e)] if e.0.starts_with("ffmpeg: ")),
            "{frames:?}"
        );
        assert!(matches!(probe(&broken).await, Err(Unprobed::NotMedia)));
    }

    /// A program on PATH named ffprobe that does not run, like a mise shim with
    /// no version set, is no ffprobe.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_ffprobe_that_does_not_run_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let clip = directory.path().join("clip.mp4");
        std::fs::write(&clip, b"a video").unwrap();
        let ffprobe = crate::adapters::fake_program::fake_program(
            directory.path(),
            "ffprobe",
            "echo 'No version is set for shim: ffprobe' >&2; exit 1",
        );
        assert!(matches!(
            probe_with(&ffprobe, &clip).await,
            Err(Unprobed::Missing)
        ));
    }

    #[tokio::test]
    async fn without_ffprobe_it_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let clip = directory.path().join("clip.mp4");
        std::fs::write(&clip, b"a video").unwrap();
        let ffprobe = directory.path().join("no-ffprobe");
        assert!(matches!(
            probe_with(&ffprobe, &clip).await,
            Err(Unprobed::Missing)
        ));
    }
}
