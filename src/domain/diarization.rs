//! Who spoke when: each speech region is embedded in short windows, and each
//! window goes to the speaker whose voice is nearest, or starts a new one, as
//! the recording is heard. The runs of windows one speaker said are kept, as
//! stretches of the last four hours, and every few minutes of speech and when
//! the recording ends they are clustered again as a whole: what the first
//! minutes got wrong is moved to the speaker it belongs to.

use std::collections::{BTreeMap, BTreeSet};

use rayon::prelude::*;

use crate::domain::linkage::cluster;
use crate::ports::{EmbeddingError, SAMPLE_RATE, SpeakerEmbedder, Word};

const WINDOW: usize = SAMPLE_RATE as usize * 3 / 2;
const HOP: usize = WINDOW / 2;
/// Regions shorter than this carry too little voice to embed; they take the
/// speaker heard last.
const MIN_CLIP: usize = SAMPLE_RATE as usize / 2;
/// At most this many speakers at once: past it, a window joins the nearest.
const MAX_SPEAKERS: usize = 32;
/// How many windows must be heard since a small speaker first spoke before
/// they are folded into a larger one (about 90 s of speech).
const SETTLE: usize = 120;
/// Seconds of turns a live input follows closely behind what the diarizer has heard.
const RECENT: f64 = 300.0;
/// Seconds behind what was heard that stretches, turns and lines are kept for
/// a re-cluster to move; older stretches stay with the voice they were heard in.
const REACH: f64 = 4.0 * 3600.0;
/// A span shorter than this, in seconds, is rounding.
const EPSILON: f64 = 1e-6;
/// Lines of a live input that wait, at most, for the diarizer to reach them.
const WAITING: usize = 256;

/// Stretches of the recording, in seconds, said by one speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
}

/// Who spoke when in a whole recording, speakers numbered from 0 in the order
/// they first speak, and each speaker's voice (unit length), indexed like
/// `Turn::speaker`.
#[derive(Debug, Clone, PartialEq)]
pub struct Diarization {
    pub turns: Vec<Turn>,
    pub voices: Vec<Vec<f32>>,
}

/// How windows are told apart, in cosine distance.
#[derive(Debug, Clone, Copy)]
pub struct Clustering {
    /// A window at least this far from every speaker's voice starts a new speaker.
    pub threshold: f32,
    /// Two speakers whose voices come closer than this are one.
    pub merge: f32,
    /// A settled speaker holding less than this share of the windows joins the
    /// nearest larger one.
    pub min_share: f32,
    /// A small speaker is folded into a larger one only once this many windows
    /// were heard since they first spoke.
    pub settle: usize,
    /// The speech heard is clustered again, as a whole, when two groups are at
    /// least this far apart on average.
    pub recluster: f32,
    /// Windows of speech heard between two re-clusters.
    pub every: usize,
}

impl Default for Clustering {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            merge: 0.3,
            min_share: 0.02,
            settle: SETTLE,
            recluster: 0.5,
            every: 400,
        }
    }
}

/// The part of the recording a window speaks for, and its voice (unit length).
struct Window {
    start: f64,
    end: f64,
    embedding: Vec<f32>,
}

/// A speech region: where it is in the recording, in seconds, and its windows
/// (none when it is too short to embed).
pub struct Region {
    start: f64,
    end: f64,
    windows: Vec<Window>,
}

impl Region {
    #[cfg(test)]
    pub fn new(start: f64, end: f64, windows: Vec<(f64, f64, Vec<f32>)>) -> Self {
        let windows = windows
            .into_iter()
            .map(|(start, end, embedding)| Window {
                start,
                end,
                embedding,
            })
            .collect();
        Self {
            start,
            end,
            windows,
        }
    }

    pub fn span(&self) -> (f64, f64) {
        (self.start, self.end)
    }

    /// Each window's span in seconds and its embedding (unit length).
    pub fn windows(&self) -> impl Iterator<Item = (f64, f64, &[f32])> {
        self.windows
            .iter()
            .map(|w| (w.start, w.end, &w.embedding[..]))
    }
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

/// Embed the speech region `pcm`, which starts `start` seconds into the recording.
pub fn region(
    embedder: &dyn SpeakerEmbedder,
    start: f64,
    pcm: &[i16],
) -> Result<Region, EmbeddingError> {
    let end = start + seconds(pcm.len());
    if pcm.len() < MIN_CLIP {
        return Ok(Region {
            start,
            end,
            windows: Vec::new(),
        });
    }
    let starts = window_starts(pcm.len());
    let embeddings: Vec<Vec<f32>> = starts
        .par_iter()
        .map(|&at| embedder.embed(&pcm[at..pcm.len().min(at + WINDOW)]))
        .collect::<Result<_, _>>()?;
    // Each window speaks for the stretch nearer its centre than its neighbours'.
    let centres: Vec<f64> = starts
        .iter()
        .map(|&at| start + seconds(at + WINDOW.min(pcm.len()) / 2))
        .collect();
    let windows = embeddings
        .into_iter()
        .enumerate()
        .map(|(index, embedding)| Window {
            start: match index {
                0 => start,
                _ => (centres[index - 1] + centres[index]) / 2.0,
            },
            end: centres
                .get(index + 1)
                .map_or(end, |next| (centres[index] + next) / 2.0),
            embedding: normalized(embedding),
        })
        .collect();
    Ok(Region {
        start,
        end,
        windows,
    })
}

/// A stretch of the recording, in seconds, that `from` was told to have said
/// and `to` said.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub start: f64,
    pub end: f64,
    pub from: usize,
    pub to: usize,
}

/// Give each span of `moved` to its new speaker in `turns`, which are in time
/// order, cutting the turns that cross a boundary.
pub fn move_spans(turns: &mut Vec<Turn>, moved: &[Moved]) {
    for span in moved {
        let first = turns.partition_point(|t| t.end <= span.start + EPSILON);
        let last = first + turns[first..].partition_point(|t| t.start < span.end - EPSILON);
        let mut pieces: Vec<Turn> = Vec::new();
        for turn in &turns[first..last] {
            if turn.speaker != span.from {
                pieces.push(turn.clone());
                continue;
            }
            let (start, end) = (turn.start.max(span.start), turn.end.min(span.end));
            let piece = |start, end, speaker| Turn {
                start,
                end,
                speaker,
            };
            if start - turn.start > EPSILON {
                pieces.push(piece(turn.start, start, turn.speaker));
            }
            pieces.push(piece(start, end, span.to));
            if turn.end - end > EPSILON {
                pieces.push(piece(end, turn.end, turn.speaker));
            }
        }
        turns.splice(first..last, pieces);
    }
    let mut joined: Vec<Turn> = Vec::with_capacity(turns.len());
    for turn in turns.drain(..) {
        match joined.last_mut() {
            Some(last) if last.speaker == turn.speaker && turn.start - last.end < EPSILON => {
                last.end = last.end.max(turn.end);
            }
            _ => joined.push(turn),
        }
    }
    *turns = joined;
}

/// What one region told: its turns, the voice of each speaker it changed, and
/// the speakers found to be one. A re-cluster tells no turns but the stretches
/// it moved from a speaker to another.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Update {
    /// Where the region ends, in seconds: the recording is heard up to here.
    pub end: f64,
    pub turns: Vec<Turn>,
    /// Unit length.
    pub voices: Vec<(usize, Vec<f32>)>,
    /// In order, as (gone, kept): `gone` is never told again.
    pub merged: Vec<(usize, usize)>,
    /// In order; a speaker named in `to` may be new.
    pub moved: Vec<Moved>,
}

/// A run of windows one speaker said in a region: the sum of their voices.
struct Stretch {
    id: u64,
    start: f64,
    end: f64,
    sum: Vec<f32>,
    windows: usize,
    speaker: usize,
}

/// The stretches to cluster again, copied so that a worker thread can do it
/// while the diarizer goes on hearing.
pub struct Recluster {
    ids: Vec<u64>,
    voices: Vec<Vec<f32>>,
    weights: Vec<usize>,
    threshold: f32,
    min_share: f32,
}

/// The group each stretch of a [`Recluster`] fell into.
pub struct Reclustered {
    ids: Vec<u64>,
    groups: Vec<usize>,
}

impl Recluster {
    pub fn run(self) -> Reclustered {
        let voices: Vec<&[f32]> = self.voices.iter().map(Vec::as_slice).collect();
        let groups = cluster(&voices, &self.weights, self.threshold, self.min_share);
        Reclustered {
            ids: self.ids,
            groups,
        }
    }
}

/// A speaker's running voice: the sum of their windows.
struct Voice {
    sum: Vec<f32>,
    windows: usize,
    /// How many windows were heard before they first spoke.
    since: usize,
}

impl Voice {
    fn similarity(&self, embedding: &[f32]) -> f32 {
        let norm = dot(&self.sum, &self.sum).sqrt();
        if norm == 0.0 {
            return 0.0;
        }
        dot(&self.sum, embedding) / norm
    }

    fn unit(&self) -> Vec<f32> {
        normalized(self.sum.clone())
    }
}

/// The speakers heard so far, numbered as they first speak; a joined speaker's
/// number is not used again.
pub struct Speakers {
    clustering: Clustering,
    voices: Vec<Option<Voice>>,
    windows: usize,
    last: Option<usize>,
    /// The stretches of the last [`REACH`] seconds, in time order.
    stretches: Vec<Stretch>,
    stretched: u64,
    /// Where the recording is heard up to, in seconds.
    heard: f64,
    /// How many windows were heard when the stretches were last clustered.
    clustered: usize,
}

impl Speakers {
    pub fn new(clustering: Clustering) -> Self {
        Self {
            clustering,
            voices: Vec::new(),
            windows: 0,
            last: None,
            stretches: Vec::new(),
            stretched: 0,
            heard: 0.0,
            clustered: 0,
        }
    }

    fn live(&self) -> impl Iterator<Item = (usize, &Voice)> {
        let voices = self.voices.iter().enumerate();
        voices.filter_map(|(speaker, voice)| voice.as_ref().map(|voice| (speaker, voice)))
    }

    /// The speaker of one window: the nearest, or a new one when every voice is
    /// at least the threshold away.
    fn assign(&mut self, embedding: &[f32]) -> usize {
        let nearest = self
            .live()
            .map(|(speaker, voice)| (speaker, voice.similarity(embedding)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let full = self.live().count() >= MAX_SPEAKERS;
        let speaker = match nearest {
            Some((speaker, similarity)) if 1.0 - similarity < self.clustering.threshold || full => {
                speaker
            }
            _ => {
                self.voices.push(Some(Voice {
                    sum: vec![0.0; embedding.len()],
                    windows: 0,
                    since: self.windows,
                }));
                self.voices.len() - 1
            }
        };
        let voice = self.voices[speaker].as_mut().expect("a live speaker");
        voice
            .sum
            .iter_mut()
            .zip(embedding)
            .for_each(|(s, x)| *s += x);
        voice.windows += 1;
        self.windows += 1;
        speaker
    }

    /// `gone` was `kept` all along.
    fn join(&mut self, gone_id: usize, kept: usize) {
        let gone = self.voices[gone_id].take().expect("a live speaker");
        let voice = self.voices[kept].as_mut().expect("a live speaker");
        voice
            .sum
            .iter_mut()
            .zip(&gone.sum)
            .for_each(|(s, x)| *s += x);
        voice.windows += gone.windows;
        voice.since = voice.since.min(gone.since);
        for stretch in self.stretches.iter_mut().filter(|s| s.speaker == gone_id) {
            stretch.speaker = kept;
        }
    }

    /// The two speakers whose voices are closest, when closer than the merge
    /// distance, as (gone, kept): the one heard less goes.
    fn closest(&self) -> Option<(usize, usize)> {
        let units: Vec<(usize, Vec<f32>, usize)> = self
            .live()
            .map(|(speaker, voice)| (speaker, voice.unit(), voice.windows))
            .collect();
        let mut closest: Option<(usize, usize, f32)> = None;
        for (index, a) in units.iter().enumerate() {
            for b in &units[index + 1..] {
                let distance = 1.0 - dot(&a.1, &b.1);
                if closest.is_none_or(|(_, _, nearest)| distance < nearest) {
                    let (gone, kept) = if b.2 > a.2 { (a.0, b.0) } else { (b.0, a.0) };
                    closest = Some((gone, kept, distance));
                }
            }
        }
        closest
            .filter(|&(_, _, distance)| distance < self.clustering.merge)
            .map(|(gone, kept, _)| (gone, kept))
    }

    /// A settled speaker holding less than the minimum share of the windows,
    /// and the larger speaker nearest them, as (gone, kept).
    fn small(&self) -> Option<(usize, usize)> {
        let minimum = self.clustering.min_share * self.windows as f32;
        let large: Vec<(usize, Vec<f32>)> = self
            .live()
            .filter(|(_, voice)| voice.windows as f32 >= minimum)
            .map(|(speaker, voice)| (speaker, voice.unit()))
            .collect();
        let (gone, voice) = self.live().find(|(_, voice)| {
            (voice.windows as f32) < minimum && self.windows - voice.since >= self.clustering.settle
        })?;
        let voice = voice.unit();
        let nearest = large
            .iter()
            .max_by(|a, b| dot(&a.1, &voice).total_cmp(&dot(&b.1, &voice)))?;
        Some((gone, nearest.0))
    }

    /// Join speakers whose voices came close, then settled ones too small to be
    /// anyone, each as (gone, kept).
    fn merge(&mut self) -> Vec<(usize, usize)> {
        let mut merged = Vec::new();
        while let Some((gone, kept)) = self.closest().or_else(|| self.small()) {
            self.join(gone, kept);
            merged.push((gone, kept));
        }
        merged
    }

    /// Keep the runs of windows of one speaker in `region`, whose windows were
    /// said by `speakers`, and forget the stretches past the reach.
    fn keep_stretches(&mut self, region: &Region, speakers: impl Iterator<Item = usize>) {
        let first = self.stretches.len();
        for (window, speaker) in region.windows.iter().zip(speakers) {
            match self.stretches.get_mut(first..).and_then(<[_]>::last_mut) {
                Some(last) if last.speaker == speaker => {
                    last.end = window.end;
                    last.windows += 1;
                    last.sum
                        .iter_mut()
                        .zip(&window.embedding)
                        .for_each(|(s, x)| *s += x);
                }
                _ => {
                    self.stretches.push(Stretch {
                        id: self.stretched,
                        start: window.start,
                        end: window.end,
                        sum: window.embedding.clone(),
                        windows: 1,
                        speaker,
                    });
                    self.stretched += 1;
                }
            }
        }
        let oldest = self.heard - REACH;
        self.stretches.retain(|stretch| stretch.end >= oldest);
    }

    /// Whether enough speech was heard since the last re-cluster for another.
    pub fn due(&self) -> bool {
        self.windows - self.clustered >= self.clustering.every
    }

    /// The stretches to cluster again, or `None` when there are fewer than two.
    pub fn snapshot(&mut self) -> Option<Recluster> {
        self.clustered = self.windows;
        if self.stretches.len() < 2 {
            return None;
        }
        let stretches = self.stretches.iter();
        Some(Recluster {
            ids: stretches.clone().map(|s| s.id).collect(),
            voices: stretches
                .clone()
                .map(|s| normalized(s.sum.clone()))
                .collect(),
            weights: stretches.map(|s| s.windows).collect(),
            threshold: self.clustering.recluster,
            min_share: self.clustering.min_share,
        })
    }

    /// Cluster the stretches again now, and move what changed.
    pub fn recluster(&mut self) -> Option<Update> {
        let groups = self.snapshot()?.run();
        self.regroup(&groups)
    }

    /// Apply the groups found for the stretches as they were: each group keeps
    /// the speaker most of it was already said by, one speaker to a group, and
    /// the others are new speakers. What was heard since stays where it is.
    /// `None` when nothing moved.
    pub fn regroup(&mut self, found: &Reclustered) -> Option<Update> {
        let group_of = |stretch: &Stretch| {
            let at = found.ids.binary_search(&stretch.id).ok()?;
            Some(found.groups[at])
        };
        let mut shared: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for stretch in &self.stretches {
            if let Some(group) = group_of(stretch) {
                *shared.entry((group, stretch.speaker)).or_default() += stretch.windows;
            }
        }
        let mut pairs: Vec<((usize, usize), usize)> = shared.into_iter().collect();
        pairs.sort_by_key(|pair| std::cmp::Reverse(pair.1));
        let mut speaker_of_group: BTreeMap<usize, usize> = BTreeMap::new();
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        for ((group, speaker), _) in pairs {
            if !speaker_of_group.contains_key(&group) && taken.insert(speaker) {
                speaker_of_group.insert(group, speaker);
            }
        }
        let mut moves: Vec<Moved> = Vec::new();
        for index in 0..self.stretches.len() {
            let stretch = &self.stretches[index];
            let Some(group) = group_of(stretch) else {
                continue;
            };
            let to = match speaker_of_group.get(&group) {
                Some(&speaker) => speaker,
                None => {
                    self.voices.push(Some(Voice {
                        sum: vec![0.0; stretch.sum.len()],
                        windows: 0,
                        since: self.windows,
                    }));
                    speaker_of_group.insert(group, self.voices.len() - 1);
                    self.voices.len() - 1
                }
            };
            let stretch = &mut self.stretches[index];
            if stretch.speaker == to {
                continue;
            }
            let from = std::mem::replace(&mut stretch.speaker, to);
            let (start, end, windows) = (stretch.start, stretch.end, stretch.windows);
            let sum = stretch.sum.clone();
            self.shift(&sum, windows, from, to);
            moves.push(Moved {
                start,
                end,
                from,
                to,
            });
        }
        let mut merged: Vec<(usize, usize)> = Vec::new();
        let emptied: Vec<usize> = self
            .live()
            .filter(|(_, voice)| voice.windows == 0)
            .map(|(speaker, _)| speaker)
            .collect();
        for gone in emptied {
            let to: BTreeSet<usize> = moves
                .iter()
                .filter(|m| m.from == gone)
                .map(|m| m.to)
                .collect();
            if let (1, Some(&kept)) = (to.len(), to.first()) {
                merged.push((gone, kept));
                moves.retain(|m| m.from != gone);
            }
            self.voices[gone] = None;
        }
        if moves.is_empty() && merged.is_empty() {
            return None;
        }
        let now = |speaker: usize| {
            merged
                .iter()
                .find(|&&(gone, _)| gone == speaker)
                .map_or(speaker, |&(_, kept)| kept)
        };
        self.last = self
            .last
            .map(now)
            .filter(|&speaker| self.voices[speaker].is_some())
            .or_else(|| self.stretches.last().map(|s| s.speaker));
        let mut moved: Vec<Moved> = Vec::new();
        for span in moves {
            match moved.last_mut() {
                Some(last)
                    if (last.from, last.to) == (span.from, span.to)
                        && span.start - last.end < EPSILON =>
                {
                    last.end = span.end;
                }
                _ => moved.push(span),
            }
        }
        let touched: BTreeSet<usize> = moved
            .iter()
            .flat_map(|m| [m.from, m.to])
            .chain(merged.iter().map(|&(_, kept)| kept))
            .filter(|&speaker| self.voices[speaker].is_some())
            .collect();
        let voices = touched
            .into_iter()
            .filter_map(|speaker| Some((speaker, self.voices[speaker].as_ref()?.unit())))
            .collect();
        Some(Update {
            end: self.heard,
            turns: Vec::new(),
            voices,
            merged,
            moved,
        })
    }

    /// Take a stretch's voice from the speaker `from` to the speaker `to`.
    fn shift(&mut self, sum: &[f32], windows: usize, from: usize, to: usize) {
        let from = self.voices[from].as_mut().expect("a live speaker");
        from.sum.iter_mut().zip(sum).for_each(|(s, x)| *s -= x);
        from.windows -= windows;
        let to = self.voices[to].as_mut().expect("a live speaker");
        to.sum.iter_mut().zip(sum).for_each(|(s, x)| *s += x);
        to.windows += windows;
    }

    /// Hear one region: each window goes to a speaker, speakers that came
    /// together are joined, and the region's turns are told.
    pub fn hear(&mut self, region: &Region) -> Update {
        let assigned: Vec<usize> = region
            .windows
            .iter()
            .map(|window| self.assign(&window.embedding))
            .collect();
        let merged = if assigned.is_empty() {
            Vec::new()
        } else {
            self.merge()
        };
        let now = |mut speaker: usize| {
            for &(gone, kept) in &merged {
                if speaker == gone {
                    speaker = kept;
                }
            }
            speaker
        };
        self.last = assigned.last().copied().or(self.last).map(now);
        self.heard = region.end;
        self.keep_stretches(region, assigned.iter().map(|&speaker| now(speaker)));
        let spans: Vec<(f64, f64, usize)> = if region.windows.is_empty() {
            let last = self.last.map(|speaker| (region.start, region.end, speaker));
            last.into_iter().collect()
        } else {
            let windows = region.windows.iter().zip(&assigned);
            windows
                .map(|(w, &speaker)| (w.start, w.end, now(speaker)))
                .collect()
        };
        let mut turns: Vec<Turn> = Vec::new();
        for (start, end, speaker) in spans {
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
        let mut changed: BTreeSet<usize> = assigned.iter().map(|&speaker| now(speaker)).collect();
        changed.extend(merged.iter().map(|&(_, kept)| now(kept)));
        let voices = changed
            .into_iter()
            .filter_map(|speaker| Some((speaker, self.voices[speaker].as_ref()?.unit())))
            .collect();
        Update {
            end: region.end,
            turns,
            voices,
            merged,
            moved: Vec::new(),
        }
    }
}

/// Embeds each speech region as it arrives and tells whose voice it is.
pub struct Diarizer<'a> {
    embedder: &'a dyn SpeakerEmbedder,
    speakers: Speakers,
}

impl<'a> Diarizer<'a> {
    pub fn new(embedder: &'a dyn SpeakerEmbedder, clustering: Clustering) -> Self {
        Self {
            embedder,
            speakers: Speakers::new(clustering),
        }
    }

    /// Hear the speech region `pcm`, which starts `start` seconds into the recording.
    pub fn add(&mut self, start: f64, pcm: &[i16]) -> Result<Update, EmbeddingError> {
        let region = region(self.embedder, start, pcm)?;
        Ok(self.speakers.hear(&region))
    }

    /// Whether enough speech was heard for the stretches to be clustered again.
    pub fn due(&self) -> bool {
        self.speakers.due()
    }

    /// The stretches to cluster again, to be run apart from the diarizer.
    pub fn snapshot(&mut self) -> Option<Recluster> {
        self.speakers.snapshot()
    }

    /// Move what a clustering of the stretches of `snapshot` changed; `None`
    /// when nothing moved.
    pub fn regroup(&mut self, found: &Reclustered) -> Option<Update> {
        self.speakers.regroup(found)
    }
}

/// A whole recording's updates, gathered into who spoke when.
#[derive(Default)]
pub struct Recording {
    turns: Vec<Turn>,
    voices: BTreeMap<usize, Vec<f32>>,
}

impl Recording {
    pub fn apply(&mut self, update: Update) {
        for &(gone, kept) in &update.merged {
            let joined = self.turns.iter_mut().filter(|t| t.speaker == gone);
            joined.for_each(|t| t.speaker = kept);
            self.voices.remove(&gone);
        }
        move_spans(&mut self.turns, &update.moved);
        self.turns.extend(update.turns);
        self.voices.extend(update.voices);
    }

    /// The turns so far, in time order, by the speakers they are now.
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// The turns of the whole recording, in time order, and the speakers' voices.
    pub fn finish(self) -> Diarization {
        let mut spans: Vec<(f64, f64, usize)> = self
            .turns
            .iter()
            .map(|t| (t.start, t.end, t.speaker))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Speakers are numbered in the order they first speak.
        let mut order: Vec<usize> = Vec::new();
        let mut turns: Vec<Turn> = Vec::new();
        for (start, end, found) in spans {
            let speaker = order.iter().position(|&s| s == found).unwrap_or_else(|| {
                order.push(found);
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
            .map(|speaker| self.voices.get(speaker).cloned().unwrap_or_default())
            .collect();
        Diarization { turns, voices }
    }
}

/// What `regions`, heard in order, tell: an update per region, one more after
/// every [`Clustering::every`] windows of speech and one when they end, as a
/// session tells them.
pub fn updates(regions: &[Region], clustering: Clustering) -> Vec<Update> {
    let mut speakers = Speakers::new(clustering);
    let mut told = Vec::new();
    for region in regions {
        told.push(speakers.hear(region));
        if speakers.due() {
            told.extend(speakers.recluster());
        }
    }
    told.extend(speakers.recluster());
    told
}

/// Who spoke when in `regions`, heard in order and clustered again as a session does.
pub fn diarize(regions: &[Region], clustering: Clustering) -> Diarization {
    let mut recording = Recording::default();
    for update in updates(regions, clustering) {
        recording.apply(update);
    }
    recording.finish()
}

/// A line a live input became: its session, its time, and where its speech
/// starts and ends in the input, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardLine {
    pub session: String,
    pub at: f64,
    pub start: f64,
    pub end: f64,
    /// Its words, timed, when the transcriber times them.
    pub words: Vec<Word>,
}

/// The least a part of a line talks, in seconds; a speaker who changes for
/// less is the diarizer's jitter and stays with the neighbouring part.
const PART: f64 = 0.5;

/// A stretch of a line said by one speaker, from the word at index `first`.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub first: usize,
    pub speaker: usize,
}

/// A heard line with the parts the diarizer divides it in: its time, its words
/// as written, and one part per speaker's stretch.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub at: f64,
    pub words: Vec<String>,
    pub parts: Vec<Part>,
}

/// The parts of a line spanning `[start, end)` with `words`, from `turns`: each
/// word goes to the speaker who talks most over it, and the line is cut where
/// that changes, at the word nearest the change. A speaker who talks less than
/// [`PART`] joins the longer neighbouring part. A line without word times, or
/// said by one speaker, is one part, by [`speaker_of`]. Empty without turns.
pub fn parts(turns: &[Turn], words: &[Word], start: f64, end: f64) -> Vec<Part> {
    let mut runs: Vec<Run> = Vec::new();
    for (first, word) in words.iter().enumerate() {
        let Some(speaker) = speaker_of(turns, word.start, word.end) else {
            return Vec::new();
        };
        let talk = (word.end - word.start).max(0.0);
        runs.push(Run {
            first,
            speaker,
            talk,
        });
    }
    join_same(&mut runs);
    while runs.len() > 1 {
        let shortest = (0..runs.len()).min_by(|&a, &b| runs[a].talk.total_cmp(&runs[b].talk));
        let Some(short) = shortest.filter(|&i| runs[i].talk < PART) else {
            break;
        };
        let run = runs.remove(short);
        let (before, after) = (
            short.checked_sub(1),
            Some(short).filter(|&i| i < runs.len()),
        );
        let into = match (before, after) {
            (Some(b), Some(a)) if runs[a].talk > runs[b].talk => a,
            (Some(b), _) => b,
            (None, a) => a.expect("a neighbour remains"),
        };
        runs[into].talk += run.talk;
        runs[into].first = runs[into].first.min(run.first);
        join_same(&mut runs);
    }
    match runs.as_slice() {
        [_, _, ..] => runs
            .into_iter()
            .map(|run| Part {
                first: run.first,
                speaker: run.speaker,
            })
            .collect(),
        _ => speaker_of(turns, start, end)
            .map(|speaker| Part { first: 0, speaker })
            .into_iter()
            .collect(),
    }
}

/// Consecutive words of one speaker.
struct Run {
    first: usize,
    speaker: usize,
    talk: f64,
}

/// Fold neighbouring runs of one speaker into one.
fn join_same(runs: &mut Vec<Run>) {
    let mut joined: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        match joined.last_mut() {
            Some(last) if last.speaker == run.speaker => last.talk += run.talk,
            _ => joined.push(run),
        }
    }
    *runs = joined;
}

/// What a session learns of a live input's voices: its lines, by time, with
/// the speaker who said each; speakers found to be one, as (gone, kept); and
/// the latest voice of each speaker involved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Said {
    pub lines: Vec<Placed>,
    pub merged: Vec<(usize, usize)>,
    pub voices: BTreeMap<usize, Vec<f32>>,
}

/// What became known of a live input's voices, for every session it records for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub lines: Vec<(HeardLine, Vec<Part>)>,
    pub merged: Vec<(usize, usize)>,
    pub voices: BTreeMap<usize, Vec<f32>>,
}

impl Known {
    /// What the session `session` learns.
    pub fn of(&self, session: &str) -> Said {
        Said {
            lines: self
                .lines
                .iter()
                .filter(|(line, _)| line.session == session)
                .map(|(line, parts)| Placed {
                    at: line.at,
                    words: line.words.iter().map(|w| w.text.clone()).collect(),
                    parts: parts.clone(),
                })
                .collect(),
            merged: self.merged.clone(),
            voices: self.voices.clone(),
        }
    }
}

/// The voices of a live input as the diarizer tells them: the recent turns,
/// each speaker's latest voice, and the lines that wait for the diarizer to
/// hear past them. All of it is bounded, however long the input records.
#[derive(Default)]
pub struct Live {
    turns: Vec<Turn>,
    horizon: f64,
    waiting: Vec<HeardLine>,
    /// Lines, with the parts last told, while their turns can still change.
    settled: Vec<(HeardLine, Vec<Part>)>,
    voices: BTreeMap<usize, Vec<f32>>,
    ended: bool,
}

impl Live {
    /// A line was heard: its speaker is known once the diarizer has heard past
    /// its end, or has stopped.
    pub fn line(&mut self, line: HeardLine) -> Known {
        let mut known = Known::default();
        if self.ended || line.end <= self.horizon {
            self.said(line, &mut known);
        } else {
            self.waiting.push(line);
            if self.waiting.len() > WAITING {
                let oldest = self.waiting.remove(0);
                self.said(oldest, &mut known);
            }
        }
        known
    }

    /// The diarizer heard one more region.
    pub fn update(&mut self, update: Update) -> Known {
        let mut known = Known {
            merged: update.merged,
            ..Known::default()
        };
        for &(gone, kept) in &known.merged {
            let joined = self.turns.iter_mut().filter(|t| t.speaker == gone);
            joined.for_each(|t| t.speaker = kept);
            self.voices.remove(&gone);
        }
        move_spans(&mut self.turns, &update.moved);
        self.turns.extend(update.turns);
        self.horizon = self.horizon.max(update.end);
        for (speaker, voice) in update.voices {
            known.voices.insert(speaker, voice.clone());
            self.voices.insert(speaker, voice);
        }
        let horizon = self.horizon;
        for (line, told) in &mut self.settled {
            let moved = update
                .moved
                .iter()
                .any(|span| span.start < line.end && span.end > line.start);
            let followed = !line.words.is_empty() && line.end >= horizon - RECENT;
            if !followed && !moved {
                continue;
            }
            let now = parts(
                around(&self.turns, line.start, line.end),
                &line.words,
                line.start,
                line.end,
            );
            if !now.is_empty() && now != *told {
                Self::tell(&self.voices, line.clone(), now.clone(), &mut known);
                *told = now;
            }
        }
        let (ready, waiting): (Vec<HeardLine>, Vec<HeardLine>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|line| line.end <= horizon);
        self.waiting = waiting;
        for line in ready {
            self.said(line, &mut known);
        }
        self.turns.retain(|t| t.end >= horizon - REACH);
        self.settled
            .retain(|(line, _)| line.start >= horizon - REACH);
        known
    }

    /// The diarizer stopped: the lines waiting, and any heard later, go to the
    /// speakers of the turns it told.
    pub fn finish(&mut self) -> Known {
        self.ended = true;
        let mut known = Known::default();
        for line in std::mem::take(&mut self.waiting) {
            self.said(line, &mut known);
        }
        known
    }

    fn said(&mut self, line: HeardLine, known: &mut Known) {
        let turns = around(&self.turns, line.start, line.end);
        let parts = parts(turns, &line.words, line.start, line.end);
        if parts.is_empty() {
            return;
        }
        self.settled.push((line.clone(), parts.clone()));
        Self::tell(&self.voices, line, parts, known);
    }

    /// Tell `line` is said as `parts`, with the voices of their speakers.
    fn tell(
        voices: &BTreeMap<usize, Vec<f32>>,
        line: HeardLine,
        parts: Vec<Part>,
        known: &mut Known,
    ) {
        for part in &parts {
            if let Some(voice) = voices.get(&part.speaker) {
                known
                    .voices
                    .entry(part.speaker)
                    .or_insert_with(|| voice.clone());
            }
        }
        known.lines.push((line, parts));
    }
}

/// The turns near a stretch spanning `[start, end)`, which is all of `turns`
/// when none is: turns are in time order.
pub fn around(turns: &[Turn], start: f64, end: f64) -> &[Turn] {
    const NEAR: f64 = 60.0;
    let first = turns.partition_point(|t| t.end < start - NEAR);
    let last = turns.partition_point(|t| t.start <= end + NEAR);
    match turns.get(first..last) {
        Some(near) if !near.is_empty() => near,
        _ => turns,
    }
}

/// The label a live speaker carries in one session, and the voices kept for it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Labelled {
    pub label: String,
    /// The voice the session kept for the label before this speaker took it.
    pub base: Option<Vec<f32>>,
    /// The voice last kept for the label.
    pub kept: Option<Vec<f32>>,
}

/// The label of each live speaker in one session, by speaker.
pub type Labels = BTreeMap<usize, Labelled>;

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

/// The dot product: the cosine of two unit vectors.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
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

    /// The regions of `clips`, each `(start, pcm)`.
    fn regions(clips: &[(f64, Vec<i16>)]) -> Vec<Region> {
        let embedder = BySign;
        clips
            .iter()
            .map(|(start, pcm)| region(&embedder, *start, pcm).unwrap())
            .collect()
    }

    /// A region of one window per voice, each a second long, from `start`.
    fn windows(start: f64, voices: &[&[f32]]) -> Region {
        let windows = voices.iter().enumerate().map(|(index, voice)| {
            let at = start + index as f64;
            (at, at + 1.0, normalized(voice.to_vec()))
        });
        Region::new(start, start + voices.len() as f64, windows.collect())
    }

    fn rounded(turns: &[Turn]) -> Vec<(f64, f64, usize)> {
        let tenth = |x: f64| (x * 10.0).round() / 10.0;
        turns
            .iter()
            .map(|t| (tenth(t.start), tenth(t.end), t.speaker))
            .collect()
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
        let heard = regions(&[
            (1.0, speech(100, 4.0)),
            (6.0, speech(-100, 3.0)),
            (10.0, speech(100, 2.0)),
            (12.5, speech(-100, 0.2)), // too short: the speaker heard last
        ]);
        let Diarization { turns, voices } = diarize(&heard, Clustering::default());
        assert_eq!(voices.len(), 2);
        assert!(dot(&voices[0], &[1.0, 0.1, 0.0]) > 0.99);
        assert!(dot(&voices[1], &[0.0, 0.1, 1.0]) > 0.99);
        assert_eq!(
            rounded(&turns),
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
    fn each_region_is_told_as_it_is_heard() {
        let embedder = BySign;
        let mut diarizer = Diarizer::new(&embedder, Clustering::default());
        let first = diarizer.add(1.0, &speech(100, 2.0)).unwrap();
        assert_eq!(first.end, 3.0);
        assert_eq!(rounded(&first.turns), [(1.0, 3.0, 0)]);
        assert_eq!(first.voices.len(), 1);
        assert!(dot(&first.voices[0].1, &[1.0, 0.1, 0.0]) > 0.99);
        assert!(first.merged.is_empty());

        let second = diarizer.add(4.0, &speech(-100, 2.0)).unwrap();
        assert_eq!(rounded(&second.turns), [(4.0, 6.0, 1)]);
        let changed: Vec<usize> = second.voices.iter().map(|v| v.0).collect();
        assert_eq!(changed, [1]);

        let short = diarizer.add(7.0, &speech(100, 0.2)).unwrap();
        assert_eq!(rounded(&short.turns), [(7.0, 7.2, 1)]);
        assert!(short.voices.is_empty());
    }

    #[test]
    fn a_short_region_before_any_voice_has_no_speaker() {
        let embedder = BySign;
        let mut diarizer = Diarizer::new(&embedder, Clustering::default());
        let short = diarizer.add(0.0, &speech(100, 0.2)).unwrap();
        assert!(short.turns.is_empty());
        let Diarization { turns, voices } = diarize(
            &regions(&[(1.0, speech(100, 0.2)), (2.0, speech(-100, 0.3))]),
            Clustering::default(),
        );
        assert!(turns.is_empty());
        assert!(voices.is_empty());
    }

    #[test]
    fn speakers_whose_voices_come_close_are_joined() {
        let mut speakers = Speakers::new(Clustering {
            threshold: 0.2,
            merge: 0.5,
            min_share: 0.0,
            ..Clustering::default()
        });
        // The second window is past the threshold, so it starts a speaker; the
        // two voices are within the merge distance, so they are one.
        let update = speakers.hear(&windows(0.0, &[&[1.0, 0.0], &[0.6, 0.8]]));
        assert_eq!(update.merged, [(1, 0)]);
        assert_eq!(rounded(&update.turns), [(0.0, 2.0, 0)]);
        assert_eq!(update.voices.len(), 1);
        assert_eq!(update.voices[0].0, 0);
        assert!(dot(&update.voices[0].1, &normalized(vec![1.6, 0.8])) > 0.9999);
        // A joined speaker's number is not used again.
        let update = speakers.hear(&windows(2.0, &[&[0.0, 1.0]]));
        assert_eq!(rounded(&update.turns), [(2.0, 3.0, 2)]);
        assert!(update.merged.is_empty());
    }

    #[test]
    fn a_settled_speaker_too_small_joins_the_nearest_larger_one() {
        let mut speakers = Speakers::new(Clustering::default());
        let (a, b): (&[f32], &[f32]) = (&[1.0, 0.0], &[0.0, 1.0]);
        let first = speakers.hear(&windows(0.0, &[a, b]));
        assert!(first.merged.is_empty(), "not settled yet");
        let more = vec![a; Clustering::default().settle + 5];
        let second = speakers.hear(&windows(2.0, &more));
        assert_eq!(second.merged, [(1, 0)]);
        assert_eq!(speakers.live().count(), 1);
    }

    #[test]
    fn speakers_are_bounded() {
        let mut speakers = Speakers::new(Clustering {
            threshold: 0.5,
            merge: 0.0,
            min_share: 0.0,
            ..Clustering::default()
        });
        let axes: Vec<Vec<f32>> = (0..=MAX_SPEAKERS)
            .map(|axis| {
                let mut voice = vec![0.0; MAX_SPEAKERS + 1];
                voice[axis] = 1.0;
                voice
            })
            .collect();
        let voices: Vec<&[f32]> = axes.iter().map(Vec::as_slice).collect();
        let update = speakers.hear(&windows(0.0, &voices));
        assert_eq!(speakers.live().count(), MAX_SPEAKERS);
        assert_eq!(update.voices.len(), MAX_SPEAKERS);
    }

    #[test]
    fn a_recording_gathers_its_updates_with_speakers_joined() {
        let mut recording = Recording::default();
        let turn = |start, end, speaker| Turn {
            start,
            end,
            speaker,
        };
        recording.apply(Update {
            end: 2.0,
            turns: vec![turn(0.0, 1.0, 3), turn(1.0, 2.0, 5)],
            voices: vec![(3, vec![1.0, 0.0]), (5, vec![0.0, 1.0])],
            merged: Vec::new(),
            moved: Vec::new(),
        });
        recording.apply(Update {
            end: 4.0,
            turns: vec![turn(2.5, 4.0, 7)],
            voices: vec![(7, vec![0.6, 0.8])],
            merged: vec![(3, 5)],
            moved: Vec::new(),
        });
        assert_eq!(
            recording.finish(),
            Diarization {
                turns: vec![turn(0.0, 2.0, 0), turn(2.5, 4.0, 1)],
                voices: vec![vec![0.0, 1.0], vec![0.6, 0.8]],
            }
        );
    }

    #[test]
    fn silence_long_enough_is_a_voice_of_its_own() {
        let heard = regions(&[(0.0, speech(100, 2.0)), (3.0, speech(0, 2.0))]);
        let Diarization { turns, voices } = diarize(&heard, Clustering::default());
        let speakers: Vec<usize> = turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, [0, 1]);
        assert_eq!(voices.len(), 2);
    }

    #[test]
    fn a_zero_vector_stays_zero() {
        assert_eq!(normalized(vec![0.0, 0.0]), [0.0, 0.0]);
    }

    #[test]
    fn a_voice_with_no_sound_is_near_no_one() {
        let mut speakers = Speakers::new(Clustering::default());
        assert_eq!(speakers.assign(&[0.0, 0.0]), 0);
        assert_eq!(speakers.assign(&[1.0, 0.0]), 1);
    }

    fn line(session: &str, at: f64, start: f64, end: f64) -> HeardLine {
        HeardLine {
            session: session.into(),
            at,
            start,
            end,
            words: Vec::new(),
        }
    }

    /// A line said whole by `speaker`.
    fn whole(speaker: usize) -> Vec<Part> {
        vec![Part { first: 0, speaker }]
    }

    #[test]
    fn a_live_line_gets_its_speaker_once_the_diarizer_heard_it() {
        let mut live = Live::default();
        // Heard before the diarizer reaches it: it waits.
        assert_eq!(live.line(line("n1", 100.0, 0.0, 2.0)), Known::default());
        let known = live.update(Update {
            end: 3.0,
            turns: vec![Turn {
                start: 0.0,
                end: 3.0,
                speaker: 0,
            }],
            voices: vec![(0, vec![1.0, 0.0])],
            merged: Vec::new(),
            moved: Vec::new(),
        });
        assert_eq!(known.lines, [(line("n1", 100.0, 0.0, 2.0), whole(0))]);
        assert_eq!(known.voices[&0], [1.0, 0.0]);

        // Heard after: told at once, with its speaker's voice.
        let known = live.line(line("n2", 101.0, 1.0, 3.0));
        assert_eq!(known.lines, [(line("n2", 101.0, 1.0, 3.0), whole(0))]);
        assert_eq!(known.voices[&0], [1.0, 0.0]);
        assert_eq!(
            known.of("n2"),
            Said {
                lines: vec![Placed {
                    at: 101.0,
                    words: Vec::new(),
                    parts: whole(0)
                }],
                merged: Vec::new(),
                voices: known.voices.clone(),
            }
        );
        assert!(known.of("n1").lines.is_empty());

        // A speaker joined into another: the turns kept follow.
        let known = live.update(Update {
            end: 6.0,
            turns: vec![Turn {
                start: 4.0,
                end: 6.0,
                speaker: 1,
            }],
            voices: vec![(1, vec![0.0, 1.0])],
            merged: vec![(0, 1)],
            moved: Vec::new(),
        });
        assert_eq!(known.merged, [(0, 1)]);
        assert!(known.lines.is_empty());
        let known = live.line(line("n1", 102.0, 0.0, 1.0));
        assert_eq!(known.lines[0].1, whole(1));
        assert_eq!(known.voices.keys().copied().collect::<Vec<_>>(), [1]);

        // The diarizer stops: the lines waiting go to the turns it told, and
        // later lines at once.
        assert!(live.line(line("n1", 103.0, 6.5, 7.0)).lines.is_empty());
        let known = live.finish();
        assert_eq!(known.lines, [(line("n1", 103.0, 6.5, 7.0), whole(1))]);
        assert_eq!(live.line(line("n1", 104.0, 9.0, 10.0)).lines.len(), 1);
    }

    #[test]
    fn a_live_input_keeps_recent_turns_and_a_bounded_wait() {
        let mut live = Live::default();
        for index in 0..WAITING {
            let known = live.line(line("n1", index as f64, 10.0, 11.0));
            assert!(known.lines.is_empty());
        }
        // One more than the wait holds: the oldest goes, and no speaker is known yet.
        assert!(live.line(line("n1", 999.0, 10.0, 11.0)).lines.is_empty());
        assert_eq!(live.waiting.len(), WAITING);
        let turn = |start, end, speaker| Turn {
            start,
            end,
            speaker,
        };
        let known = live.update(Update {
            end: 1000.0,
            turns: vec![turn(0.0, 20.0, 0), turn(900.0, 1000.0, 1)],
            voices: Vec::new(),
            merged: Vec::new(),
            moved: Vec::new(),
        });
        assert_eq!(known.lines.len(), WAITING);
        assert!(known.lines.iter().all(|(_, parts)| *parts == whole(0)));
        assert!(known.voices.is_empty());
        assert_eq!(live.turns.len(), 2, "turns are kept within the reach");
        assert!(live.waiting.is_empty());
        live.update(heard(
            REACH + 2000.0,
            vec![turn(REACH + 1900.0, REACH + 2000.0, 1)],
        ));
        assert_eq!(live.turns, [turn(REACH + 1900.0, REACH + 2000.0, 1)]);
    }

    fn turn(start: f64, end: f64, speaker: usize) -> Turn {
        Turn {
            start,
            end,
            speaker,
        }
    }

    /// Words of a second each, from `start`.
    fn spoken(start: f64, texts: &[&str]) -> Vec<Word> {
        let words = texts.iter().enumerate().map(|(index, text)| Word {
            start: start + index as f64,
            end: start + index as f64 + 1.0,
            text: (*text).into(),
        });
        words.collect()
    }

    fn firsts(parts: &[Part]) -> Vec<(usize, usize)> {
        parts.iter().map(|p| (p.first, p.speaker)).collect()
    }

    #[test]
    fn a_line_over_two_turns_is_cut_at_the_word_nearest_the_change() {
        let words = spoken(0.0, &["Tudo", "bem", "com", "voce", "sim", "e", "voce"]);
        let turns = [turn(0.0, 4.2, 0), turn(4.2, 7.0, 1)];
        assert_eq!(firsts(&parts(&turns, &words, 0.0, 7.0)), [(0, 0), (4, 1)]);
        let later = [turn(0.0, 5.1, 0), turn(5.1, 7.0, 1)];
        assert_eq!(firsts(&parts(&later, &words, 0.0, 7.0)), [(0, 0), (5, 1)]);
        let back = [turn(0.0, 2.0, 0), turn(2.0, 5.0, 1), turn(5.0, 7.0, 0)];
        assert_eq!(
            firsts(&parts(&back, &words, 0.0, 7.0)),
            [(0, 0), (2, 1), (5, 0)]
        );
    }

    #[test]
    fn a_line_said_by_one_speaker_is_one_part_even_over_a_blip() {
        let words = spoken(0.0, &["Tudo", "bem", "com", "voce"]);
        let blip = [turn(0.0, 1.0, 0), turn(1.0, 1.3, 1), turn(1.3, 4.0, 0)];
        assert_eq!(firsts(&parts(&blip, &words, 0.0, 4.0)), [(0, 0)]);
        let first = [turn(0.0, 0.3, 1), turn(0.3, 4.0, 0)];
        assert_eq!(firsts(&parts(&first, &words, 0.0, 4.0)), [(0, 0)]);
        let one = [turn(0.0, 4.0, 3)];
        assert_eq!(firsts(&parts(&one, &words, 0.0, 4.0)), [(0, 3)]);
    }

    #[test]
    fn a_line_without_word_times_goes_whole_to_who_talks_most() {
        let turns = [turn(0.0, 1.0, 0), turn(1.0, 4.0, 1)];
        assert_eq!(firsts(&parts(&turns, &[], 0.0, 4.0)), [(0, 1)]);
        assert!(parts(&[], &[], 0.0, 4.0).is_empty());
        let words = spoken(0.0, &["a", "b"]);
        assert!(parts(&[], &words, 0.0, 2.0).is_empty());
    }

    fn timed(session: &str, at: f64, words: Vec<Word>) -> HeardLine {
        HeardLine {
            start: words[0].start,
            end: words[words.len() - 1].end,
            words,
            ..line(session, at, 0.0, 0.0)
        }
    }

    fn heard(end: f64, turns: Vec<Turn>) -> Update {
        Update {
            end,
            turns,
            voices: vec![(0, vec![1.0, 0.0]), (1, vec![0.0, 1.0])],
            merged: Vec::new(),
            moved: Vec::new(),
        }
    }

    #[test]
    fn a_live_line_is_cut_again_as_the_turns_change() {
        let mut live = Live::default();
        let words = spoken(0.0, &["Tudo", "bem", "com", "voce", "sim", "e", "voce"]);
        let said = timed("n1", 100.0, words);
        assert_eq!(live.line(said.clone()), Known::default());
        let turns = vec![turn(0.0, 4.2, 0), turn(4.2, 7.0, 1)];
        let known = live.update(heard(8.0, turns));
        assert_eq!(known.lines.len(), 1);
        assert_eq!(firsts(&known.lines[0].1), [(0, 0), (4, 1)]);
        assert_eq!(known.voices.keys().copied().collect::<Vec<_>>(), [0, 1]);

        // The change moves: the line is told again, cut elsewhere.
        let known = live.update(heard(9.0, vec![turn(4.2, 5.1, 0)]));
        assert_eq!(known.lines.len(), 1);
        assert_eq!(firsts(&known.lines[0].1), [(0, 0), (5, 1)]);

        // The two speakers are found to be one: the parts rejoin.
        let mut joined = heard(10.0, Vec::new());
        joined.merged = vec![(1, 0)];
        let known = live.update(joined);
        assert_eq!(firsts(&known.lines[0].1), [(0, 0)]);

        // Nothing moved: nothing is told.
        assert!(live.update(heard(11.0, Vec::new())).lines.is_empty());
    }

    #[test]
    fn lines_are_followed_only_while_their_turns_are_kept() {
        let mut live = Live::default();
        live.line(timed("n1", 1.0, spoken(0.0, &["a", "b"])));
        live.update(heard(2.0, vec![turn(0.0, 2.0, 0)]));
        assert_eq!(live.settled.len(), 1);
        live.update(heard(RECENT + 10.0, vec![turn(RECENT, RECENT + 10.0, 0)]));
        assert_eq!(live.settled.len(), 1, "kept for a re-cluster to move");
        live.update(heard(REACH + 10.0, vec![turn(REACH, REACH + 10.0, 0)]));
        assert!(live.settled.is_empty());
        assert!(live.line(line("n1", 9.0, REACH, REACH + 1.0)).lines.len() == 1);
        assert_eq!(live.settled.len(), 1, "a line without words too");
    }

    fn spans(moved: &[Moved]) -> Vec<(f64, f64, usize, usize)> {
        moved
            .iter()
            .map(|m| (m.start, m.end, m.from, m.to))
            .collect()
    }

    #[test]
    fn a_recluster_moves_a_stretch_heard_wrong_to_the_speaker_it_belongs_to() {
        let mut speakers = Speakers::new(Clustering::default());
        let (a, early, b): (&[f32], &[f32], &[f32]) = (&[1.0, 0.0], &[0.6, 0.8], &[0.0, 1.0]);
        speakers.hear(&windows(0.0, &[a; 8]));
        // Only the first voice is known, so the early sound of the second joins it.
        let heard = speakers.hear(&windows(8.0, &[early; 2]));
        assert_eq!(rounded(&heard.turns), [(8.0, 10.0, 0)]);
        let heard = speakers.hear(&windows(10.0, &[b; 8]));
        assert_eq!(rounded(&heard.turns), [(10.0, 18.0, 1)]);

        let update = speakers.recluster().unwrap();
        assert_eq!(spans(&update.moved), [(8.0, 10.0, 0, 1)]);
        assert!(update.merged.is_empty() && update.turns.is_empty());
        assert_eq!(update.end, 18.0);
        let told: Vec<usize> = update.voices.iter().map(|v| v.0).collect();
        assert_eq!(told, [0, 1]);
        let second = normalized(vec![2.0 * 0.6, 8.0 + 2.0 * 0.8]);
        assert!(dot(&update.voices[1].1, &second) > 0.9999);
        assert_eq!(speakers.recluster(), None, "nothing left to move");
    }

    #[test]
    fn a_recluster_gives_a_stretch_apart_from_the_rest_a_new_speaker() {
        let mut speakers = Speakers::new(Clustering {
            recluster: 0.3,
            ..Clustering::default()
        });
        let (p, q): (&[f32], &[f32]) = (&[1.0, 0.0], &[0.5, 0.866]);
        for (start, voice) in [(0.0, p), (6.0, q), (12.0, p)] {
            let heard = speakers.hear(&windows(start, &[voice; 6]));
            assert_eq!(heard.turns[0].speaker, 0, "close enough to join online");
        }
        let update = speakers.recluster().unwrap();
        assert_eq!(spans(&update.moved), [(6.0, 12.0, 0, 1)]);
        // The new speaker is heard on as their own.
        let next = speakers.hear(&windows(18.0, &[q; 2]));
        assert_eq!(next.turns[0].speaker, 1);
    }

    #[test]
    fn a_speaker_whose_every_stretch_moved_to_one_other_is_joined_to_them() {
        let mut speakers = Speakers::new(Clustering {
            recluster: 1.5,
            ..Clustering::default()
        });
        let (a, b): (&[f32], &[f32]) = (&[1.0, 0.0], &[0.0, 1.0]);
        speakers.hear(&windows(0.0, &[a; 8]));
        speakers.hear(&windows(8.0, &[b; 8]));
        let update = speakers.recluster().unwrap();
        assert_eq!(update.merged, [(1, 0)]);
        assert!(update.moved.is_empty());
        let told: Vec<usize> = update.voices.iter().map(|v| v.0).collect();
        assert_eq!(told, [0]);
        assert_eq!(speakers.live().count(), 1);
        let next = speakers.hear(&windows(16.0, &[b; 2]));
        assert_eq!(next.turns[0].speaker, 0);
    }

    #[test]
    fn a_recluster_keeps_a_short_speaker_who_spoke_and_folds_one_who_never_settled() {
        let unit = |hot: usize, small: f32| {
            let mut voice = vec![0.1 * small; 4];
            voice[hot] = 1.0;
            voice
        };
        let (a, b, c) = (unit(0, 0.0), unit(1, 0.0), unit(2, 0.0));
        let mut speakers = Speakers::new(Clustering::default());
        let mut at = 0.0;
        let mut hear = |speakers: &mut Speakers, voice: &Vec<f32>, windows: usize| {
            let heard = speakers.hear(&self::windows(at, &vec![voice.as_slice(); windows]));
            at += windows as f64;
            heard
        };
        hear(&mut speakers, &a, 400);
        hear(&mut speakers, &b, 300);
        // 20 of 720 windows, 2.8%: a person who spoke a little.
        hear(&mut speakers, &c, 20);
        assert_eq!(speakers.recluster(), None);
        assert_eq!(speakers.live().count(), 3);
        // 5 of 725, 0.7%, far from every voice: born in the last minutes.
        let d = normalized(vec![0.1, 0.1, 0.25, 0.96]);
        let born = hear(&mut speakers, &d, 5);
        assert_eq!(born.turns[0].speaker, 3);
        assert!(born.merged.is_empty(), "not settled online");
        let update = speakers.recluster().unwrap();
        assert_eq!(update.merged, [(3, 2)]);
        assert!(update.moved.is_empty());
        assert_eq!(speakers.live().count(), 3);
    }

    #[test]
    fn stretches_are_kept_within_the_reach_and_a_recluster_needs_two() {
        let mut speakers = Speakers::new(Clustering {
            every: 4,
            ..Clustering::default()
        });
        let (a, b): (&[f32], &[f32]) = (&[1.0, 0.0], &[0.0, 1.0]);
        speakers.hear(&windows(0.0, &[a; 3]));
        assert!(!speakers.due());
        assert!(speakers.snapshot().is_none(), "one stretch");
        speakers.hear(&windows(3.0, &[b; 2]));
        assert!(!speakers.due(), "counted since the last");
        speakers.hear(&windows(5.0, &[b; 2]));
        assert!(speakers.due());
        assert!(speakers.snapshot().is_some());
        assert!(!speakers.due());
        speakers.hear(&windows(REACH + 100.0, &[b; 2]));
        assert_eq!(speakers.stretches.len(), 1);
    }

    #[test]
    fn spans_moved_cut_the_turns_they_cross_and_turns_of_one_speaker_rejoin() {
        let mut turns = vec![turn(0.0, 10.0, 0), turn(10.0, 20.0, 1)];
        let span = |start, end, from, to| Moved {
            start,
            end,
            from,
            to,
        };
        move_spans(&mut turns, &[span(2.0, 5.0, 0, 2), span(4.0, 12.0, 1, 3)]);
        assert_eq!(
            turns,
            [
                turn(0.0, 2.0, 0),
                turn(2.0, 5.0, 2),
                turn(5.0, 10.0, 0),
                turn(10.0, 12.0, 3),
                turn(12.0, 20.0, 1)
            ]
        );
        move_spans(&mut turns, &[span(0.0, 20.0, 2, 0), span(10.0, 12.0, 3, 1)]);
        assert_eq!(turns, [turn(0.0, 10.0, 0), turn(10.0, 20.0, 1)]);
    }

    #[test]
    fn a_recording_follows_the_stretches_moved_and_the_speakers_joined_before() {
        let mut recording = Recording::default();
        recording.apply(Update {
            end: 6.0,
            turns: vec![turn(0.0, 2.0, 3), turn(2.0, 4.0, 5), turn(4.0, 6.0, 7)],
            voices: vec![
                (3, vec![1.0, 0.0]),
                (5, vec![0.0, 1.0]),
                (7, vec![0.6, 0.8]),
            ],
            ..Update::default()
        });
        recording.apply(Update {
            end: 6.0,
            merged: vec![(7, 3)],
            ..Update::default()
        });
        // The span of speaker 3 now includes what 7 said.
        recording.apply(Update {
            end: 6.0,
            moved: vec![Moved {
                start: 4.0,
                end: 6.0,
                from: 3,
                to: 5,
            }],
            ..Update::default()
        });
        assert_eq!(recording.turns(), [turn(0.0, 2.0, 3), turn(2.0, 6.0, 5)]);
        assert_eq!(
            recording.finish(),
            Diarization {
                turns: vec![turn(0.0, 2.0, 0), turn(2.0, 6.0, 1)],
                voices: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            }
        );
    }

    #[test]
    fn a_moved_span_tells_a_line_older_than_recent_again_cut_where_the_speaker_changes() {
        let mut live = Live::default();
        live.line(timed("n1", 10.0, spoken(10.0, &["a", "b", "c", "d"])));
        let known = live.update(heard(20.0, vec![turn(0.0, 20.0, 0)]));
        assert_eq!(firsts(&known.lines[0].1), [(0, 0)]);
        // Long after: the line is no longer followed, but for a move.
        let known = live.update(heard(1310.0, vec![turn(1300.0, 1310.0, 0)]));
        assert!(known.lines.is_empty());
        let mut moved = heard(1310.0, Vec::new());
        moved.moved = vec![Moved {
            start: 12.0,
            end: 14.0,
            from: 0,
            to: 1,
        }];
        let known = live.update(moved);
        assert_eq!(known.lines.len(), 1);
        assert_eq!(firsts(&known.lines[0].1), [(0, 0), (2, 1)]);
        assert_eq!(known.voices.keys().copied().collect::<Vec<_>>(), [0, 1]);
        assert!(live.update(heard(1311.0, Vec::new())).lines.is_empty());
    }
}
