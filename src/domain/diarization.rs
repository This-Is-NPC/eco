//! Who spoke when in one recording: each speech region is embedded in short
//! windows, and the windows are grouped by voice.

use std::collections::BTreeMap;

use rayon::prelude::*;

use crate::ports::{EmbeddingError, SAMPLE_RATE, SpeakerEmbedder};

const WINDOW: usize = SAMPLE_RATE as usize * 3 / 2;
const HOP: usize = WINDOW / 2;
/// Regions shorter than this carry too little voice to embed; they take the
/// speaker of the nearest turn.
const MIN_CLIP: usize = SAMPLE_RATE as usize / 2;

/// Stretches of the recording, in seconds, said by one speaker (numbered from 0
/// in the order they first speak).
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
}

/// Who spoke when, and each speaker's voice: the unit-length mean of their
/// windows, indexed like `Turn::speaker`.
#[derive(Debug, Clone, PartialEq)]
pub struct Diarization {
    pub turns: Vec<Turn>,
    pub voices: Vec<Vec<f32>>,
}

/// How the windows are grouped.
#[derive(Debug, Clone, Copy)]
pub struct Clustering {
    /// Two groups merge while the mean cosine distance between them is below this.
    pub threshold: f32,
    /// Groups holding less than this share of the windows join the nearest larger one.
    pub min_share: f32,
}

impl Default for Clustering {
    fn default() -> Self {
        Self {
            threshold: 0.65,
            min_share: 0.02,
        }
    }
}

/// The part of the recording a window speaks for, and its voice.
struct Window {
    start: f64,
    end: f64,
    embedding: Vec<f32>,
}

/// Collects speech regions as they arrive and tells the speakers apart at the end.
pub struct Diarizer<'a> {
    embedder: &'a dyn SpeakerEmbedder,
    windows: Vec<Window>,
    /// Regions too short to embed.
    short: Vec<(f64, f64)>,
}

fn seconds(samples: usize) -> f64 {
    samples as f64 / f64::from(SAMPLE_RATE)
}

/// Window starts in a region of `len` samples: every `HOP`, the last one flush
/// with the end.
fn window_starts(len: usize) -> Vec<usize> {
    if len <= WINDOW {
        return vec![0];
    }
    let mut starts: Vec<usize> = (0..=len - WINDOW).step_by(HOP).collect();
    if starts.last() != Some(&(len - WINDOW)) {
        starts.push(len - WINDOW);
    }
    starts
}

impl<'a> Diarizer<'a> {
    pub fn new(embedder: &'a dyn SpeakerEmbedder) -> Self {
        Self {
            embedder,
            windows: Vec::new(),
            short: Vec::new(),
        }
    }

    /// Add the speech region `pcm`, which starts `start` seconds into the recording.
    pub fn add(&mut self, start: f64, pcm: &[i16]) -> Result<(), EmbeddingError> {
        let end = start + seconds(pcm.len());
        if pcm.len() < MIN_CLIP {
            self.short.push((start, end));
            return Ok(());
        }
        let starts = window_starts(pcm.len());
        let embeddings: Vec<Vec<f32>> = starts
            .par_iter()
            .map(|&at| self.embedder.embed(&pcm[at..pcm.len().min(at + WINDOW)]))
            .collect::<Result<_, _>>()?;
        // Each window speaks for the stretch nearer its centre than its neighbours'.
        let centres: Vec<f64> = starts
            .iter()
            .map(|&at| start + seconds(at + WINDOW.min(pcm.len()) / 2))
            .collect();
        for (index, embedding) in embeddings.into_iter().enumerate() {
            let from = match index {
                0 => start,
                _ => (centres[index - 1] + centres[index]) / 2.0,
            };
            let to = centres
                .get(index + 1)
                .map_or(end, |next| (centres[index] + next) / 2.0);
            self.windows.push(Window {
                start: from,
                end: to,
                embedding: normalized(embedding),
            });
        }
        Ok(())
    }

    /// The turns of the whole recording, in time order, and the speakers' voices.
    pub fn finish(&self, clustering: Clustering) -> Diarization {
        let embeddings: Vec<&[f32]> = self.windows.iter().map(|w| &w.embedding[..]).collect();
        let labels = cluster(&embeddings, clustering);
        let mut spans: Vec<(f64, f64, Option<usize>)> = self
            .windows
            .iter()
            .zip(&labels)
            .map(|(w, &label)| (w.start, w.end, Some(label)))
            .chain(self.short.iter().map(|&(start, end)| (start, end, None)))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let labelled: Vec<(f64, usize)> = spans
            .iter()
            .filter_map(|s| s.2.map(|label| ((s.0 + s.1) / 2.0, label)))
            .collect();
        // Speakers are numbered in the order they first speak.
        let mut order: Vec<usize> = Vec::new();
        let mut turns: Vec<Turn> = Vec::new();
        for (start, end, label) in spans {
            let Some(label) = label.or_else(|| nearest(&labelled, (start + end) / 2.0)) else {
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
        let voices = order
            .iter()
            .map(|&label| {
                let mut sum = vec![0f32; self.windows[0].embedding.len()];
                let members = self
                    .windows
                    .iter()
                    .zip(&labels)
                    .filter(|(_, l)| **l == label);
                for (window, _) in members {
                    sum.iter_mut()
                        .zip(&window.embedding)
                        .for_each(|(s, x)| *s += x);
                }
                normalized(sum)
            })
            .collect();
        Diarization { turns, voices }
    }
}

/// The label of the labelled span whose middle is nearest `at`.
fn nearest(labelled: &[(f64, usize)], at: f64) -> Option<usize> {
    labelled
        .iter()
        .min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))
        .map(|&(_, label)| label)
}

/// The speaker who talks most within `[start, end)`, or the nearest one.
pub fn speaker_of(turns: &[Turn], start: f64, end: f64) -> Option<usize> {
    let mut talk: Vec<f64> = Vec::new();
    for turn in turns {
        let overlap = turn.end.min(end) - turn.start.max(start);
        if overlap > 0.0 {
            if talk.len() <= turn.speaker {
                talk.resize(turn.speaker + 1, 0.0);
            }
            talk[turn.speaker] += overlap;
        }
    }
    let most = (0..talk.len()).max_by(|&a, &b| talk[a].total_cmp(&talk[b]));
    most.filter(|&speaker| talk[speaker] > 0.0).or_else(|| {
        let middle = (start + end) / 2.0;
        let distance = |t: &Turn| (t.start - middle).abs().min((t.end - middle).abs());
        turns
            .iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .map(|t| t.speaker)
    })
}

/// Who says each line of a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Lines {
    /// A label per line: `Speaker N`, numbered as they first speak in the lines.
    pub labels: Vec<String>,
    /// For `Speaker N`, the diarized speaker at index N - 1.
    pub speakers: Vec<usize>,
}

impl Lines {
    /// The voice of each label, from the voices diarization found.
    pub fn voices(&self, voices: &[Vec<f32>]) -> BTreeMap<String, Vec<f32>> {
        let found = self.speakers.iter().enumerate();
        found
            .map(|(index, &speaker)| (label(index), voices[speaker].clone()))
            .collect()
    }
}

fn label(index: usize) -> String {
    format!("Speaker {}", index + 1)
}

/// The speaker of each line from its span; `None` when one voice says them all.
pub fn label_lines(turns: &[Turn], spans: &[(f64, f64)]) -> Option<Lines> {
    let mut order: Vec<usize> = Vec::new();
    let mut labels = Vec::with_capacity(spans.len());
    for &(start, end) in spans {
        let speaker = speaker_of(turns, start, end)?;
        let number = order.iter().position(|&s| s == speaker).unwrap_or_else(|| {
            order.push(speaker);
            order.len() - 1
        });
        labels.push(label(number));
    }
    (order.len() > 1).then_some(Lines {
        labels,
        speakers: order,
    })
}

/// `vector` scaled to unit length (zero stays zero).
pub fn normalized(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        vector.iter_mut().for_each(|x| *x /= norm);
    }
    vector
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

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

/// A label per unit-length embedding: average-linkage agglomerative clustering
/// on cosine distance (nearest-neighbour chain), stopped at the threshold, then
/// small groups folded into the nearest larger one.
pub fn cluster(embeddings: &[&[f32]], clustering: Clustering) -> Vec<usize> {
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
        if best_distance >= clustering.threshold {
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
    let groups: Vec<usize> = (0..n).map(root).collect();
    fold_small(embeddings, groups, clustering.min_share)
}

/// Move the members of groups smaller than `min_share` of all embeddings to the
/// large group whose centroid is closest; labels come out as 0, 1, 2…
fn fold_small(embeddings: &[&[f32]], groups: Vec<usize>, min_share: f32) -> Vec<usize> {
    let n = embeddings.len();
    let dimension = embeddings[0].len();
    let mut members: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for (index, &group) in groups.iter().enumerate() {
        members.entry(group).or_default().push(index);
    }
    let minimum = (min_share * n as f32).ceil() as usize;
    let mut large: Vec<(usize, Vec<f32>)> = members
        .iter()
        .filter(|(_, m)| m.len() >= minimum)
        .map(|(&group, m)| {
            let mut centroid = vec![0f32; dimension];
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
    large.sort_by_key(|(group, _)| *group);
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

    /// A voice is the sign of the samples: positive clips are one speaker,
    /// negative another, silence a third.
    struct BySign;

    impl SpeakerEmbedder for BySign {
        fn embed(&self, pcm: &[i16]) -> Result<Vec<f32>, EmbeddingError> {
            let mean = pcm.iter().map(|&s| f64::from(s)).sum::<f64>() / pcm.len() as f64;
            Ok(match mean {
                m if m > 0.0 => vec![1.0, 0.1, 0.0],
                m if m < 0.0 => vec![0.0, 0.1, 1.0],
                _ => vec![0.0, 1.0, 0.0],
            })
        }
    }

    fn speech(value: i16, seconds: f64) -> Vec<i16> {
        vec![value; (seconds * f64::from(SAMPLE_RATE)) as usize]
    }

    #[test]
    fn windows_cover_a_region_and_end_flush() {
        assert_eq!(window_starts(WINDOW / 2), [0]);
        assert_eq!(window_starts(WINDOW), [0]);
        assert_eq!(window_starts(WINDOW + HOP), [0, HOP]);
        assert_eq!(window_starts(WINDOW + HOP + 10), [0, HOP, HOP + 10]);
    }

    #[test]
    fn regions_are_split_into_turns_by_voice() {
        let embedder = BySign;
        let mut diarizer = Diarizer::new(&embedder);
        diarizer.add(1.0, &speech(100, 4.0)).unwrap();
        diarizer.add(6.0, &speech(-100, 3.0)).unwrap();
        diarizer.add(10.0, &speech(100, 2.0)).unwrap();
        diarizer.add(12.5, &speech(-100, 0.2)).unwrap(); // too short: nearest turn
        let Diarization { turns, voices } = diarizer.finish(Clustering::default());
        assert_eq!(voices.len(), 2);
        assert!(dot(&voices[0], &[1.0, 0.1, 0.0]) > 0.99);
        assert!(dot(&voices[1], &[0.0, 0.1, 1.0]) > 0.99);
        let spans: Vec<(f64, f64, usize)> = turns
            .iter()
            .map(|t| {
                (
                    (t.start * 10.0).round() / 10.0,
                    (t.end * 10.0).round() / 10.0,
                    t.speaker,
                )
            })
            .collect();
        assert_eq!(
            spans,
            [
                (1.0, 5.0, 0),
                (6.0, 9.0, 1),
                (10.0, 12.0, 0),
                (12.5, 12.7, 0)
            ]
        );
        assert_eq!(speaker_of(&turns, 6.5, 8.0), Some(1));
        assert_eq!(speaker_of(&turns, 4.0, 6.5), Some(0));
        assert_eq!(speaker_of(&turns, 20.0, 21.0), Some(0));
        let labels = label_lines(&turns, &[(6.0, 9.0), (1.0, 3.0), (10.0, 11.0)]);
        let lines = labels.unwrap();
        assert_eq!(lines.labels, ["Speaker 1", "Speaker 2", "Speaker 2"]);
        assert_eq!(lines.speakers, [1, 0]);
        let voices = lines.voices(&[vec![1.0], vec![2.0]]);
        assert_eq!(voices["Speaker 1"], [2.0]);
        assert_eq!(voices["Speaker 2"], [1.0]);
        assert_eq!(label_lines(&turns, &[(1.0, 3.0), (10.0, 11.0)]), None);
        assert_eq!(label_lines(&[], &[(1.0, 3.0)]), None);
    }

    #[test]
    fn clustering_stops_at_the_threshold_and_folds_small_groups() {
        let a = [1.0, 0.0];
        let b = [0.0, 1.0];
        let near_a = normalized(vec![0.95, 0.1]);
        let embeddings: Vec<&[f32]> = vec![&a, &b, &near_a, &a, &b, &b];
        let strict = Clustering {
            threshold: 0.6,
            min_share: 0.0,
        };
        assert_eq!(cluster(&embeddings, strict), [0, 1, 0, 0, 1, 1]);
        let folding = Clustering {
            threshold: 0.001,
            min_share: 0.3,
        };
        assert_eq!(cluster(&embeddings, folding), [0, 1, 0, 0, 1, 1]);
        assert_eq!(cluster(&embeddings[..1], strict), [0]);
        let apart = Clustering {
            threshold: 0.001,
            min_share: 0.5,
        };
        let near_b = normalized(vec![0.1, 0.95]);
        let three: Vec<&[f32]> = vec![&a, &b, &near_b];
        assert_eq!(cluster(&three, apart), [0, 0, 0], "no group is large");
    }

    #[test]
    fn short_clips_alone_make_no_turns() {
        let embedder = BySign;
        let mut diarizer = Diarizer::new(&embedder);
        diarizer.add(1.0, &speech(100, 0.2)).unwrap();
        diarizer.add(2.0, &speech(-100, 0.3)).unwrap();
        let Diarization { turns, voices } = diarizer.finish(Clustering::default());
        assert!(turns.is_empty());
        assert!(voices.is_empty());
    }

    #[test]
    fn silence_long_enough_is_a_voice_of_its_own() {
        let embedder = BySign;
        let mut diarizer = Diarizer::new(&embedder);
        diarizer.add(0.0, &speech(100, 2.0)).unwrap();
        diarizer.add(3.0, &speech(0, 2.0)).unwrap();
        let Diarization { turns, voices } = diarizer.finish(Clustering::default());
        let speakers: Vec<usize> = turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, [0, 1]);
        assert_eq!(voices.len(), 2);
    }

    #[test]
    fn a_zero_vector_stays_zero() {
        assert_eq!(normalized(vec![0.0, 0.0]), [0.0, 0.0]);
    }
}
