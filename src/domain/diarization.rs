//! Who spoke when: each speech region is embedded in short windows, and each
//! window goes to the speaker whose voice is nearest, or starts a new one, as
//! the recording is heard. Only the speakers' running voices are kept, so a
//! region costs the same at the third hour as at the first minute.

use std::collections::{BTreeMap, BTreeSet};

use rayon::prelude::*;

use crate::ports::{EmbeddingError, SAMPLE_RATE, SpeakerEmbedder};

const WINDOW: usize = SAMPLE_RATE as usize * 3 / 2;
const HOP: usize = WINDOW / 2;
/// Regions shorter than this carry too little voice to embed; they take the
/// speaker heard last.
const MIN_CLIP: usize = SAMPLE_RATE as usize / 2;
/// At most this many speakers at once: past it, a window joins the nearest.
const MAX_SPEAKERS: usize = 32;
/// A small speaker is folded into a larger one only once this many windows
/// were heard since they first spoke (about 90 s of speech).
const SETTLE: usize = 120;
/// Seconds of turns a live input keeps behind what the diarizer has heard.
const RECENT: f64 = 300.0;
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
}

impl Default for Clustering {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            merge: 0.3,
            min_share: 0.02,
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

/// What one region told: its turns, the voice of each speaker it changed, and
/// the speakers found to be one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Update {
    /// Where the region ends, in seconds: the recording is heard up to here.
    pub end: f64,
    pub turns: Vec<Turn>,
    /// Unit length.
    pub voices: Vec<(usize, Vec<f32>)>,
    /// In order, as (gone, kept): `gone` is never told again.
    pub merged: Vec<(usize, usize)>,
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
}

impl Speakers {
    pub fn new(clustering: Clustering) -> Self {
        Self {
            clustering,
            voices: Vec::new(),
            windows: 0,
            last: None,
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
    fn join(&mut self, gone: usize, kept: usize) {
        let gone = self.voices[gone].take().expect("a live speaker");
        let voice = self.voices[kept].as_mut().expect("a live speaker");
        voice
            .sum
            .iter_mut()
            .zip(&gone.sum)
            .for_each(|(s, x)| *s += x);
        voice.windows += gone.windows;
        voice.since = voice.since.min(gone.since);
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
            (voice.windows as f32) < minimum && self.windows - voice.since >= SETTLE
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
}

/// A whole recording's updates, gathered into who spoke when.
#[derive(Default)]
pub struct Recording {
    turns: Vec<Turn>,
    voices: BTreeMap<usize, Vec<f32>>,
    /// Each speaker joined, and who they joined.
    into: BTreeMap<usize, usize>,
}

impl Recording {
    pub fn apply(&mut self, update: Update) {
        self.turns.extend(update.turns);
        for (gone, kept) in update.merged {
            self.into.insert(gone, kept);
            self.voices.remove(&gone);
        }
        self.voices.extend(update.voices);
    }

    fn current(&self, mut speaker: usize) -> usize {
        while let Some(&kept) = self.into.get(&speaker) {
            speaker = kept;
        }
        speaker
    }

    /// The turns of the whole recording, in time order, and the speakers' voices.
    pub fn finish(self) -> Diarization {
        let mut spans: Vec<(f64, f64, usize)> = self
            .turns
            .iter()
            .map(|t| (t.start, t.end, self.current(t.speaker)))
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

/// Who spoke when in `regions`, heard in order.
pub fn diarize(regions: &[Region], clustering: Clustering) -> Diarization {
    let mut speakers = Speakers::new(clustering);
    let mut recording = Recording::default();
    for region in regions {
        recording.apply(speakers.hear(region));
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
}

/// What a session learns of a live input's voices: its lines, by time, with
/// the speaker who said each; speakers found to be one, as (gone, kept); and
/// the latest voice of each speaker involved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Said {
    pub lines: Vec<(f64, usize)>,
    pub merged: Vec<(usize, usize)>,
    pub voices: BTreeMap<usize, Vec<f32>>,
}

/// What became known of a live input's voices, for every session it records for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub lines: Vec<(HeardLine, usize)>,
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
                .map(|(line, speaker)| (line.at, *speaker))
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
        self.turns.extend(update.turns);
        self.horizon = self.horizon.max(update.end);
        for (speaker, voice) in update.voices {
            known.voices.insert(speaker, voice.clone());
            self.voices.insert(speaker, voice);
        }
        let horizon = self.horizon;
        let (ready, waiting): (Vec<HeardLine>, Vec<HeardLine>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|line| line.end <= horizon);
        self.waiting = waiting;
        for line in ready {
            self.said(line, &mut known);
        }
        self.turns.retain(|t| t.end >= horizon - RECENT);
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

    fn said(&self, line: HeardLine, known: &mut Known) {
        if let Some(speaker) = speaker_of(&self.turns, line.start, line.end) {
            if let Some(voice) = self.voices.get(&speaker) {
                known.voices.entry(speaker).or_insert_with(|| voice.clone());
            }
            known.lines.push((line, speaker));
        }
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
        let windows = voices.iter().enumerate().map(|(index, voice)| Window {
            start: start + index as f64,
            end: start + index as f64 + 1.0,
            embedding: normalized(voice.to_vec()),
        });
        Region {
            start,
            end: start + voices.len() as f64,
            windows: windows.collect(),
        }
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
        let more = vec![a; SETTLE + 5];
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
        });
        recording.apply(Update {
            end: 4.0,
            turns: vec![turn(2.5, 4.0, 7)],
            voices: vec![(7, vec![0.6, 0.8])],
            merged: vec![(3, 5)],
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

    fn line(session: &str, at: f64, start: f64, end: f64) -> HeardLine {
        HeardLine {
            session: session.into(),
            at,
            start,
            end,
        }
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
        });
        assert_eq!(known.lines, [(line("n1", 100.0, 0.0, 2.0), 0)]);
        assert_eq!(known.voices[&0], [1.0, 0.0]);

        // Heard after: told at once, with its speaker's voice.
        let known = live.line(line("n2", 101.0, 1.0, 3.0));
        assert_eq!(known.lines, [(line("n2", 101.0, 1.0, 3.0), 0)]);
        assert_eq!(known.voices[&0], [1.0, 0.0]);
        assert_eq!(
            known.of("n2"),
            Said {
                lines: vec![(101.0, 0)],
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
        });
        assert_eq!(known.merged, [(0, 1)]);
        assert!(known.lines.is_empty());
        let known = live.line(line("n1", 102.0, 0.0, 1.0));
        assert_eq!(known.lines[0].1, 1);
        assert_eq!(known.voices.keys().copied().collect::<Vec<_>>(), [1]);

        // The diarizer stops: the lines waiting go to the turns it told, and
        // later lines at once.
        assert!(live.line(line("n1", 103.0, 6.5, 7.0)).lines.is_empty());
        let known = live.finish();
        assert_eq!(known.lines, [(line("n1", 103.0, 6.5, 7.0), 1)]);
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
        });
        assert_eq!(known.lines.len(), WAITING);
        assert!(known.lines.iter().all(|(_, speaker)| *speaker == 0));
        assert!(known.voices.is_empty());
        // Turns further behind than `RECENT` are gone.
        assert_eq!(live.turns, [turn(900.0, 1000.0, 1)]);
        assert!(live.waiting.is_empty());
    }
}
