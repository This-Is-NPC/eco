//! Diarization error rate as pyansession.metrics computes it: scored within the UEM
//! less a collar around every reference boundary (half before, half after),
//! overlapping speech included, speakers mapped one to one to maximise overlap.

use std::collections::BTreeSet;

/// Who speaks over `[start, end)`, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub speaker: String,
}

/// `SPEAKER <file> <channel> <start> <duration> <NA> <NA> <speaker> …` lines.
pub fn parse_rttm(text: &str) -> Result<Vec<Segment>, String> {
    text.lines()
        .filter(|line| line.starts_with("SPEAKER"))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let number = |i: usize| -> Result<f64, String> {
                let field = fields.get(i).ok_or(format!("short line: {line}"))?;
                field.parse().map_err(|_| format!("not a number: {line}"))
            };
            let (start, duration) = (number(3)?, number(4)?);
            let speaker = fields.get(7).ok_or(format!("no speaker: {line}"))?;
            Ok(Segment {
                start,
                end: start + duration,
                speaker: (*speaker).into(),
            })
        })
        .collect()
}

/// `<file> <channel> <start> <end>` lines: the stretches that are scored.
pub fn parse_uem(text: &str) -> Result<Vec<(f64, f64)>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match (
                fields.get(2).map(|f| f.parse()),
                fields.get(3).map(|f| f.parse()),
            ) {
                (Some(Ok(start)), Some(Ok(end))) => Ok((start, end)),
                _ => Err(format!("bad UEM line: {line}")),
            }
        })
        .collect()
}

/// Seconds of each kind of error, and of reference speech.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Score {
    pub total: f64,
    pub missed: f64,
    pub false_alarm: f64,
    pub confusion: f64,
}

impl Score {
    pub fn der(&self) -> f64 {
        (self.missed + self.false_alarm + self.confusion) / self.total
    }

    pub fn add(&mut self, other: Score) {
        self.total += other.total;
        self.missed += other.missed;
        self.false_alarm += other.false_alarm;
        self.confusion += other.confusion;
    }
}

/// `regions` less every `[t - collar / 2, t + collar / 2]` around a boundary.
fn scored(regions: &[(f64, f64)], reference: &[Segment], collar: f64) -> Vec<(f64, f64)> {
    let mut cuts: Vec<(f64, f64)> = reference
        .iter()
        .flat_map(|s| [s.start, s.end])
        .map(|t| (t - collar / 2.0, t + collar / 2.0))
        .filter(|_| collar > 0.0)
        .collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut kept = Vec::new();
    for &(start, end) in regions {
        let mut from = start;
        for &(cut_start, cut_end) in &cuts {
            if cut_end <= from || cut_start >= end {
                continue;
            }
            if cut_start > from {
                kept.push((from, cut_start));
            }
            from = from.max(cut_end);
        }
        if from < end {
            kept.push((from, end));
        }
    }
    kept
}

/// `segments` cut to `regions`.
fn crop(segments: &[Segment], regions: &[(f64, f64)]) -> Vec<Segment> {
    let mut cropped = Vec::new();
    for segment in segments {
        for &(start, end) in regions {
            let (from, to) = (segment.start.max(start), segment.end.min(end));
            if from < to {
                cropped.push(Segment {
                    start: from,
                    end: to,
                    speaker: segment.speaker.clone(),
                });
            }
        }
    }
    cropped
}

fn labels(segments: &[Segment]) -> Vec<String> {
    let set: BTreeSet<&str> = segments.iter().map(|s| s.speaker.as_str()).collect();
    set.into_iter().map(String::from).collect()
}

/// The column assigned to each row that maximises the total of `gain`
/// (Hungarian method on a square matrix).
fn best_assignment(gain: &[Vec<f64>]) -> Vec<usize> {
    let n = gain.len();
    let cost = |i: usize, j: usize| -gain[i - 1][j - 1];
    let (mut u, mut v) = (vec![0.0; n + 1], vec![0.0; n + 1]);
    let (mut row_of, mut way) = (vec![0usize; n + 1], vec![0usize; n + 1]);
    for i in 1..=n {
        row_of[0] = i;
        let mut j0 = 0;
        let mut lowest = vec![f64::INFINITY; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = row_of[j0];
            let (mut delta, mut j1) = (f64::INFINITY, 0);
            for j in 1..=n {
                if !used[j] {
                    let reduced = cost(i0, j) - u[i0] - v[j];
                    if reduced < lowest[j] {
                        lowest[j] = reduced;
                        way[j] = j0;
                    }
                    if lowest[j] < delta {
                        delta = lowest[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[row_of[j]] += delta;
                    v[j] -= delta;
                } else {
                    lowest[j] -= delta;
                }
            }
            j0 = j1;
            if row_of[j0] == 0 {
                break;
            }
        }
        while j0 != 0 {
            let j1 = way[j0];
            row_of[j0] = row_of[j1];
            j0 = j1;
        }
    }
    let mut column = vec![0; n];
    for j in 1..=n {
        if row_of[j] > 0 {
            column[row_of[j] - 1] = j - 1;
        }
    }
    column
}

/// Score `hypothesis` against `reference` within `uem` (the whole reference when
/// `None`), forgiving `collar` seconds around each reference boundary.
pub fn score(
    reference: &[Segment],
    hypothesis: &[Segment],
    uem: Option<&[(f64, f64)]>,
    collar: f64,
) -> Score {
    let extent = || {
        let start = reference
            .iter()
            .map(|s| s.start)
            .fold(f64::INFINITY, f64::min);
        let end = reference.iter().map(|s| s.end).fold(0.0, f64::max);
        vec![(start.min(end), end)]
    };
    let regions = scored(&uem.map_or_else(extent, <[_]>::to_vec), reference, collar);
    let (reference, hypothesis) = (crop(reference, &regions), crop(hypothesis, &regions));
    let (said, guessed) = (labels(&reference), labels(&hypothesis));
    let size = said.len().max(guessed.len());
    let mut together = vec![vec![0.0; size]; size];
    for r in &reference {
        let row = said.iter().position(|l| *l == r.speaker).expect("listed");
        for h in &hypothesis {
            let overlap = r.end.min(h.end) - r.start.max(h.start);
            if overlap > 0.0 {
                let column = guessed
                    .iter()
                    .position(|l| *l == h.speaker)
                    .expect("listed");
                together[row][column] += overlap;
            }
        }
    }
    let assignment = best_assignment(&together);
    // The reference speaker each hypothesis speaker stands for, if any.
    let mut stands_for: Vec<Option<usize>> = vec![None; guessed.len()];
    for (row, &column) in assignment.iter().enumerate() {
        if row < said.len() && column < guessed.len() && together[row][column] > 0.0 {
            stands_for[column] = Some(row);
        }
    }
    let mut bounds: Vec<f64> = reference
        .iter()
        .chain(&hypothesis)
        .flat_map(|s| [s.start, s.end])
        .collect();
    bounds.sort_by(f64::total_cmp);
    bounds.dedup();
    let mut total = Score::default();
    for pair in bounds.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let middle = (from + to) / 2.0;
        let active = |segments: &[Segment], names: &[String]| -> BTreeSet<usize> {
            segments
                .iter()
                .filter(|s| s.start <= middle && middle < s.end)
                .map(|s| names.iter().position(|l| *l == s.speaker).expect("listed"))
                .collect()
        };
        let (r, h) = (active(&reference, &said), active(&hypothesis, &guessed));
        let correct = h
            .iter()
            .filter(|&&g| stands_for[g].is_some_and(|row| r.contains(&row)))
            .count();
        let duration = to - from;
        total.total += duration * r.len() as f64;
        total.missed += duration * r.len().saturating_sub(h.len()) as f64;
        total.false_alarm += duration * h.len().saturating_sub(r.len()) as f64;
        total.confusion += duration * (r.len().min(h.len()) - correct) as f64;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(list: &[(f64, f64, &str)]) -> Vec<Segment> {
        list.iter()
            .map(|&(start, end, speaker)| Segment {
                start,
                end,
                speaker: speaker.into(),
            })
            .collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn reads_rttm_and_uem() {
        let rttm = "SPEAKER ES2004a 1 0.37 1.39 <NA> <NA> MEO015 <NA> <NA>\n";
        assert_eq!(
            parse_rttm(rttm).unwrap(),
            segments(&[(0.37, 0.37 + 1.39, "MEO015")])
        );
        assert_eq!(
            parse_uem("ES2004a 1 0.000 1049.354687\n").unwrap(),
            [(0.0, 1049.354687)]
        );
        assert!(parse_rttm("SPEAKER x 1 a 1 <NA> <NA> s").is_err());
    }

    #[test]
    fn labels_do_not_matter_only_the_mapping() {
        let reference = segments(&[(0.0, 10.0, "A"), (10.0, 20.0, "B")]);
        let hypothesis = segments(&[(0.0, 10.0, "spk1"), (10.0, 20.0, "spk0")]);
        let score = score(&reference, &hypothesis, None, 0.0);
        assert_eq!(score.der(), 0.0);
    }

    /// The components pyansession.metrics 4.1 gives for this pair
    /// (`DiarizationErrorRate(collar=…)` with the UEM 0–30 s).
    #[test]
    fn agrees_with_pyansession_metrics() {
        let reference = segments(&[
            (0.0, 10.0, "A"),
            (8.0, 15.0, "B"),
            (16.0, 25.0, "A"),
            (26.0, 29.0, "C"),
        ]);
        let hypothesis = segments(&[
            (0.5, 9.0, "x"),
            (9.0, 16.5, "y"),
            (17.0, 22.0, "x"),
            (22.0, 25.5, "y"),
            (27.0, 30.0, "z"),
        ]);
        let uem = [(0.0, 30.0)];
        let plain = score(&reference, &hypothesis, Some(&uem), 0.0);
        let expected = (29.0, 4.0, 2.5, 3.5);
        let got = (
            plain.total,
            plain.missed,
            plain.false_alarm,
            plain.confusion,
        );
        assert!(
            close(got.0, expected.0) && close(got.1, expected.1),
            "{plain:?}"
        );
        assert!(
            close(got.2, expected.2) && close(got.3, expected.3),
            "{plain:?}"
        );
        let forgiving = score(&reference, &hypothesis, Some(&uem), 0.5);
        let expected = (26.0, 3.0, 1.5, 3.0);
        let got = (
            forgiving.total,
            forgiving.missed,
            forgiving.false_alarm,
            forgiving.confusion,
        );
        assert!(
            close(got.0, expected.0) && close(got.1, expected.1),
            "{forgiving:?}"
        );
        assert!(
            close(got.2, expected.2) && close(got.3, expected.3),
            "{forgiving:?}"
        );
    }

    #[test]
    fn assignment_maximises_the_total() {
        let gain = vec![
            vec![1.0, 5.0, 0.0],
            vec![4.0, 6.0, 0.0],
            vec![0.0, 0.0, 0.0],
        ];
        assert_eq!(best_assignment(&gain), [1, 0, 2]);
    }
}
