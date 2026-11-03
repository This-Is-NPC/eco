//! The clustering eco ran when a recording ended, kept to compare with the
//! online one: average-linkage agglomerative clustering of every window on
//! cosine distance, then small groups folded into the nearest larger one.

use std::collections::BTreeMap;

use crate::domain::diarization::{Region, Turn, dot, normalized};

/// The upper triangle of a symmetric matrix of distances, without the diagonal.
struct Condensed {
    n: usize,
    values: Vec<f32>,
}

impl Condensed {
    fn index(&self, i: usize, j: usize) -> usize {
        let (i, j) = if i < j { (i, j) } else { (j, i) };
        i * self.n - i * (i + 1) / 2 + (j - i - 1)
    }

    fn get(&self, i: usize, j: usize) -> f32 {
        self.values[self.index(i, j)]
    }

    fn set(&mut self, i: usize, j: usize, value: f32) {
        let index = self.index(i, j);
        self.values[index] = value;
    }
}

/// A label per unit-length embedding: nearest-neighbour-chain average linkage,
/// stopped at `threshold`, then groups under `min_share` of the embeddings
/// folded into the large group whose centroid is closest.
fn cluster(embeddings: &[&[f32]], threshold: f32, min_share: f32) -> Vec<usize> {
    let n = embeddings.len();
    if n < 2 {
        return vec![0; n];
    }
    let mut distances = Condensed {
        n,
        values: vec![0.0; n * (n - 1) / 2],
    };
    for i in 0..n {
        for j in i + 1..n {
            distances.set(i, j, 1.0 - dot(embeddings[i], embeddings[j]));
        }
    }
    // Each active group is represented by one of its members.
    let mut size = vec![1usize; n];
    let mut active = vec![true; n];
    let mut parent: Vec<usize> = (0..n).collect();
    let mut chain: Vec<usize> = Vec::new();
    let mut remaining = n;
    while remaining > 1 {
        if chain.is_empty() {
            chain.push((0..n).find(|&i| active[i]).expect("two groups remain"));
        }
        let top = *chain.last().expect("not empty");
        let previous = chain.len().checked_sub(2).map(|i| chain[i]);
        let mut best = previous.unwrap_or(usize::MAX);
        let mut best_distance = previous.map_or(f32::INFINITY, |p| distances.get(top, p));
        for other in (0..n).filter(|&o| active[o] && o != top) {
            let distance = distances.get(top, other);
            if distance < best_distance {
                (best, best_distance) = (other, distance);
            }
        }
        if Some(best) != previous {
            chain.push(best);
            continue;
        }
        chain.truncate(chain.len() - 2);
        if best_distance >= threshold {
            // Reducible linkage: the closest pair left is past the threshold
            // only if every pair on the chain is; keep looking elsewhere.
            active[top] = false;
            active[best] = false;
            remaining -= 2;
            continue;
        }
        let (kept, gone) = (top.min(best), top.max(best));
        for other in (0..n).filter(|&o| active[o] && o != kept && o != gone) {
            let merged = (size[kept] as f32 * distances.get(kept, other)
                + size[gone] as f32 * distances.get(gone, other))
                / (size[kept] + size[gone]) as f32;
            distances.set(kept, other, merged);
        }
        size[kept] += size[gone];
        active[gone] = false;
        parent[gone] = kept;
        remaining -= 1;
    }
    let root = |mut i: usize| {
        while parent[i] != i {
            i = parent[i];
        }
        i
    };
    fold_small(embeddings, (0..n).map(root).collect(), min_share)
}

/// Move the members of groups smaller than `min_share` of all embeddings to the
/// large group whose centroid is closest; labels come out as 0, 1, 2…
fn fold_small(embeddings: &[&[f32]], groups: Vec<usize>, min_share: f32) -> Vec<usize> {
    let n = embeddings.len();
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, &group) in groups.iter().enumerate() {
        members.entry(group).or_default().push(index);
    }
    let minimum = (min_share * n as f32).ceil() as usize;
    let large: Vec<(usize, Vec<f32>)> = members
        .iter()
        .filter(|(_, m)| m.len() >= minimum)
        .map(|(&group, m)| {
            let mut centroid = vec![0f32; embeddings[0].len()];
            for &index in m {
                centroid
                    .iter_mut()
                    .zip(embeddings[index])
                    .for_each(|(c, x)| *c += x);
            }
            (group, normalized(centroid))
        })
        .collect();
    if large.is_empty() {
        return vec![0; n];
    }
    groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            large
                .iter()
                .position(|(g, _)| g == group)
                .unwrap_or_else(|| {
                    (0..large.len())
                        .max_by(|&a, &b| {
                            dot(&large[a].1, embeddings[index])
                                .total_cmp(&dot(&large[b].1, embeddings[index]))
                        })
                        .expect("one large group")
                })
        })
        .collect()
}

/// Who spoke when in `regions` once all were heard: every window clustered
/// together; a region too short to embed takes the speaker of the window
/// nearest it in time. Speakers are numbered in the order they first speak.
pub fn diarize(regions: &[Region], threshold: f32, min_share: f32) -> Vec<Turn> {
    let windows: Vec<(f64, f64, &[f32])> = regions.iter().flat_map(Region::windows).collect();
    let embeddings: Vec<&[f32]> = windows.iter().map(|w| w.2).collect();
    let labels = cluster(&embeddings, threshold, min_share);
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
