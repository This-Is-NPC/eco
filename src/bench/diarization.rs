//! Diarization error rate of eco's diarization over recordings that come with a
//! reference: `<name>.wav` (16 kHz mono), `<name>.rttm` and optionally `<name>.uem`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::adapters::audio_file::WavFileSource;
use crate::adapters::speaker_tract::TractEmbedder;
use crate::adapters::vad_silero::{SileroModel, SileroVad};
use crate::bench::clusters::{
    Agreement, Churn, LIMITS, Overlap, lifetimes, margins, overlap, replay, small, spread,
};
use crate::bench::der::{Score, Segment, parse_rttm, parse_uem, score};
use crate::bench::{embeddings, linkage};
use crate::domain::channel::segment_channel;
use crate::domain::diarization::{Clustering, Region, Turn, region};
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
    /// One table per combination of the lists below, from the same embeddings.
    pub thresholds: Vec<f32>,
    /// Merge distances to compare, crossed with the thresholds.
    pub merges: Vec<f32>,
    /// Smallest shares of the windows a speaker may hold, to compare.
    pub min_shares: Vec<f32>,
    /// Windows heard before a small speaker is folded, to compare.
    pub settles: Vec<usize>,
    /// Seconds forgiven around each reference boundary, half on each side.
    pub collar: f64,
    /// Where to write eco's turns as `<name>.rttm`, for the first threshold.
    pub output: Option<PathBuf>,
    /// Where to write each recording's window embeddings as `<name>.emb`.
    pub dump: Option<PathBuf>,
}

/// The average-linkage clustering at the end of a recording stops here, the
/// distance that gave the lowest DER on the dev set.
const OFFLINE_THRESHOLD: f32 = 0.65;

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

/// The speech regions of a recording, embedded, and its length in seconds.
pub async fn diarize(
    path: &Path,
    vad: &SileroModel,
    embedder: &TractEmbedder,
) -> Result<(Vec<Region>, f64)> {
    let mut source = WavFileSource::new(path.into(), false);
    let mut vad = SileroVad::new(vad.clone());
    let mut probability = |frame: &[i16]| {
        vad.probability(frame)
            .map_err(|e| AudioError(e.to_string()))
    };
    let mut regions = Vec::new();
    let (mut failed, mut heard) = (None, 0usize);
    let mut add = |segment: crate::domain::segmenter::Segment| {
        heard = heard.max(segment.start + segment.pcm.len());
        let start = segment.start as f64 / f64::from(SAMPLE_RATE);
        match region(embedder, start, &segment.pcm) {
            Ok(region) => regions.push(region),
            Err(e) => {
                failed.get_or_insert(e);
            }
        }
    };
    segment_channel(
        &mut source,
        None,
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
    Ok((regions, heard as f64 / f64::from(SAMPLE_RATE)))
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

/// One recording with its reference and its embedded speech regions.
struct Meeting {
    name: String,
    reference: Vec<Segment>,
    uem: Option<Vec<(f64, f64)>>,
    regions: Vec<Region>,
    /// The middle of each window, in seconds.
    centres: Vec<f64>,
}

/// One clustering of a meeting scored against its reference.
struct Evaluation {
    turns: Vec<Turn>,
    result: Score,
    overlap: Overlap,
    agreement: Agreement,
}

impl Meeting {
    fn evaluate(&self, turns: Vec<Turn>, collar: f64) -> Evaluation {
        let hypothesis = hypothesis(&turns);
        let uem = self.uem.as_deref();
        let overlap = overlap(&self.reference, &hypothesis, uem);
        Evaluation {
            result: score(&self.reference, &hypothesis, uem, collar),
            agreement: Agreement::of(&overlap),
            overlap,
            turns,
        }
    }
}

fn found(turns: &[Turn]) -> usize {
    turns.iter().map(|t| t.speaker + 1).max().unwrap_or(0)
}

fn percent(part: f64, score: &Score) -> String {
    format!("{:.1}", 100.0 * part / score.total)
}

fn ratio(share: f64) -> String {
    format!("{:.1}", 100.0 * share)
}

const COLUMNS: &str = "recording speakers found missed f.alarm confus. DER% purity cover <5% churn% stayed | off.found DER% purity cover";

/// The found speakers of one evaluation, each with how long it lived, and how
/// sure the windows were of their nearest voice.
fn print_detail(meeting: &Meeting, evaluation: &Evaluation, voices: &[Vec<f32>]) {
    println!("  {}: found speakers", meeting.name);
    println!("    speaker windows seconds first last main reference (share of theirs)");
    for life in lifetimes(&evaluation.turns, &meeting.centres, &evaluation.overlap) {
        let main = life.main.map_or(String::from("-"), |(name, share)| {
            format!("{name} ({})", ratio(share))
        });
        println!(
            "    {:>7} {:>7} {:>7.0} {:>5.0} {:>4.0} {main}",
            life.speaker, life.windows, life.seconds, life.first, life.last
        );
    }
    match spread(&margins(&meeting.regions, voices)) {
        Some(s) => println!(
            "    margin of {} windows: under {} {} % ; p10 {:.3} p50 {:.3} p90 {:.3}",
            s.windows,
            LIMITS.map(|l| l.to_string()).join(" / "),
            s.below.map(ratio).join(" / "),
            s.percentiles[0],
            s.percentiles[1],
            s.percentiles[2],
        ),
        None => println!("    margin: fewer than two voices"),
    }
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
    let mut meetings = Vec::new();
    for name in recordings(&options)? {
        let base = options.dir.join(&name);
        let reference = parse_rttm(&std::fs::read_to_string(base.with_extension("rttm"))?)
            .map_err(anyhow::Error::msg)?;
        let uem = match std::fs::read_to_string(base.with_extension("uem")) {
            Ok(text) => Some(parse_uem(&text).map_err(anyhow::Error::msg)?),
            Err(_) => None,
        };
        let started = Instant::now();
        let (regions, length) = diarize(&base.with_extension("wav"), &vad, &embedder).await?;
        let took = started.elapsed().as_secs_f64();
        println!(
            "{name}: {:.1} min in {took:.1} s ({:.0}x real time)",
            length / 60.0,
            length / took
        );
        if let Some(dir) = &options.dump {
            embeddings::write(&dir.join(format!("{name}.emb")), &regions)?;
        }
        let centres = regions
            .iter()
            .flat_map(Region::windows)
            .map(|w| (w.0 + w.1) / 2.0)
            .collect();
        meetings.push(Meeting {
            name,
            reference,
            uem,
            regions,
            centres,
        });
    }
    let offline_share = options.min_shares[0];
    let offline: Vec<Evaluation> = meetings
        .iter()
        .map(|m| {
            let turns = linkage::diarize(&m.regions, OFFLINE_THRESHOLD, offline_share);
            m.evaluate(turns, options.collar)
        })
        .collect();
    let mut first = true;
    for &threshold in &options.thresholds {
        for &merge in &options.merges {
            for &min_share in &options.min_shares {
                for &settle in &options.settles {
                    let clustering = Clustering {
                        threshold,
                        merge,
                        min_share,
                        settle,
                    };
                    println!(
                        "\nonline: threshold {threshold}, merge {merge}, min share {min_share}, settle {settle}; offline: average linkage at {OFFLINE_THRESHOLD}, min share {offline_share}"
                    );
                    println!("{COLUMNS}");
                    let (mut all, mut agreement, mut churn) =
                        (Score::default(), Agreement::default(), Churn::default());
                    let (mut off, mut off_agreement) = (Score::default(), Agreement::default());
                    let mut extras = 0;
                    let mut details = Vec::new();
                    for (meeting, offline) in meetings.iter().zip(&offline) {
                        let (online, moved) = replay(&meeting.regions, clustering);
                        if let Some(dir) = options.output.as_ref().filter(|_| first) {
                            write_rttm(
                                &dir.join(format!("{}.rttm", meeting.name)),
                                &meeting.name,
                                &online.turns,
                            )?;
                        }
                        let voices = online.voices;
                        let evaluation = meeting.evaluate(online.turns, options.collar);
                        let (result, offline_result) = (evaluation.result, offline.result);
                        let speakers = evaluation.overlap.reference.len();
                        let lives =
                            lifetimes(&evaluation.turns, &meeting.centres, &evaluation.overlap);
                        let under = small(&lives);
                        println!(
                            "{:10} {speakers:>8} {:>5} {:>6} {:>7} {:>7} {:>5} {:>6} {:>5} {under:>3} {:>6} {:>6.0} | {:>9} {:>4} {:>6} {:>5}",
                            meeting.name,
                            found(&evaluation.turns),
                            percent(result.missed, &result),
                            percent(result.false_alarm, &result),
                            percent(result.confusion, &result),
                            format!("{:.1}", 100.0 * result.der()),
                            ratio(evaluation.agreement.purity()),
                            ratio(evaluation.agreement.coverage()),
                            ratio(moved.changed / moved.speech),
                            moved.stayed(),
                            found(&offline.turns),
                            format!("{:.1}", 100.0 * offline_result.der()),
                            ratio(offline.agreement.purity()),
                            ratio(offline.agreement.coverage()),
                        );
                        all.add(result);
                        agreement.add(evaluation.agreement);
                        churn.add(moved);
                        off.add(offline_result);
                        off_agreement.add(offline.agreement);
                        extras += under;
                        if first {
                            details.push((meeting, evaluation, voices));
                        }
                    }
                    println!(
                        "{:10} {:>8} {:>5} {:>6} {:>7} {:>7} {:>5} {:>6} {:>5} {extras:>3} {:>6} {:>6.0} | {:>9} {:>4} {:>6} {:>5}",
                        "all",
                        "",
                        "",
                        percent(all.missed, &all),
                        percent(all.false_alarm, &all),
                        percent(all.confusion, &all),
                        format!("{:.1}", 100.0 * all.der()),
                        ratio(agreement.purity()),
                        ratio(agreement.coverage()),
                        ratio(churn.changed / churn.speech),
                        churn.stayed(),
                        "",
                        format!("{:.1}", 100.0 * off.der()),
                        ratio(off_agreement.purity()),
                        ratio(off_agreement.coverage()),
                    );
                    println!(
                        "churn: {:.0} s of {:.0} s changed speaker after being told; longest {:.0} s",
                        churn.changed, churn.speech, churn.longest
                    );
                    for (meeting, evaluation, voices) in details {
                        print_detail(meeting, &evaluation, &voices);
                    }
                    first = false;
                }
            }
        }
    }
    if let Some(peak) = peak_rss_mb() {
        println!("\npeak RSS {peak:.0} MB");
    }
    Ok(())
}
