//! What a diarization did to the speakers, beyond the error rate: how pure and
//! complete they are against a reference, how long each found speaker lived,
//! how sure each window was of its speaker, and how often the online path
//! changed its mind.

use std::collections::BTreeMap;

use crate::bench::der::{Segment, crop, labels};
use crate::domain::diarization::{Clustering, Diarization, Recording, Region, Speakers, Turn, dot};

/// Seconds of speech shared by each reference speaker (rows) and each found
/// speaker (columns), within the scored regions.
pub struct Overlap {
    pub reference: Vec<String>,
    pub found: Vec<String>,
    pub seconds: Vec<Vec<f64>>,
}

pub fn overlap(
    reference: &[Segment],
    hypothesis: &[Segment],
    uem: Option<&[(f64, f64)]>,
) -> Overlap {
    let everything = [(f64::NEG_INFINITY, f64::INFINITY)];
    let regions = uem.unwrap_or(&everything);
    let (reference, hypothesis) = (crop(reference, regions), crop(hypothesis, regions));
    let (said, guessed) = (labels(&reference), labels(&hypothesis));
    let mut seconds = vec![vec![0.0; guessed.len()]; said.len()];
    for r in &reference {
        let row = said.iter().position(|l| *l == r.speaker).expect("listed");
        for h in &hypothesis {
            let shared = r.end.min(h.end) - r.start.max(h.start);
            if shared > 0.0 {
                let column = guessed
                    .iter()
                    .position(|l| *l == h.speaker)
                    .expect("listed");
                seconds[row][column] += shared;
            }
        }
    }
    Overlap {
        reference: said,
        found: guessed,
        seconds,
    }
}

/// Seconds of speech that agree with the main partner of their speaker, summed
/// so that meetings add up.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Agreement {
    /// In each found speaker, the time in its main reference speaker.
    pub pure: f64,
    /// In each reference speaker, the time in its main found speaker.
    pub covered: f64,
    /// Speech both the reference and the hypothesis hold.
    pub total: f64,
}

impl Agreement {
    pub fn of(overlap: &Overlap) -> Self {
        let rows = overlap.seconds.iter();
        let covered = rows
            .map(|row| row.iter().copied().fold(0.0, f64::max))
            .sum();
        let pure = (0..overlap.found.len())
            .map(|column| {
                let column = overlap.seconds.iter().map(|row| row[column]);
                column.fold(0.0, f64::max)
            })
            .sum();
        Self {
            pure,
            covered,
            total: overlap.seconds.iter().flatten().sum(),
        }
    }

    /// Share of a found speaker's time that is in its main reference speaker.
    pub fn purity(&self) -> f64 {
        self.pure / self.total
    }

    /// Share of a reference speaker's time that is in its main found speaker.
    pub fn coverage(&self) -> f64 {
        self.covered / self.total
    }

    pub fn add(&mut self, other: Self) {
        self.pure += other.pure;
        self.covered += other.covered;
        self.total += other.total;
    }
}

/// One found speaker over the recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Lifetime {
    pub speaker: usize,
    pub windows: usize,
    pub seconds: f64,
    pub first: f64,
    pub last: f64,
    /// The reference speaker they share most time with, and that time's share
    /// of theirs.
    pub main: Option<(String, f64)>,
}

/// Each speaker of `turns` with the windows (`centres`, in seconds) that fall
/// in their turns.
pub fn lifetimes(turns: &[Turn], centres: &[f64], overlap: &Overlap) -> Vec<Lifetime> {
    let mut found: BTreeMap<usize, Lifetime> = BTreeMap::new();
    for turn in turns {
        let life = found.entry(turn.speaker).or_insert_with(|| Lifetime {
            speaker: turn.speaker,
            windows: 0,
            seconds: 0.0,
            first: turn.start,
            last: turn.end,
            main: None,
        });
        life.seconds += turn.end - turn.start;
        life.first = life.first.min(turn.start);
        life.last = life.last.max(turn.end);
        life.windows += centres
            .iter()
            .filter(|&&at| turn.start <= at && at < turn.end)
            .count();
    }
    for life in found.values_mut() {
        let column = overlap
            .found
            .iter()
            .position(|label| *label == life.speaker.to_string());
        life.main = column.and_then(|column| {
            let shared = overlap.seconds.iter().map(|row| row[column]);
            let total: f64 = shared.clone().sum();
            if total == 0.0 {
                return None;
            }
            let (row, best) = shared.enumerate().max_by(|a, b| a.1.total_cmp(&b.1))?;
            Some((overlap.reference[row].clone(), best / total))
        });
    }
    found.into_values().collect()
}

/// Found speakers holding less than 5% of the speech.
pub fn small(lifetimes: &[Lifetime]) -> usize {
    let total: f64 = lifetimes.iter().map(|l| l.seconds).sum();
    lifetimes
        .iter()
        .filter(|l| l.seconds < 0.05 * total)
        .count()
}

/// Per window, the gap in cosine similarity between its nearest and its
/// second-nearest voice among `voices`; none with fewer than two voices.
pub fn margins(regions: &[Region], voices: &[Vec<f32>]) -> Vec<f32> {
    if voices.len() < 2 {
        return Vec::new();
    }
    regions
        .iter()
        .flat_map(Region::windows)
        .map(|(_, _, embedding)| {
            let (mut best, mut second) = (f32::MIN, f32::MIN);
            for voice in voices {
                let similarity = dot(voice, embedding);
                if similarity > best {
                    (best, second) = (similarity, best);
                } else if similarity > second {
                    second = similarity;
                }
            }
            best - second
        })
        .collect()
}

/// How windows spread over margins: the share under each of `LIMITS`, and the
/// 10th, 50th and 90th percentiles.
#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    pub windows: usize,
    pub below: [f64; 3],
    pub percentiles: [f32; 3],
}

pub const LIMITS: [f32; 3] = [0.05, 0.1, 0.2];

pub fn spread(margins: &[f32]) -> Option<Spread> {
    if margins.is_empty() {
        return None;
    }
    let mut sorted = margins.to_vec();
    sorted.sort_by(f32::total_cmp);
    let share =
        |limit: f32| sorted.iter().filter(|&&m| m < limit).count() as f64 / sorted.len() as f64;
    let at = |percent: usize| sorted[(sorted.len() - 1) * percent / 100];
    Some(Spread {
        windows: sorted.len(),
        below: LIMITS.map(share),
        percentiles: [at(10), at(50), at(90)],
    })
}

/// Speech whose speaker was told one way and ended another, summed so that
/// meetings add up.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Churn {
    /// Seconds of speech told.
    pub speech: f64,
    /// Seconds of speech whose speaker changed after it was first told.
    pub changed: f64,
    /// Seconds of speech times seconds of recording it stayed with the old speaker.
    pub wrong: f64,
    /// The longest a changed stretch stayed with the old speaker.
    pub longest: f64,
}

impl Churn {
    /// Seconds of recording a changed second stayed wrong, on average.
    pub fn stayed(&self) -> f64 {
        if self.changed == 0.0 {
            return 0.0;
        }
        self.wrong / self.changed
    }

    pub fn add(&mut self, other: Self) {
        self.speech += other.speech;
        self.changed += other.changed;
        self.wrong += other.wrong;
        self.longest = self.longest.max(other.longest);
    }
}

/// Who spoke when in `regions`, heard in order as a session hears them, and how
/// much of what was told changed afterwards.
pub fn replay(regions: &[Region], clustering: Clustering) -> (Diarization, Churn) {
    let mut speakers = Speakers::new(clustering);
    let mut recording = Recording::default();
    // Each stretch as told, and when; each speaker joined, into whom, and when.
    let mut told: Vec<(Turn, f64)> = Vec::new();
    let mut joined: BTreeMap<usize, (usize, f64)> = BTreeMap::new();
    for region in regions {
        let update = speakers.hear(region);
        told.extend(update.turns.iter().map(|t| (t.clone(), update.end)));
        joined.extend(
            update
                .merged
                .iter()
                .map(|&(gone, kept)| (gone, (kept, update.end))),
        );
        recording.apply(update);
    }
    let mut churn = Churn::default();
    for (turn, at) in &told {
        let seconds = turn.end - turn.start;
        churn.speech += seconds;
        let (mut speaker, mut changed) = (turn.speaker, *at);
        while let Some(&(kept, when)) = joined.get(&speaker) {
            (speaker, changed) = (kept, when);
        }
        if changed > *at {
            churn.changed += seconds;
            churn.wrong += seconds * (changed - at);
            churn.longest = churn.longest.max(changed - at);
        }
    }
    (recording.finish(), churn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(start: f64, end: f64, speaker: &str) -> Segment {
        Segment {
            start,
            end,
            speaker: speaker.into(),
        }
    }

    fn turn(start: f64, end: f64, speaker: usize) -> Turn {
        Turn {
            start,
            end,
            speaker,
        }
    }

    fn hypothesis(turns: &[Turn]) -> Vec<Segment> {
        let turns = turns.iter();
        turns
            .map(|t| said(t.start, t.end, &t.speaker.to_string()))
            .collect()
    }

    /// Reference: A 0-10, B 10-20. Found: 0 holds A and 2 s of B, 1 the rest of
    /// B, and 2 is a short extra of A.
    fn split_and_mixed() -> (Vec<Segment>, Vec<Turn>) {
        let reference = vec![said(0.0, 10.0, "A"), said(10.0, 20.0, "B")];
        let turns = vec![
            turn(0.0, 8.0, 0),
            turn(8.0, 10.0, 2),
            turn(10.0, 12.0, 0),
            turn(12.0, 20.0, 1),
        ];
        (reference, turns)
    }

    #[test]
    fn overlap_counts_shared_seconds_within_the_scored_regions() {
        let (reference, turns) = split_and_mixed();
        let all = overlap(&reference, &hypothesis(&turns), None);
        assert_eq!(all.reference, ["A", "B"].map(String::from));
        assert_eq!(all.found, ["0", "1", "2"].map(String::from));
        assert_eq!(all.seconds, [[8.0, 0.0, 2.0], [2.0, 8.0, 0.0]]);
        let part = overlap(&reference, &hypothesis(&turns), Some(&[(6.0, 14.0)]));
        assert_eq!(part.seconds, [[2.0, 0.0, 2.0], [2.0, 2.0, 0.0]]);
    }

    #[test]
    fn purity_and_coverage_ignore_what_only_one_side_holds() {
        let (reference, turns) = split_and_mixed();
        let agreement = Agreement::of(&overlap(&reference, &hypothesis(&turns), None));
        // Found 0 is 8 of 10 A, 1 is all B, 2 is all A: 8 + 8 + 2 of 20.
        assert_eq!(agreement.purity(), 0.9);
        // A has 8 of 10 in found 0, B has 8 of 10 in found 1.
        assert_eq!(agreement.coverage(), 0.8);
        let longer = vec![said(0.0, 30.0, "A"), said(10.0, 20.0, "B")];
        let extra = hypothesis(&[turn(0.0, 20.0, 0), turn(25.0, 30.0, 1)]);
        let both = Agreement::of(&overlap(&longer, &extra, None));
        assert_eq!(both.total, 35.0, "A at 20-25 is only in the reference");
        assert_eq!((both.pure, both.covered), (25.0, 30.0));
        let mut sum = agreement;
        sum.add(both);
        assert_eq!(sum.total, 55.0);
    }

    #[test]
    fn a_found_speaker_has_windows_seconds_span_and_a_main_reference_speaker() {
        let (reference, turns) = split_and_mixed();
        let overlap = overlap(&reference, &hypothesis(&turns), None);
        let centres = [1.0, 9.0, 11.0, 15.0, 19.0];
        let lives = lifetimes(&turns, &centres, &overlap);
        assert_eq!(
            lives[0],
            Lifetime {
                speaker: 0,
                windows: 2,
                seconds: 10.0,
                first: 0.0,
                last: 12.0,
                main: Some(("A".into(), 0.8)),
            }
        );
        assert_eq!(
            (lives[1].windows, lives[1].main.clone()),
            (2, Some(("B".into(), 1.0)))
        );
        assert_eq!((lives[2].windows, lives[2].seconds), (1, 2.0));
        // 2 s of 20 is 10%: none is under 5%.
        assert_eq!(small(&lives), 0);
        let lives = lifetimes(&[turn(0.0, 39.0, 0), turn(39.0, 40.0, 5)], &[], &overlap);
        assert_eq!(small(&lives), 1);
        assert_eq!(lives[1].main, None, "no reference speech to share");
    }

    fn voice(x: f32, y: f32) -> Vec<f32> {
        crate::domain::diarization::normalized(vec![x, y])
    }

    #[test]
    fn a_margin_is_the_gap_between_the_two_nearest_voices() {
        let windows = vec![(0.0, 1.0, voice(1.0, 0.0)), (1.0, 2.0, voice(1.0, 1.0))];
        let regions = [Region::new(0.0, 2.0, windows)];
        let voices = [voice(1.0, 0.0), voice(0.0, 1.0), voice(-1.0, 0.0)];
        let found = margins(&regions, &voices);
        // First: nearest 1, second 0 (orthogonal). Second: both 0.707.
        assert!((found[0] - 1.0).abs() < 1e-6);
        assert!(found[1].abs() < 1e-6);
        assert!(margins(&regions, &voices[..1]).is_empty());
    }

    #[test]
    fn a_spread_counts_windows_under_each_limit_and_gives_percentiles() {
        assert_eq!(spread(&[]), None);
        let margins: Vec<f32> = (0..100).map(|n| n as f32 / 100.0).collect();
        let spread = spread(&margins).unwrap();
        assert_eq!(spread.windows, 100);
        assert_eq!(spread.below, [0.05, 0.1, 0.2]);
        assert_eq!(spread.percentiles, [0.09, 0.49, 0.89]);
    }

    /// One-window regions, a second apart, each of the given voices.
    fn heard(voices: &[(f32, f32)]) -> Vec<Region> {
        voices
            .iter()
            .enumerate()
            .map(|(n, &(x, y))| {
                let at = n as f64;
                Region::new(at, at + 1.0, vec![(at, at + 1.0, voice(x, y))])
            })
            .collect()
    }

    #[test]
    fn nothing_changes_when_every_speaker_stays_who_they_were() {
        let regions = heard(&[(1.0, 0.0), (0.0, 1.0), (1.0, 0.0)]);
        let (found, churn) = replay(&regions, Clustering::default());
        assert_eq!(found.turns.len(), 3);
        assert_eq!(
            churn,
            Churn {
                speech: 3.0,
                ..Churn::default()
            }
        );
        assert_eq!(churn.stayed(), 0.0);
    }

    #[test]
    fn speech_told_to_a_speaker_who_then_joins_another_is_churn() {
        // At 0 and 0 degrees one speaker; 60 degrees starts another (distance
        // 0.5 against the threshold 0.3); two windows at 40 degrees go to the
        // second and pull its voice within the merge distance of the first.
        // The first has fewer windows, so it is the one that goes.
        let clustering = Clustering {
            threshold: 0.3,
            merge: 0.34,
            min_share: 0.0,
            settle: 0,
        };
        let at = |degrees: f32| (degrees.to_radians().cos(), degrees.to_radians().sin());
        let regions = heard(&[at(0.0), at(0.0), at(60.0), at(40.0), at(40.0)]);
        let (found, churn) = replay(&regions, clustering);
        assert_eq!(found.turns, [turn(0.0, 5.0, 0)]);
        // The first two seconds were told to a speaker gone at the end of the
        // fifth: 4 s and 3 s of recording later.
        assert_eq!(
            churn,
            Churn {
                speech: 5.0,
                changed: 2.0,
                wrong: 7.0,
                longest: 4.0
            }
        );
        assert_eq!(churn.stayed(), 3.5);
        let mut sum = churn;
        sum.add(churn);
        assert_eq!((sum.speech, sum.longest), (10.0, 4.0));
    }
}
