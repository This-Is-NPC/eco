//! Cut a stream of VAD-scored frames into speech segments.

use std::collections::VecDeque;

use crate::ports::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

const FRAME_MS: u32 = FRAME_SAMPLES as u32 * 1000 / SAMPLE_RATE;

#[derive(Debug, Clone, Copy)]
pub struct SegmenterConfig {
    pub threshold: f32,
    /// Below threshold - hysteresis a frame counts as silence; between the two it keeps the state.
    pub hysteresis: f32,
    pub min_silence_ms: u32,
    pub min_speech_ms: u32,
    pub max_segment_ms: u32,
    pub pad_ms: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            hysteresis: 0.15,
            min_silence_ms: 480,
            min_speech_ms: 256,
            max_segment_ms: 15_000,
            pad_ms: 96,
        }
    }
}

/// A stretch of speech and where it starts in the stream, in samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub pcm: Vec<i16>,
    pub start: usize,
}

pub struct Segmenter {
    config: SegmenterConfig,
    /// Frames pushed so far.
    pushed: usize,
    min_silence: usize,
    min_speech: usize,
    max_frames: usize,
    pad: usize,
    lead: VecDeque<Frame>,
    frames: Vec<Frame>,
    speech: usize,
    silence: usize,
}

impl Segmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        let frames = |ms: u32| (ms / FRAME_MS) as usize;
        Self {
            config,
            pushed: 0,
            min_silence: frames(config.min_silence_ms),
            min_speech: frames(config.min_speech_ms),
            max_frames: frames(config.max_segment_ms),
            pad: frames(config.pad_ms),
            lead: VecDeque::new(),
            frames: Vec::new(),
            speech: 0,
            silence: 0,
        }
    }

    /// Add one frame; return a finished segment when one closes.
    pub fn push(&mut self, frame: Frame, probability: f32) -> Option<Segment> {
        self.pushed += 1;
        if self.frames.is_empty() {
            if probability < self.config.threshold {
                self.lead.push_back(frame);
                while self.lead.len() > self.pad {
                    self.lead.pop_front();
                }
                return None;
            }
            self.frames = self.lead.drain(..).collect();
            self.frames.push(frame);
            self.speech = 1;
            self.silence = 0;
            return None;
        }
        self.frames.push(frame);
        if probability >= self.config.threshold {
            self.speech += 1;
            self.silence = 0;
        } else if probability < self.config.threshold - self.config.hysteresis {
            self.silence += 1;
        }
        if self.silence >= self.min_silence {
            return self.close(self.silence.saturating_sub(self.pad));
        }
        if self.frames.len() >= self.max_frames {
            return self.close(0);
        }
        None
    }

    /// Close the pending segment at the end of the stream.
    pub fn flush(&mut self) -> Option<Segment> {
        if self.frames.is_empty() {
            None
        } else {
            self.close(self.silence.saturating_sub(self.pad))
        }
    }

    fn close(&mut self, trailing: usize) -> Option<Segment> {
        let mut frames = std::mem::take(&mut self.frames);
        let start = (self.pushed - frames.len()) * FRAME_SAMPLES;
        frames.truncate(frames.len() - trailing);
        let speech = std::mem::take(&mut self.speech);
        self.silence = 0;
        (speech >= self.min_speech).then(|| Segment {
            pcm: frames.concat(),
            start,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: i16) -> Frame {
        vec![value; FRAME_SAMPLES]
    }

    fn feed(segmenter: &mut Segmenter, probabilities: &[f32]) -> Vec<Segment> {
        probabilities
            .iter()
            .enumerate()
            .filter_map(|(index, &probability)| segmenter.push(frame(index as i16), probability))
            .collect()
    }

    fn frames(ms: u32) -> usize {
        (ms / FRAME_MS) as usize
    }

    #[test]
    fn closes_after_silence_with_padding() {
        let config = SegmenterConfig::default();
        let (pad, silence) = (frames(config.pad_ms), frames(config.min_silence_ms));
        let mut probabilities = vec![0.0; 5];
        probabilities.extend([0.9; 20]);
        probabilities.extend(vec![0.0; silence]);
        let segments = feed(&mut Segmenter::new(config), &probabilities);
        let [segment] = segments.as_slice() else {
            panic!("one segment")
        };
        assert_eq!(segment.pcm[0] as usize, 5 - pad);
        assert_eq!(segment.start, (5 - pad) * FRAME_SAMPLES);
        assert_eq!(*segment.pcm.last().unwrap() as usize, 5 + 20 - 1 + pad);
        assert_eq!(segment.pcm.len(), (pad + 20 + pad) * FRAME_SAMPLES);
    }

    #[test]
    fn drops_blips_shorter_than_min_speech() {
        let silence = frames(SegmenterConfig::default().min_silence_ms);
        let mut probabilities = vec![0.9; 2];
        probabilities.extend(vec![0.0; silence]);
        assert!(
            feed(
                &mut Segmenter::new(SegmenterConfig::default()),
                &probabilities
            )
            .is_empty()
        );
    }

    #[test]
    fn uncertain_frames_do_not_count_as_silence() {
        let silence = frames(SegmenterConfig::default().min_silence_ms);
        let mut probabilities = vec![0.9; 20];
        probabilities.extend(vec![0.4; silence * 3]);
        assert!(
            feed(
                &mut Segmenter::new(SegmenterConfig::default()),
                &probabilities
            )
            .is_empty()
        );
    }

    #[test]
    fn splits_long_monologues() {
        let config = SegmenterConfig {
            max_segment_ms: 3200,
            ..SegmenterConfig::default()
        };
        let segments = feed(&mut Segmenter::new(config), &[0.9; 250]);
        assert_eq!(segments.len(), 2);
        assert!(segments.iter().all(|s| s.pcm.len() == 100 * FRAME_SAMPLES));
        assert_eq!(segments[1].start, 100 * FRAME_SAMPLES);
    }

    #[test]
    fn flush_returns_pending_speech() {
        let mut segmenter = Segmenter::new(SegmenterConfig::default());
        feed(&mut segmenter, &[0.9; 20]);
        assert_eq!(
            segmenter.flush().map(|s| s.pcm.len()),
            Some(20 * FRAME_SAMPLES)
        );
        assert!(segmenter.flush().is_none());
    }
}
