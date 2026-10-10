//! Diarization error rate of eco's diarization over recordings that come with a
//! reference: `<name>.wav` (16 kHz mono), `<name>.rttm` and optionally `<name>.uem`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::adapters::audio_file::WavFileSource;
use crate::adapters::speaker_tract::TractEmbedder;
use crate::adapters::vad_silero::{SileroModel, SileroVad};
use crate::bench::der::{Score, Segment, parse_rttm, parse_uem, score};
use crate::domain::channel::segment_channel;
use crate::domain::diarization::{Clustering, Diarizer, Turn};
use crate::domain::segmenter::SegmenterConfig;
use crate::paths;
use crate::ports::{AudioError, SAMPLE_RATE};

pub struct Options {
    /// Where the recordings and their references are.
    pub dir: PathBuf,
    /// A file naming the recordings to run, one per line; every WAV with an RTTM otherwise.
    pub list: Option<PathBuf>,
    /// The speaker embedding model; the one `eco setup` downloads by default.
    pub model: Option<PathBuf>,
    /// One table per threshold, from the same embeddings.
    pub thresholds: Vec<f32>,
    pub min_share: f32,
    /// Seconds forgiven around each reference boundary, half on each side.
    pub collar: f64,
    /// Where to write eco's turns as `<name>.rttm`, for the first threshold.
    pub output: Option<PathBuf>,
}

fn recordings(options: &Options) -> Result<Vec<String>> {
    let names: Vec<String> = match &options.list {
        Some(list) => std::fs::read_to_string(list)
            .with_context(|| list.display().to_string())?
            .split_whitespace()
            .map(String::from)
            .collect(),
        None => {
            let mut names: Vec<String> = std::fs::read_dir(&options.dir)?
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    let name = path.file_stem()?.to_str()?.to_string();
                    (path.extension()? == "wav").then_some(name)
                })
                .filter(|name| options.dir.join(format!("{name}.rttm")).exists())
                .collect();
            names.sort();
            names
        }
    };
    if names.is_empty() {
        bail!("no recordings with an RTTM in {}", options.dir.display());
    }
    Ok(names)
}

/// The peak resident memory of this process so far, in MB.
fn peak_rss_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

/// The speech regions of a recording, embedded.
pub async fn diarize<'a>(
    path: &Path,
    vad: &SileroModel,
    embedder: &'a TractEmbedder,
) -> Result<(Diarizer<'a>, f64)> {
    let mut source = WavFileSource::new(path.into(), false);
    let mut vad = SileroVad::new(vad.clone());
    let mut probability = |frame: &[i16]| {
        vad.probability(frame)
            .map_err(|e| AudioError(e.to_string()))
    };
    let mut diarizer = Diarizer::new(embedder);
    let (mut failed, mut heard) = (None, 0usize);
    let mut add = |segment: crate::domain::segmenter::Segment| {
        heard = heard.max(segment.start + segment.pcm.len());
        let start = segment.start as f64 / f64::from(SAMPLE_RATE);
        if let Err(e) = diarizer.add(start, &segment.pcm) {
            failed.get_or_insert(e);
        }
    };
    segment_channel(
        &mut source,
        &mut probability,
        &|_, _| {},
        SegmenterConfig::default(),
        &mut add,
        &mut |_| {},
    )
    .await?;
    if let Some(e) = failed {
        bail!("{}: {e}", path.display());
    }
    Ok((diarizer, heard as f64 / f64::from(SAMPLE_RATE)))
}

fn hypothesis(turns: &[Turn]) -> Vec<Segment> {
    turns
        .iter()
        .map(|t| Segment {
            start: t.start,
            end: t.end,
            speaker: t.speaker.to_string(),
        })
        .collect()
}

fn write_rttm(path: &Path, name: &str, turns: &[Turn]) -> Result<()> {
    let lines: String = turns
        .iter()
        .map(|t| {
            let (start, duration) = (t.start, t.end - t.start);
            format!(
                "SPEAKER {name} 1 {start:.3} {duration:.3} <NA> <NA> speaker{} <NA> <NA>\n",
                t.speaker
            )
        })
        .collect();
    std::fs::write(path, lines).with_context(|| path.display().to_string())
}

fn percent(part: f64, score: &Score) -> String {
    format!("{:.1}", 100.0 * part / score.total)
}

pub async fn run(options: Options) -> Result<()> {
    let model = options.model.clone().unwrap_or_else(paths::speaker_model);
    let loading = Instant::now();
    let embedder = TractEmbedder::load(&model).map_err(|e| anyhow::anyhow!(e.0))?;
    let vad = SileroModel::load(&paths::vad_model())?;
    println!(
        "model {} loaded in {} ms; collar {} s",
        model.display(),
        loading.elapsed().as_millis(),
        options.collar
    );
    let mut runs = Vec::new();
    for name in recordings(&options)? {
        let base = options.dir.join(&name);
        let reference = parse_rttm(&std::fs::read_to_string(base.with_extension("rttm"))?)
            .map_err(anyhow::Error::msg)?;
        let uem = match std::fs::read_to_string(base.with_extension("uem")) {
            Ok(text) => Some(parse_uem(&text).map_err(anyhow::Error::msg)?),
            Err(_) => None,
        };
        let started = Instant::now();
        let (diarizer, length) = diarize(&base.with_extension("wav"), &vad, &embedder).await?;
        let took = started.elapsed().as_secs_f64();
        println!(
            "{name}: {:.1} min in {took:.1} s ({:.0}x real time)",
            length / 60.0,
            length / took
        );
        runs.push((name, reference, uem, diarizer));
    }
    for (index, &threshold) in options.thresholds.iter().enumerate() {
        let clustering = Clustering {
            threshold,
            min_share: options.min_share,
        };
        println!("\nthreshold {threshold}, min share {}", options.min_share);
        println!(
            "{:10} {:>8} {:>8} {:>7} {:>7} {:>7} {:>6}",
            "recording", "speakers", "found", "missed", "f.alarm", "confus.", "DER %"
        );
        let mut all = Score::default();
        for (name, reference, uem, diarizer) in &runs {
            let turns = diarizer.finish(clustering).turns;
            if let Some(dir) = options.output.as_ref().filter(|_| index == 0) {
                write_rttm(&dir.join(format!("{name}.rttm")), name, &turns)?;
            }
            let found = turns.iter().map(|t| t.speaker + 1).max().unwrap_or(0);
            let speakers = {
                let mut names: Vec<&str> = reference.iter().map(|s| s.speaker.as_str()).collect();
                names.sort();
                names.dedup();
                names.len()
            };
            let result = score(
                reference,
                &hypothesis(&turns),
                uem.as_deref(),
                options.collar,
            );
            println!(
                "{name:10} {speakers:>8} {found:>8} {:>7} {:>7} {:>7} {:>6}",
                percent(result.missed, &result),
                percent(result.false_alarm, &result),
                percent(result.confusion, &result),
                format!("{:.1}", 100.0 * result.der()),
            );
            all.add(result);
        }
        println!(
            "{:10} {:>8} {:>8} {:>7} {:>7} {:>7} {:>6}",
            "all",
            "",
            "",
            percent(all.missed, &all),
            percent(all.false_alarm, &all),
            percent(all.confusion, &all),
            format!("{:.1}", 100.0 * all.der()),
        );
    }
    if let Some(peak) = peak_rss_mb() {
        println!("\npeak RSS {peak:.0} MB");
    }
    Ok(())
}
