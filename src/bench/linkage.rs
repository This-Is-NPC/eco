//! The clustering eco ran when a recording ended, kept to compare with the
//! live one: average-linkage clustering of every window, then small groups
//! folded into the nearest larger one.

use crate::domain::diarization::{Region, Turn};
use crate::domain::linkage::cluster;

/// Who spoke when in `regions` once all were heard: every window clustered
/// together; a region too short to embed takes the speaker of the window
/// nearest it in time. Speakers are numbered in the order they first speak.
pub fn diarize(regions: &[Region], threshold: f32, min_share: f32) -> Vec<Turn> {
    let windows: Vec<(f64, f64, &[f32])> = regions.iter().flat_map(Region::windows).collect();
    let embeddings: Vec<&[f32]> = windows.iter().map(|w| w.2).collect();
    let weights = vec![1; embeddings.len()];
    let labels = cluster(&embeddings, &weights, threshold, min_share);
    let labelled: Vec<(f64, usize)> = windows
        .iter()
        .zip(&labels)
        .map(|(w, &label)| ((w.0 + w.1) / 2.0, label))
        .collect();
    let mut spans: Vec<(f64, f64, Option<usize>)> = windows
        .iter()
        .zip(&labels)
        .map(|(w, &label)| (w.0, w.1, Some(label)))
        .chain(
            regions
                .iter()
                .filter(|r| r.windows().next().is_none())
                .map(|r| (r.span().0, r.span().1, None)),
        )
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut order: Vec<usize> = Vec::new();
    let mut turns: Vec<Turn> = Vec::new();
    for (start, end, label) in spans {
        let middle = (start + end) / 2.0;
        let nearest = labelled
            .iter()
            .min_by(|a, b| (a.0 - middle).abs().total_cmp(&(b.0 - middle).abs()))
            .map(|&(_, label)| label);
        let Some(label) = label.or(nearest) else {
            continue;
        };
        let speaker = order.iter().position(|&l| l == label).unwrap_or_else(|| {
            order.push(label);
            order.len() - 1
        });
        match turns.last_mut() {
            Some(last) if last.speaker == speaker && start - last.end < 1e-6 => {
                last.end = last.end.max(end);
            }
            _ => turns.push(Turn {
                start,
                end,
                speaker,
            }),
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diarization::normalized;

    fn voice(x: f32, y: f32) -> Vec<f32> {
        normalized(vec![x, y])
    }

    /// One region of one-second windows, one per voice.
    fn region(start: f64, voices: &[(f32, f32)]) -> Region {
        let windows = voices.iter().enumerate().map(|(n, &(x, y))| {
            let at = start + n as f64;
            (at, at + 1.0, voice(x, y))
        });
        Region::new(start, start + voices.len() as f64, windows.collect())
    }

    #[test]
    fn windows_join_by_average_distance_and_stop_at_the_threshold() {
        let regions = [
            region(0.0, &[(1.0, 0.0), (1.0, 0.1)]),
            region(2.0, &[(0.0, 1.0), (0.1, 1.0)]),
            region(4.0, &[(1.0, 0.05)]),
        ];
        let turns = diarize(&regions, 0.5, 0.0);
        let spans: Vec<(f64, f64, usize)> =
            turns.iter().map(|t| (t.start, t.end, t.speaker)).collect();
        assert_eq!(spans, [(0.0, 2.0, 0), (2.0, 4.0, 1), (4.0, 5.0, 0)]);
        assert_eq!(
            diarize(&regions, 1.5, 0.0).len(),
            1,
            "one group past the threshold"
        );
    }

    #[test]
    fn a_small_group_folds_into_the_nearest_large_one_and_a_short_region_follows_its_neighbour() {
        let regions = [
            region(0.0, &[(1.0, 0.0); 4]),
            region(4.0, &[(0.0, 1.0); 4]),
            Region::new(8.0, 8.5, Vec::new()),
            region(9.0, &[(1.0, 0.6)]),
        ];
        let turns = diarize(&regions, 0.1, 0.2);
        let spans: Vec<(f64, f64, usize)> =
            turns.iter().map(|t| (t.start, t.end, t.speaker)).collect();
        // The lone window is 1 of 9, under 20%: it joins the nearest large group.
        assert_eq!(spans, [(0.0, 4.0, 0), (4.0, 8.5, 1), (9.0, 10.0, 0)]);
    }
}
