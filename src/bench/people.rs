//! How safely people are named by voice: AMI series hold the same four people
//! in meetings a–d. Each speaker eco finds in an `a` meeting is enrolled under
//! the reference speaker it overlaps most; the speakers of the other meetings
//! are then ranked against everyone enrolled, from every series.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::adapters::speaker_tract::TractEmbedder;
use crate::adapters::vad_silero::SileroModel;
use crate::bench::der::{Segment, parse_rttm};
use crate::bench::diarization::diarize;
use crate::config;
use crate::domain::diarization::{Clustering, Diarization};
use crate::domain::people::{Person, ranked};

/// The reference speaker each found speaker overlaps most.
fn references(diarization: &Diarization, reference: &[Segment]) -> Vec<Option<String>> {
    (0..diarization.voices.len())
        .map(|speaker| {
            let mut talk: BTreeMap<&str, f64> = BTreeMap::new();
            for turn in diarization.turns.iter().filter(|t| t.speaker == speaker) {
                for segment in reference {
                    let overlap = turn.end.min(segment.end) - turn.start.max(segment.start);
                    if overlap > 0.0 {
                        *talk.entry(&segment.speaker).or_default() += overlap;
                    }
                }
            }
            let most = talk.into_iter().max_by(|a, b| a.1.total_cmp(&b.1));
            most.map(|(name, _)| name.to_string())
        })
        .collect()
}

pub async fn run(dir: PathBuf, series: Vec<String>) -> Result<()> {
    let embedder =
        TractEmbedder::load(&config::speaker_model()).map_err(|e| anyhow::anyhow!(e.0))?;
    let vad = SileroModel::load(&config::vad_model())?;
    let mut people: Vec<Person> = Vec::new();
    // (reference speaker, voice) of every speaker found outside the `a` meetings.
    let mut heard: Vec<(String, Vec<f32>)> = Vec::new();
    for prefix in &series {
        for meeting in ["a", "b", "c", "d"] {
            let name = format!("{prefix}{meeting}");
            let base = dir.join(&name);
            let reference = parse_rttm(
                &std::fs::read_to_string(base.with_extension("rttm"))
                    .with_context(|| base.display().to_string())?,
            )
            .map_err(anyhow::Error::msg)?;
            let (diarizer, _) = diarize(&base.with_extension("wav"), &vad, &embedder).await?;
            let diarization = diarizer.finish(Clustering::default());
            let found = references(&diarization, &reference);
            println!("{name}: {} speakers found", found.len());
            for (index, (voice, who)) in diarization.voices.into_iter().zip(found).enumerate() {
                let Some(who) = who else { continue };
                if meeting == "a" {
                    let label = format!("Speaker {}", index + 1);
                    match people.iter_mut().find(|p| p.name == who) {
                        Some(person) => person.enroll(&name, &label, voice),
                        None => {
                            let mut person = Person::new(&who);
                            person.enroll(&name, &label, voice);
                            people.push(person);
                        }
                    }
                } else {
                    heard.push((who, voice));
                }
            }
        }
    }
    // Each heard voice: its best match, and whether that is the right person.
    let mut best: Vec<(f32, bool)> = heard
        .iter()
        .map(|(who, voice)| {
            let top = ranked(&people, voice).remove(0);
            let right = people
                .iter()
                .find(|p| p.id == top.person)
                .is_some_and(|p| &p.name == who);
            (top.score, right)
        })
        .collect();
    best.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "\n{} people enrolled, {} voices to name",
        people.len(),
        heard.len()
    );
    println!("{:>9} {:>7} {:>6}", "threshold", "named", "wrong");
    for step in 0..=12 {
        let threshold = 0.3 + 0.05 * step as f32;
        let named: Vec<_> = best.iter().filter(|b| b.0 >= threshold).collect();
        let wrong = named.iter().filter(|b| !b.1).count();
        println!("{threshold:>9.2} {:>7} {wrong:>6}", named.len());
    }
    let worst_wrong = best
        .iter()
        .filter(|b| !b.1)
        .map(|b| b.0)
        .fold(f32::MIN, f32::max);
    let right: Vec<f32> = best.iter().filter(|b| b.1).map(|b| b.0).collect();
    println!(
        "\nhighest wrong match {worst_wrong:.3}; right matches {:.3}–{:.3}",
        right.iter().copied().fold(f32::MAX, f32::min),
        right.iter().copied().fold(f32::MIN, f32::max)
    );
    Ok(())
}
