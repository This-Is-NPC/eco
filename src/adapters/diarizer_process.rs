//! Diarization in a child `eco diarize`: the embedding model's memory goes back
//! to the system when it exits. Speech regions go to its stdin as
//! `start (u64 LE, samples) · length (u32 LE) · samples (i16 LE)`; at the end of
//! its input it prints JSON `{"turns": [[start, end, speaker], …], "voices": [[…], …]}`.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
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
    let (regions, received) = mpsc::channel();
    let model = model.to_path_buf();
    thread::spawn(move || found(diarize(&model, received)));
    regions
}

/// Send each region `regions` yields to a child diarizer, then return what it found.
fn diarize(model: &Path, regions: Receiver<Segment>) -> Result<Diarization, String> {
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
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
    let embedder = TractEmbedder::load(model).map_err(|e| anyhow::anyhow!(e.0))?;
    let mut diarizer = Diarizer::new(&embedder);
    let mut input = BufReader::new(std::io::stdin().lock());
    while let Some(region) = read_region(&mut input)? {
        let start = region.start as f64 / f64::from(SAMPLE_RATE);
        diarizer
            .add(start, &region.pcm)
            .map_err(|e| anyhow::anyhow!(e.0))?;
    }
    println!("{}", to_json(&diarizer.finish(Clustering::default())));
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
