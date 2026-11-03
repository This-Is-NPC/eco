//! Average-linkage agglomerative clustering on cosine distance: what the
//! diarizer re-runs over the speech of a session, and what a recording is
//! compared against.

use std::collections::BTreeMap;

use rayon::prelude::*;

use crate::domain::diarization::{dot, normalized};

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

/// A label per unit-length embedding, each standing for `weights` observations:
/// nearest-neighbour-chain average linkage, stopped at `threshold`, then groups
/// under `min_share` of all observations folded into the large group whose
/// centroid is closest.
pub fn cluster(
    embeddings: &[&[f32]],
    weights: &[usize],
    threshold: f32,
    min_share: f32,
) -> Vec<usize> {
    let n = embeddings.len();
    if n < 2 {
        return vec![0; n];
    }
    let mut distances = Condensed {
        n,
        values: vec![0.0; n * (n - 1) / 2],
    };
    // Row i holds the distances to the items after it, one row after another.
    let mut rows: Vec<&mut [f32]> = Vec::with_capacity(n - 1);
    let mut rest = distances.values.as_mut_slice();
    for i in 0..n - 1 {
        let (row, tail) = rest.split_at_mut(n - 1 - i);
        rows.push(row);
        rest = tail;
    }
    rows.into_par_iter().enumerate().for_each(|(i, row)| {
        for (distance, other) in row.iter_mut().zip(&embeddings[i + 1..]) {
            *distance = 1.0 - dot(embeddings[i], other);
        }
    });
    // Each active group is represented by one of its members.
    let mut size: Vec<f32> = weights.iter().map(|&w| w as f32).collect();
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
            let merged = (size[kept] * distances.get(kept, other)
                + size[gone] * distances.get(gone, other))
                / (size[kept] + size[gone]);
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
    fold_small(embeddings, weights, (0..n).map(root).collect(), min_share)
}

/// Move the members of groups lighter than `min_share` of all observations to
/// the large group whose centroid is closest; labels come out as 0, 1, 2…
fn fold_small(
    embeddings: &[&[f32]],
    weights: &[usize],
    groups: Vec<usize>,
    min_share: f32,
) -> Vec<usize> {
    let n = embeddings.len();
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, &group) in groups.iter().enumerate() {
        members.entry(group).or_default().push(index);
    }
    let total: usize = weights.iter().sum();
    let minimum = (min_share * total as f32).ceil() as usize;
    let large: Vec<(usize, Vec<f32>)> = members
        .iter()
        .filter(|(_, m)| m.iter().map(|&i| weights[i]).sum::<usize>() >= minimum)
        .map(|(&group, m)| {
            let mut centroid = vec![0f32; embeddings[0].len()];
            for &index in m {
                let weight = weights[index] as f32;
                centroid
                    .iter_mut()
                    .zip(embeddings[index])
                    .for_each(|(c, x)| *c += weight * x);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(x: f32, y: f32) -> Vec<f32> {
        normalized(vec![x, y])
    }

    #[test]
    fn items_join_by_weighted_average_distance_and_stop_at_the_threshold() {
        let voices = [voice(1.0, 0.0), voice(0.0, 1.0), voice(1.0, 0.3)];
        let items: Vec<&[f32]> = voices.iter().map(Vec::as_slice).collect();
        assert_eq!(cluster(&items, &[1, 1, 1], 0.2, 0.0), [0, 1, 0]);
        assert_eq!(cluster(&items, &[1, 1, 1], 1.5, 0.0), [0, 0, 0]);
        // B joins A first; whether C then joins both depends on B's weight.
        let line = [voice(1.0, 0.0), voice(1.0, 0.8), voice(0.0, 1.0)];
        let items: Vec<&[f32]> = line.iter().map(Vec::as_slice).collect();
        assert_eq!(cluster(&items, &[1, 1, 1], 0.5, 0.0), [0, 0, 1]);
        assert_eq!(cluster(&items, &[1, 9, 1], 0.5, 0.0), [0, 0, 0]);
    }

    #[test]
    fn a_light_group_folds_into_the_nearest_heavy_one_by_weight_not_count() {
        let voices = [voice(1.0, 0.0), voice(0.0, 1.0), voice(1.0, 0.6)];
        let items: Vec<&[f32]> = voices.iter().map(Vec::as_slice).collect();
        // One item of weight 1 in 21 is 5%: under 10%, so it folds; the
        // second item is one item too but weighs 10 and stays.
        assert_eq!(cluster(&items, &[10, 10, 1], 0.1, 0.1), [0, 1, 0]);
        assert_eq!(cluster(&items, &[10, 10, 1], 0.1, 0.0), [0, 1, 2]);
        assert_eq!(cluster(&[], &[], 0.5, 0.0), Vec::<usize>::new());
        assert_eq!(cluster(&items[..1], &[1], 0.5, 0.0), [0]);
        // No group is heavy enough: all are one.
        assert_eq!(cluster(&items, &[10, 10, 1], 0.1, 1.0), [0, 0, 0]);
    }
}
