//! Diarization in a child `eco diarize`: the embedding model's memory goes back
//! to the system when it exits. Speech regions go to its stdin as
//! `start (u64 LE, samples) · length (u32 LE) · samples (i16 LE)`; at the end of
//! its input it prints JSON `{"turns": [[start, end, speaker], …], "voices": [[…], …]}`.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde_json::{Value, json};

use crate::adapters::speaker_tract::TractEmbedder;
use crate::domain::diarization::{Clustering, Diarization, Diarizer, Turn};
use crate::domain::segmenter::Segment;
use crate::ports::SAMPLE_RATE;

fn write_region(out: &mut impl Write, region: &Segment) -> std::io::Result<()> {
    out.write_all(&(region.start as u64).to_le_bytes())?;
    out.write_all(&(region.pcm.len() as u32).to_le_bytes())?;
    for sample in &region.pcm {
        out.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

/// The next region on `input`, or `None` at its end.
fn read_region(input: &mut impl Read) -> std::io::Result<Option<Segment>> {
    let mut start = [0u8; 8];
    match input.read_exact(&mut start) {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        other => other?,
    }
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let mut bytes = vec![0u8; u32::from_le_bytes(length) as usize * 2];
    input.read_exact(&mut bytes)?;
    Ok(Some(Segment {
        start: u64::from_le_bytes(start) as usize,
        pcm: bytes
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect(),
    }))
}

fn to_json(diarization: &Diarization) -> Value {
    let turns: Vec<Value> = diarization
        .turns
        .iter()
        .map(|t| json!([t.start, t.end, t.speaker]))
        .collect();
    json!({"turns": turns, "voices": diarization.voices})
}

fn parse(text: &str) -> Option<Diarization> {
    #[derive(serde::Deserialize)]
    struct Reply {
        turns: Vec<(f64, f64, usize)>,
        voices: Vec<Vec<f32>>,
    }
    let reply: Reply = serde_json::from_str(text).ok()?;
    let turns = reply
        .turns
        .into_iter()
        .map(|(start, end, speaker)| Turn {
            start,
            end,
            speaker,
        })
        .collect();
    Some(Diarization {
        turns,
        voices: reply.voices,
    })
}

/// Start a child diarizer: each region sent goes to it, and once the sender is
/// dropped, `found` is called with what it found, on a thread of its own.
pub fn spawn(
    model: &Path,
    found: impl FnOnce(Result<Diarization, String>) + Send + 'static,
) -> Sender<Segment> {
    spawn_program(std::env::current_exe(), model, found)
}

/// `spawn`, with `program` as the child, or the error that it is unknown.
fn spawn_program(
    program: std::io::Result<PathBuf>,
    model: &Path,
    found: impl FnOnce(Result<Diarization, String>) + Send + 'static,
) -> Sender<Segment> {
    let (regions, received) = mpsc::channel();
    let model = model.to_path_buf();
    thread::spawn(move || {
        let program = program.map_err(|e| e.to_string());
        found(program.and_then(|program| diarize(&program, &model, received)))
    });
    regions
}

/// Send each region `regions` yields to the child diarizer `program`, then
/// return what it found.
fn diarize(
    program: &Path,
    model: &Path,
    regions: Receiver<Segment>,
) -> Result<Diarization, String> {
    let mut child = Command::new(program)
        .arg("diarize")
        .arg(model)
        // Each clip's tensors are sized by its length; without a fixed threshold
        // glibc keeps the freed ones, and the child grew to ~160 MB instead of ~70.
        .env("MALLOC_TRIM_THRESHOLD_", "1048576")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = BufWriter::new(child.stdin.take().expect("piped"));
    for region in regions {
        if write_region(&mut stdin, &region).is_err() {
            break;
        }
    }
    // Closing stdin tells the child the recording is over.
    drop(stdin.into_inner().map_err(|e| e.to_string())?);
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "the diarizer's reply is not turns and voices".into())
}

/// `eco diarize <model>`: the child side.
pub fn serve(model: &Path) -> anyhow::Result<()> {
    serve_on(model, std::io::stdin().lock(), std::io::stdout().lock())
}

/// `serve`, reading regions from `input` and writing the reply to `out`.
fn serve_on(model: &Path, input: impl Read, mut out: impl Write) -> anyhow::Result<()> {
    let embedder = TractEmbedder::load(model).map_err(|e| anyhow::anyhow!(e.0))?;
    let mut diarizer = Diarizer::new(&embedder);
    let mut input = BufReader::new(input);
    while let Some(region) = read_region(&mut input)? {
        let start = region.start as f64 / f64::from(SAMPLE_RATE);
        diarizer
            .add(start, &region.pcm)
            .map_err(|e| anyhow::anyhow!(e.0))?;
    }
    writeln!(out, "{}", to_json(&diarizer.finish(Clustering::default())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn regions_and_turns_cross_the_pipe_unchanged() {
        let regions = [
            Segment {
                pcm: vec![1, -2, i16::MAX, i16::MIN],
                start: 48_000,
            },
            Segment {
                pcm: Vec::new(),
                start: 7,
            },
        ];
        let mut bytes = Vec::new();
        for region in &regions {
            write_region(&mut bytes, region).unwrap();
        }
        let mut input = bytes.as_slice();
        assert_eq!(read_region(&mut input).unwrap().as_ref(), Some(&regions[0]));
        assert_eq!(read_region(&mut input).unwrap().as_ref(), Some(&regions[1]));
        assert_eq!(read_region(&mut input).unwrap(), None);

        let diarization = Diarization {
            turns: vec![Turn {
                start: 1.5,
                end: 3.25,
                speaker: 1,
            }],
            voices: vec![vec![0.6, 0.8], vec![1.0, 0.0]],
        };
        assert_eq!(parse(&to_json(&diarization).to_string()), Some(diarization));
        assert_eq!(parse("oops"), None);
    }

    /// A child that keeps its arguments, environment and input in `dir`, then
    /// runs `reply`.
    fn child(dir: &Path, reply: &str) -> PathBuf {
        crate::adapters::fake_program::fake_program(
            dir,
            "eco",
            &format!(
                r#"cd '{}'
echo "$1 $2 $MALLOC_TRIM_THRESHOLD_" > args
cat > input
{reply}"#,
                dir.display()
            ),
        )
    }

    /// What the child `program` found for `regions`, through `spawn`.
    fn found(
        program: std::io::Result<PathBuf>,
        regions: &[Segment],
    ) -> Result<Diarization, String> {
        let (tell, told) = mpsc::channel();
        let sender = spawn_program(program, Path::new("/m.onnx"), move |found| {
            tell.send(found).unwrap();
        });
        for region in regions {
            sender.send(region.clone()).unwrap();
        }
        drop(sender);
        told.recv().unwrap()
    }

    #[test]
    fn the_child_gets_the_regions_and_its_reply_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let program = child(
            dir.path(),
            r#"echo '{"turns": [[0.5, 2.0, 0]], "voices": [[1.0]]}'"#,
        );
        let region = Segment {
            pcm: vec![3, -3],
            start: 16_000,
        };
        let diarization = found(Ok(program), std::slice::from_ref(&region)).unwrap();
        assert_eq!(
            diarization,
            Diarization {
                turns: vec![Turn {
                    start: 0.5,
                    end: 2.0,
                    speaker: 0,
                }],
                voices: vec![vec![1.0]],
            }
        );
        let mut sent = Vec::new();
        write_region(&mut sent, &region).unwrap();
        assert_eq!(fs::read(dir.path().join("input")).unwrap(), sent);
        assert_eq!(
            fs::read_to_string(dir.path().join("args")).unwrap(),
            "diarize /m.onnx 1048576\n"
        );
    }

    #[test]
    fn a_child_that_fails_or_says_nonsense_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let failing = child(dir.path(), "echo '  no model at /m.onnx  ' >&2; exit 1");
        assert_eq!(found(Ok(failing), &[]), Err("no model at /m.onnx".into()));

        let dir = tempfile::tempdir().unwrap();
        let confused = child(dir.path(), "echo oops");
        assert_eq!(
            found(Ok(confused), &[]),
            Err("the diarizer's reply is not turns and voices".into())
        );
    }

    #[test]
    fn a_child_that_stops_reading_ends_the_diarization() {
        let dir = tempfile::tempdir().unwrap();
        let program = crate::adapters::fake_program::fake_program(
            dir.path(),
            "eco",
            "echo 'out of memory' >&2; exit 1",
        );
        let long = Segment {
            pcm: vec![0; 200_000],
            start: 0,
        };
        assert!(found(Ok(program), &[long.clone(), long]).is_err());
    }

    #[test]
    fn a_child_that_cannot_start_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = found(Ok(dir.path().join("eco")), &[]).unwrap_err();
        assert!(missing.starts_with("No such file"), "{missing}");
        let unknown = found(Err(std::io::Error::other("no exe")), &[]);
        assert_eq!(unknown, Err("no exe".into()));
    }

    /// The child side over the speaker model and three AMI clips (see
    /// `speaker_tract`): one turn per clip, a voice per speaker; a model that
    /// is not there is an error.
    #[test]
    fn the_child_side_diarizes_its_input() {
        let model = crate::paths::speaker_model();
        assert!(model.exists(), "run `mise run setup` first");
        let bytes = include_bytes!("../../tests/fixtures/speaker-clips.s16");
        let mut input = Vec::new();
        for (index, clip) in bytes.chunks(bytes.len() / 3).enumerate() {
            let region = Segment {
                pcm: clip
                    .chunks_exact(2)
                    .map(|p| i16::from_le_bytes([p[0], p[1]]))
                    .collect(),
                start: index * 40_000,
            };
            write_region(&mut input, &region).unwrap();
        }
        let mut out = Vec::new();
        serve_on(&model, input.as_slice(), &mut out).unwrap();
        let diarization = parse(&String::from_utf8(out).unwrap()).unwrap();
        let speakers: Vec<usize> = diarization.turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, [0, 1, 2]);
        assert_eq!(diarization.voices.len(), 3);

        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("model.onnx");
        assert!(serve_on(&missing, &[][..], Vec::new()).is_err());
    }
}
