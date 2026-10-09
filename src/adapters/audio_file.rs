//! Replay a WAV file instead of capturing, to tune and test without a call.

use std::path::PathBuf;
use std::time::Duration;

use futures::stream::{self, BoxStream, StreamExt};
use tokio::time::{Instant, sleep_until};

use crate::ports::{AudioError, AudioSource, FRAME_SAMPLES, Frame, SAMPLE_RATE};

const FRAME_DURATION: Duration =
    Duration::from_millis(FRAME_SAMPLES as u64 * 1000 / SAMPLE_RATE as u64);

/// A 16 kHz mono s16le WAV, paced in real time unless told otherwise.
pub struct WavFileSource {
    path: PathBuf,
    realtime: bool,
}

impl WavFileSource {
    pub fn new(path: PathBuf, realtime: bool) -> Self {
        Self { path, realtime }
    }

    fn samples(&self) -> Result<Vec<i16>, AudioError> {
        let reader = hound::WavReader::open(&self.path)
            .map_err(|error| AudioError(format!("{}: {error}", self.path.display())))?;
        let spec = reader.spec();
        if (spec.sample_rate, spec.channels, spec.bits_per_sample) != (SAMPLE_RATE, 1, 16) {
            return Err(AudioError(format!(
                "{}: expected 16 kHz mono s16le; convert with `ffmpeg -i in -ar {SAMPLE_RATE} -ac 1 -sample_fmt s16 out.wav`",
                self.path.display()
            )));
        }
        reader
            .into_samples::<i16>()
            .collect::<Result<_, _>>()
            .map_err(|error| AudioError(format!("{}: {error}", self.path.display())))
    }
}

impl AudioSource for WavFileSource {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        let samples = match self.samples() {
            Ok(samples) => samples,
            Err(error) => return stream::once(async move { Err(error) }).boxed(),
        };
        let realtime = self.realtime;
        let started = Instant::now();
        stream::unfold((samples, 0usize), move |(samples, sent)| async move {
            let start = sent * FRAME_SAMPLES;
            if start >= samples.len() {
                return None;
            }
            if realtime {
                sleep_until(started + FRAME_DURATION * sent as u32).await;
            } else {
                tokio::task::yield_now().await;
            }
            // The last frame is padded with silence to a full frame.
            let mut frame = samples[start..samples.len().min(start + FRAME_SAMPLES)].to_vec();
            frame.resize(FRAME_SAMPLES, 0);
            Some((Ok(frame), (samples, sent + 1)))
        })
        .boxed()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use futures::StreamExt;

    use super::*;

    pub(crate) fn write_wav(path: &Path, samples: &[i16], rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &sample in samples {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[tokio::test]
    async fn replays_whole_frames_padding_the_last() {
        // The 22.32 s test recording: 357120 samples, 698 frames.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.wav");
        let samples: Vec<i16> = (0..357_120).map(|i| (i % 1000) as i16).collect();
        write_wav(&path, &samples, SAMPLE_RATE);
        let mut source = WavFileSource::new(path, false);
        let frames: Vec<Frame> = source.frames().map(Result::unwrap).collect().await;
        assert_eq!(frames.len(), 698);
        assert!(frames.iter().all(|frame| frame.len() == FRAME_SAMPLES));
        let last = frames.last().unwrap();
        assert_eq!(last[255], samples[357_119]);
        assert!(last[256..].iter().all(|&sample| sample == 0));
    }

    #[tokio::test]
    async fn rejects_other_formats() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cd.wav");
        write_wav(&path, &[0; 100], 44_100);
        let mut source = WavFileSource::new(path, false);
        let first = source.frames().next().await.unwrap();
        assert!(first.unwrap_err().0.contains("expected 16 kHz mono s16le"));
    }

    #[tokio::test(start_paused = true)]
    async fn realtime_replay_keeps_the_frame_pace() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("short.wav");
        write_wav(&path, &[0; FRAME_SAMPLES * 10], SAMPLE_RATE);
        let started = Instant::now();
        let frames = WavFileSource::new(path, true).frames().count().await;
        assert_eq!(frames, 10);
        assert_eq!(started.elapsed(), FRAME_DURATION * 9);
    }
}
