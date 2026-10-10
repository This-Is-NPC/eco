//! One audio source, segmented and transcribed — or only measured.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use futures::{Stream, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep};

use crate::domain::loudness::loudness;
use crate::domain::segmenter::{Segment, Segmenter, SegmenterConfig};
use crate::ports::{
    AudioError, AudioSource, FRAME_SAMPLES, Frame, Heard, Phrase, SAMPLE_RATE, SpeechToText,
    StreamingSpeechToText, Transcript, TranscriptionError,
};

/// How a channel's speech becomes text: segment by segment, or as one stream
/// the provider segments itself.
#[derive(Clone, Copy)]
pub enum Transcriber<'a> {
    Segments(&'a dyn SpeechToText),
    Stream(&'a dyn StreamingSpeechToText),
}

/// Segments sent to a segment-by-segment STT before the first is answered.
const SEGMENTS_AT_ONCE: usize = 4;

/// The waits before each new attempt at a segment that failed: four attempts
/// in all, so a blip of a few seconds loses nothing.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// No attempt at a segment starts later than this after its first, so a slow
/// provider does not hold the lines behind it for long.
const RETRY_WITHIN: Duration = Duration::from_secs(15);

fn seconds(samples: usize) -> f64 {
    samples as f64 / f64::from(SAMPLE_RATE)
}

/// One segment's transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub who: String,
    /// Timed in seconds from the source's first frame.
    pub phrases: Vec<Phrase>,
    /// From the end of speech (segment closed) to the transcript arriving.
    pub latency: Duration,
}

impl Utterance {
    /// The phrases as one line.
    pub fn text(&self) -> String {
        let texts: Vec<&str> = self.phrases.iter().map(|p| p.text.as_str()).collect();
        texts.join(" ")
    }

    /// Where the speech ends in the source, in seconds.
    pub fn end(&self) -> f64 {
        self.phrases.last().map_or(0.0, |p| p.end)
    }
}

/// A speech probability for a frame.
pub type SpeechProbability<'a> = &'a mut (dyn FnMut(&[i16]) -> Result<f32, AudioError> + Send);

/// What a transcriber reports as it goes.
pub struct Listeners<'a> {
    /// A transcript, in the order the speech was heard.
    pub utterance: &'a (dyn Fn(Utterance) + Sync),
    /// A segment that could not be transcribed: who, and why.
    pub failure: &'a (dyn Fn(&str, &str) + Sync),
    /// The words of the phrase being spoken so far, from a streaming STT.
    pub partial: &'a (dyn Fn(String) + Sync),
    /// The provider's id for the request a streaming STT bills from now on.
    pub request: &'a (dyn Fn(String) + Sync),
    /// The provider's id for the request a segment-by-segment STT billed one
    /// segment under, and the segment's seconds.
    pub billed: &'a (dyn Fn(String, f64) + Sync),
}

/// What an input's capture hands the transcribers on it: each frame, numbered
/// from the capture's first, and each stretch of speech once the VAD closes it.
#[derive(Debug, Clone)]
pub enum Captured {
    Frame(usize, Frame),
    Segment(Segment, Instant),
}

/// The captured audio a transcriber reads, from when it started reading.
pub type CapturedStream<'a> = futures::stream::BoxStream<'a, Captured>;

/// Cut one audio source into stretches of speech until it ends, reporting each
/// frame's loudness and whether it is speech; `heard` sees every frame.
pub async fn segment_channel(
    source: &mut dyn AudioSource,
    speech_probability: SpeechProbability<'_>,
    signal: &(dyn Fn(f64, bool) + Sync),
    config: SegmenterConfig,
    segment: &mut (dyn FnMut(Segment) + Send),
    heard: &mut (dyn FnMut(&Frame) + Send),
) -> Result<(), AudioError> {
    let mut segmenter = Segmenter::new(config);
    let mut frames = source.frames();
    while let Some(frame) = frames.next().await {
        let frame = frame?;
        heard(&frame);
        let probability = speech_probability(&frame)?;
        signal(loudness(&frame), probability >= config.threshold);
        if let Some(closed) = segmenter.push(frame, probability) {
            segment(closed);
        }
    }
    if let Some(closed) = segmenter.flush() {
        segment(closed);
    }
    Ok(())
}

/// Capture one audio source until it ends: report each frame's signal, and hand
/// every frame and every closed stretch of speech to `captured`.
pub async fn capture_channel(
    source: &mut dyn AudioSource,
    speech_probability: SpeechProbability<'_>,
    signal: &(dyn Fn(f64, bool) + Sync),
    config: SegmenterConfig,
    captured: &(dyn Fn(Captured) + Sync),
) -> Result<(), AudioError> {
    let mut index = 0;
    segment_channel(
        source,
        speech_probability,
        signal,
        config,
        &mut |segment| captured(Captured::Segment(segment, Instant::now())),
        &mut |frame| {
            captured(Captured::Frame(index, frame.clone()));
            index += 1;
        },
    )
    .await
}

/// Capture and transcribe one audio source on its own, emitting its utterances
/// in order until it ends; `segment` sees each stretch of speech as it closes.
///
/// Reading and transcribing run side by side, so a slow transcription never holds
/// up capture; dropping this future stops both, and the source with them.
#[allow(clippy::too_many_arguments)]
pub async fn transcribe_channel(
    who: &str,
    source: &mut dyn AudioSource,
    speech_probability: SpeechProbability<'_>,
    stt: Transcriber<'_>,
    listeners: &Listeners<'_>,
    signal: &(dyn Fn(f64, bool) + Sync),
    segment: &(dyn Fn(&Segment) + Sync),
    config: SegmenterConfig,
) -> Result<(), AudioError> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Captured>();
    // The sender goes with the reading, so transcription ends when the source does.
    let read = async move {
        let captured = |captured: Captured| {
            if let Captured::Segment(closed, _) = &captured {
                segment(closed);
            }
            let _ = sender.send(captured);
        };
        capture_channel(source, speech_probability, signal, config, &captured).await
    };
    let captured = futures::stream::poll_fn(move |context| receiver.poll_recv(context)).boxed();
    let (read, ()) = tokio::join!(read, transcribe(who, captured, stt, listeners));
    read
}

/// Transcribe the audio an input's capture hands over, emitting its utterances
/// in order, until the capture ends.
pub async fn transcribe(
    who: &str,
    captured: CapturedStream<'_>,
    stt: Transcriber<'_>,
    listeners: &Listeners<'_>,
) {
    match stt {
        Transcriber::Segments(stt) => by_segment(who, captured, stt, listeners).await,
        Transcriber::Stream(stt) => streamed(who, captured, stt, listeners).await,
    }
}

/// Send one segment to the STT, trying again after each of `RETRY_DELAYS` while
/// it fails, within `RETRY_WITHIN`; the clip stays in memory. Every error is
/// tried again: a `TranscriptionError` is only the provider's text, which does
/// not tell a refused key from a dropped connection.
async fn transcribe_segment(
    stt: &dyn SpeechToText,
    pcm: &[i16],
) -> Result<Transcript, TranscriptionError> {
    let first = Instant::now();
    let mut delays = RETRY_DELAYS.iter();
    loop {
        let error = match stt.transcribe(pcm).await {
            Ok(transcript) => return Ok(transcript),
            Err(error) => error,
        };
        match delays.next() {
            Some(&delay) if first.elapsed() + delay <= RETRY_WITHIN => sleep(delay).await,
            _ => return Err(error),
        }
    }
}

/// Each segment the VAD closes goes to the STT on its own, a few at a time, their
/// lines kept in order: a segment being tried again holds back the lines after it.
async fn by_segment(
    who: &str,
    captured: CapturedStream<'_>,
    stt: &dyn SpeechToText,
    listeners: &Listeners<'_>,
) {
    let segments = captured.filter_map(|captured| {
        std::future::ready(match captured {
            Captured::Segment(segment, closed) => Some((segment, closed)),
            Captured::Frame(..) => None,
        })
    });
    let mut transcribed = segments
        .map(|(segment, ended)| async move {
            let result = transcribe_segment(stt, &segment.pcm).await;
            (segment, ended, result)
        })
        .buffered(SEGMENTS_AT_ONCE);
    while let Some((segment, ended, result)) = transcribed.next().await {
        if let Ok(Transcript {
            request: Some(request),
            ..
        }) = &result
        {
            (listeners.billed)(request.clone(), seconds(segment.pcm.len()));
        }
        match result.map(|transcript| transcript.phrases) {
            Ok(phrases) if !phrases.is_empty() => {
                let start = seconds(segment.start);
                let phrases = phrases
                    .into_iter()
                    .map(|p| Phrase {
                        start: start + p.start,
                        end: start + p.end,
                        text: p.text,
                    })
                    .collect();
                (listeners.utterance)(Utterance {
                    who: who.into(),
                    phrases,
                    latency: ended.elapsed(),
                });
            }
            Ok(_) => {}
            Err(error) => (listeners.failure)(who, &error.0),
        }
    }
}

/// The wait before a stream is opened again after a failure; each failure
/// that follows doubles it, up to the longest.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(30);

/// What a stream that ends before its capture, without an error, reports.
const CLOSED: &str = "the provider closed the stream";

/// Every frame goes to the provider, which says where speech ends. While the
/// capture runs, a connection that fails or ends is opened again after a wait
/// that backs off, and its phrases are timed from the first frame it received.
/// The first failure of an outage is reported; the rest of it is not.
async fn streamed(
    who: &str,
    captured: CapturedStream<'_>,
    stt: &dyn StreamingSpeechToText,
    listeners: &Listeners<'_>,
) {
    // When the capture's first frame was heard, for the latency of each phrase.
    let origin: OnceLock<Instant> = OnceLock::new();
    // Fused: a connection may poll it again after the capture ended, and so
    // does the wait between connections.
    let mut frames = captured
        .filter_map(|captured| {
            std::future::ready(match captured {
                Captured::Frame(index, frame) => Some((index, frame)),
                Captured::Segment(..) => None,
            })
        })
        .inspect(|(index, _)| {
            origin.get_or_init(|| {
                Instant::now() - Duration::from_secs_f64(seconds(index * FRAME_SAMPLES))
            });
        })
        .fuse();
    // Frames heard while no connection was open, sent first by the next one.
    let mut held = VecDeque::new();
    // The number of the first frame the connection received, and whether the
    // capture ended.
    let first = AtomicUsize::new(usize::MAX);
    let ended = AtomicBool::new(false);
    let mut wait = FIRST_WAIT;
    // Whether the outage under way was reported, and whether the capture ended
    // with frames still held.
    let (mut reported, mut finishing) = (false, false);
    loop {
        first.store(usize::MAX, Ordering::Relaxed);
        let opened = Instant::now();
        let fed = futures::stream::poll_fn(|context| {
            let polled = match held.pop_front() {
                Some(frame) => Poll::Ready(Some(frame)),
                None => frames.poll_next_unpin(context),
            };
            match &polled {
                Poll::Ready(Some((index, _))) => {
                    let _ = first.compare_exchange(
                        usize::MAX,
                        *index,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                Poll::Ready(None) => ended.store(true, Ordering::Relaxed),
                Poll::Pending => {}
            }
            polled.map(|next| next.map(|(_, frame)| frame))
        });
        let mut heard = stt.transcribe(fed.boxed());
        let (mut failure, mut spoke) = (None, false);
        while let Some(next) = heard.next().await {
            match next {
                Ok(Heard::Partial(words)) => {
                    spoke = true;
                    (listeners.partial)(words);
                }
                Ok(Heard::Request(id)) => (listeners.request)(id),
                Ok(Heard::Phrase(phrase)) => {
                    spoke = true;
                    let offset = seconds(first.load(Ordering::Relaxed) * FRAME_SAMPLES);
                    let (start, end) = (offset + phrase.start, offset + phrase.end);
                    let spoken = Duration::from_secs_f64(end.max(0.0));
                    let since = origin.get().map_or(Duration::ZERO, Instant::elapsed);
                    (listeners.utterance)(Utterance {
                        who: who.into(),
                        phrases: vec![Phrase {
                            start,
                            end,
                            text: phrase.text,
                        }],
                        latency: since.saturating_sub(spoken),
                    });
                }
                Err(error) => failure = Some(error.0),
            }
        }
        drop(heard);
        let took = first.load(Ordering::Relaxed) != usize::MAX;
        if took && (spoke || opened.elapsed() >= LONGEST_WAIT) {
            wait = FIRST_WAIT;
            reported = false;
        }
        let over = ended.load(Ordering::Relaxed);
        if !reported && (failure.is_some() || !over) {
            (listeners.failure)(who, failure.as_deref().unwrap_or(CLOSED));
            reported = true;
        }
        let running = !over && hold(&mut frames, &mut held, wait).await;
        // Once the capture ended, the frames still held get one connection more.
        if !running && (held.is_empty() || finishing) {
            break;
        }
        finishing = !running;
        wait = (wait * 2).min(LONGEST_WAIT);
    }
}

/// Wait `wait` before a stream is opened again, holding the frames heard
/// meanwhile; false as soon as the capture ends.
async fn hold(
    frames: &mut (impl Stream<Item = (usize, Frame)> + Unpin),
    held: &mut VecDeque<(usize, Frame)>,
    wait: Duration,
) -> bool {
    let waited = sleep(wait);
    tokio::pin!(waited);
    loop {
        tokio::select! {
            () = &mut waited => return true,
            next = frames.next() => match next {
                Some(frame) => held.push_back(frame),
                None => return false,
            },
        }
    }
}

/// Report one source's signal without transcribing or keeping anything.
pub async fn monitor_channel(
    source: &mut dyn AudioSource,
    speech_probability: SpeechProbability<'_>,
    signal: &(dyn Fn(f64, bool) + Sync),
    threshold: f32,
) -> Result<(), AudioError> {
    let mut frames = source.frames();
    while let Some(frame) = frames.next().await {
        let frame = frame?;
        signal(loudness(&frame), speech_probability(&frame)? >= threshold);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use futures::future::BoxFuture;
    use futures::stream::{self, BoxStream};

    use super::*;

    struct Recorded(Vec<i16>);

    impl AudioSource for Recorded {
        fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
            stream::iter(self.0.chunks(FRAME_SAMPLES).map(|c| Ok(c.to_vec()))).boxed()
        }
    }

    /// An STT that says two phrases per segment, numbered by its call, and
    /// nothing for a silent one. A segment whose first sample is a key of
    /// `failing` fails that many times first, each failure after `slow`.
    #[derive(Default)]
    struct FakeStt {
        calls: Mutex<Vec<usize>>,
        failing: Mutex<HashMap<i16, usize>>,
        slow: Duration,
    }

    impl SpeechToText for FakeStt {
        fn transcribe<'a>(
            &'a self,
            pcm: &'a [i16],
        ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
            Box::pin(async move {
                let calls = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(pcm.len());
                    calls.len()
                };
                let fails = self
                    .failing
                    .lock()
                    .unwrap()
                    .get_mut(&pcm[0])
                    .filter(|left| **left > 0)
                    .map(|left| *left -= 1)
                    .is_some();
                if fails {
                    sleep(self.slow).await;
                    return Err(TranscriptionError("down".into()));
                }
                if pcm.iter().all(|&sample| sample == 0) {
                    return Ok(Transcript {
                        phrases: Vec::new(),
                        request: None,
                    });
                }
                let end = pcm.len() as f64 / f64::from(SAMPLE_RATE);
                let half = |text: &str, start: f64, end: f64| Phrase {
                    start,
                    end,
                    text: format!("{text} {calls}"),
                };
                Ok(Transcript {
                    phrases: vec![
                        half("segment", 0.0, end / 2.0),
                        half("part", end / 2.0, end),
                    ],
                    request: Some(format!("request {calls}")),
                })
            })
        }
    }

    fn loud(frame: &[i16]) -> Result<f32, AudioError> {
        Ok(
            if frame.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) > 1000 {
                1.0
            } else {
                0.0
            },
        )
    }

    fn two_utterances() -> Vec<i16> {
        let speech = vec![5000; FRAME_SAMPLES * 20];
        let silence = vec![0; FRAME_SAMPLES * 30];
        [
            silence.clone(),
            speech.clone(),
            silence.clone(),
            speech,
            silence,
        ]
        .concat()
    }

    /// What a segment-by-segment transcriber reported, each line and failure
    /// with when it came.
    struct Reported {
        utterances: Vec<(Utterance, Duration)>,
        errors: Vec<(String, Duration)>,
        billed: Vec<(String, f64)>,
    }

    async fn reported(transcribing: impl AsyncFnOnce(&Listeners<'_>)) -> Reported {
        let began = Instant::now();
        let (utterances, errors, billed) = (
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
        );
        let listeners = Listeners {
            utterance: &|u| utterances.lock().unwrap().push((u, began.elapsed())),
            failure: &|who, detail| {
                errors
                    .lock()
                    .unwrap()
                    .push((format!("{who}: {detail}"), began.elapsed()));
            },
            partial: &|_| {},
            request: &|_| {},
            billed: &|id, seconds| billed.lock().unwrap().push((id, seconds)),
        };
        transcribing(&listeners).await;
        Reported {
            utterances: utterances.into_inner().unwrap(),
            errors: errors.into_inner().unwrap(),
            billed: billed.into_inner().unwrap(),
        }
    }

    struct Run {
        utterances: Vec<Utterance>,
        signals: Vec<(f64, bool)>,
        segments: Vec<usize>,
        billed: Vec<(String, f64)>,
        calls: Vec<usize>,
    }

    async fn run() -> Run {
        let stt = FakeStt::default();
        let (signals, segments) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()));
        let mut source = Recorded(two_utterances());
        let mut probability = loud;
        let reported = reported(async |listeners| {
            transcribe_channel(
                "them",
                &mut source,
                &mut probability,
                Transcriber::Segments(&stt),
                listeners,
                &|level, speech| signals.lock().unwrap().push((level, speech)),
                &|s| segments.lock().unwrap().push(s.start),
                SegmenterConfig::default(),
            )
            .await
            .unwrap();
        })
        .await;
        Run {
            utterances: reported.utterances.into_iter().map(|(u, _)| u).collect(),
            signals: signals.into_inner().unwrap(),
            segments: segments.into_inner().unwrap(),
            billed: reported.billed,
            calls: stt.calls.into_inner().unwrap(),
        }
    }

    #[tokio::test]
    async fn replays_audio_into_ordered_utterances() {
        let run = run().await;
        let texts: Vec<String> = run.utterances.iter().map(Utterance::text).collect();
        assert_eq!(texts, ["segment 1 part 1", "segment 2 part 2"]);
        assert!(run.utterances.iter().all(|u| u.who == "them"));
        // Speech spans frames 30–50 and 80–100 (32 ms each), padded by 96 ms.
        let millis = |t: f64| (t * 1000.0).round() as u64;
        let spans: Vec<(u64, u64, u64)> = run
            .utterances
            .iter()
            .map(|u| {
                (
                    millis(u.phrases[0].start),
                    millis(u.phrases[1].start),
                    millis(u.end()),
                )
            })
            .collect();
        assert_eq!(spans, [(864, 1280, 1696), (2464, 2880, 3296)]);
        assert_eq!(run.segments, [864 * 16, 2464 * 16]);
        assert_eq!(run.calls.len(), 2);
        // Each segment's request is billed for the segment's length.
        let lengths: Vec<(String, f64)> = run
            .calls
            .iter()
            .enumerate()
            .map(|(at, &samples)| (format!("request {}", at + 1), seconds(samples)))
            .collect();
        assert_eq!(run.billed, lengths);
    }

    /// Transcribe one segment per marker, each a second of the marker's value
    /// starting at `marker` × 10 s, all closed at once.
    async fn segments_run(stt: &FakeStt, markers: &[i16]) -> Reported {
        let rate = SAMPLE_RATE as usize;
        let closed = Instant::now();
        let captured: Vec<Captured> = markers
            .iter()
            .map(|&marker| {
                let segment = Segment {
                    pcm: vec![marker; rate],
                    start: marker as usize * 10 * rate,
                };
                Captured::Segment(segment, closed)
            })
            .collect();
        reported(async |listeners| {
            let captured = stream::iter(captured).boxed();
            transcribe("them", captured, Transcriber::Segments(stt), listeners).await;
        })
        .await
    }

    fn failing(marker: i16, times: usize) -> FakeStt {
        FakeStt {
            failing: Mutex::new(HashMap::from([(marker, times)])),
            ..FakeStt::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_segment_is_tried_again_and_keeps_its_time() {
        let stt = failing(1, 2);
        let run = segments_run(&stt, &[1]).await;
        assert!(run.errors.is_empty());
        assert_eq!(stt.calls.lock().unwrap().len(), 3);
        // After waits of 1 and 2 s; timed from the segment's start at 10 s.
        assert_eq!(run.utterances.len(), 1);
        let (utterance, at) = &run.utterances[0];
        assert_eq!(*at, Duration::from_secs(3));
        assert_eq!(utterance.latency, Duration::from_secs(3));
        assert_eq!(utterance.phrases[0].start, 10.0);
        assert_eq!(utterance.end(), 11.0);
        assert_eq!(utterance.text(), "segment 3 part 3");
    }

    #[tokio::test(start_paused = true)]
    async fn a_segment_that_keeps_failing_is_reported_once_and_the_next_is_heard() {
        let stt = failing(1, usize::MAX);
        let run = segments_run(&stt, &[1, 2]).await;
        // Four attempts, after waits of 1, 2 and 4 s.
        assert_eq!(
            run.errors,
            [("them: down".to_string(), Duration::from_secs(7))]
        );
        assert_eq!(stt.calls.lock().unwrap().len(), 5);
        let starts: Vec<f64> = run
            .utterances
            .iter()
            .map(|(u, _)| u.phrases[0].start)
            .collect();
        assert_eq!(starts, [20.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_segment_tried_again_keeps_its_place_before_later_ones() {
        let stt = failing(1, 2);
        let run = segments_run(&stt, &[1, 2]).await;
        assert!(run.errors.is_empty());
        // The second segment, answered at once, waits for the first.
        let lines: Vec<(f64, Duration)> = run
            .utterances
            .iter()
            .map(|(u, at)| (u.phrases[0].start, *at))
            .collect();
        assert_eq!(
            lines,
            [
                (10.0, Duration::from_secs(3)),
                (20.0, Duration::from_secs(3))
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_failing_provider_is_not_tried_past_the_bound() {
        let stt = FakeStt {
            slow: Duration::from_secs(6),
            ..failing(1, usize::MAX)
        };
        let run = segments_run(&stt, &[1]).await;
        // Failures at 6, 13 and 21 s; a wait of 4 s more would start past 15 s.
        assert_eq!(
            run.errors,
            [("them: down".to_string(), Duration::from_secs(21))]
        );
        assert_eq!(stt.calls.lock().unwrap().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_segment_without_words_leaves_no_line() {
        let stt = FakeStt::default();
        let run = segments_run(&stt, &[0, 2]).await;
        assert!(run.errors.is_empty());
        let starts: Vec<f64> = run
            .utterances
            .iter()
            .map(|(u, _)| u.phrases[0].start)
            .collect();
        assert_eq!(starts, [20.0]);
    }

    #[tokio::test]
    async fn every_frame_reports_its_signal() {
        let run = run().await;
        assert_eq!(run.signals.len(), two_utterances().len() / FRAME_SAMPLES);
        assert_eq!(run.signals.iter().filter(|(_, speech)| *speech).count(), 40);
        assert_eq!(run.signals[0].0, 0.0);
        assert!((0.6..0.8).contains(&run.signals[30].0));
    }

    /// A provider that refuses its first `refused` connections before taking
    /// audio; the others name their request first, then say one phrase per
    /// 25 frames they receive (0.8 s), timed from their stream's start, with
    /// what was heard so far at the 13th. The first connection it accepts ends
    /// after `drop_after` frames: with an error, or cleanly when `closes`.
    struct FakeStream {
        connections: Mutex<usize>,
        refused: usize,
        drop_after: Option<usize>,
        closes: bool,
        /// When each connection was opened.
        opened: Mutex<Vec<Instant>>,
    }

    fn fake_stream(refused: usize, drop_after: Option<usize>, closes: bool) -> FakeStream {
        FakeStream {
            connections: Mutex::new(0),
            refused,
            drop_after,
            closes,
            opened: Mutex::new(Vec::new()),
        }
    }

    impl StreamingSpeechToText for FakeStream {
        fn transcribe<'a>(
            &'a self,
            frames: BoxStream<'a, Frame>,
        ) -> BoxStream<'a, Result<Heard, TranscriptionError>> {
            self.opened.lock().unwrap().push(Instant::now());
            let connection = {
                let mut connections = self.connections.lock().unwrap();
                *connections += 1;
                *connections
            };
            if connection <= self.refused {
                return stream::once(async { Err(TranscriptionError("unreachable".into())) })
                    .boxed();
            }
            let limit = self
                .drop_after
                .filter(|_| connection == self.refused.saturating_add(1));
            let counted = frames.enumerate().take_while(move |(index, _)| {
                std::future::ready(limit.is_none_or(|limit| *index < limit))
            });
            let named = Ok(Heard::Request(format!("request {connection}")));
            let phrases = counted.filter_map(|(index, _)| async move {
                let end = (index + 1) as f64 * 0.032;
                match (index + 1) % 25 {
                    0 => Some(Ok(Heard::Phrase(Phrase {
                        start: end - 0.8,
                        end,
                        text: format!("phrase at {end:.1}"),
                    }))),
                    13 => Some(Ok(Heard::Partial("phrase at".into()))),
                    _ => None,
                }
            });
            let phrases = stream::once(async { named }).chain(phrases);
            match limit {
                Some(_) if !self.closes => phrases
                    .chain(stream::once(async {
                        Err(TranscriptionError("connection reset".into()))
                    }))
                    .boxed(),
                _ => phrases.boxed(),
            }
        }
    }

    struct Streamed {
        utterances: Vec<Utterance>,
        errors: Vec<String>,
        segments: usize,
        partials: usize,
        requests: Vec<String>,
    }

    async fn stream_run(stt: &FakeStream) -> Streamed {
        let (utterances, errors, segments, partials, requests) = (
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
            Mutex::new(0),
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
        );
        let listeners = Listeners {
            utterance: &|u| utterances.lock().unwrap().push(u),
            failure: &|who, detail| errors.lock().unwrap().push(format!("{who}: {detail}")),
            partial: &|words| partials.lock().unwrap().push(words),
            request: &|id| requests.lock().unwrap().push(id),
            billed: &|_, _| {},
        };
        let mut source = Recorded(two_utterances());
        let mut probability = loud;
        transcribe_channel(
            "them",
            &mut source,
            &mut probability,
            Transcriber::Stream(stt),
            &listeners,
            &|_, _| {},
            &|_| *segments.lock().unwrap() += 1,
            SegmenterConfig::default(),
        )
        .await
        .unwrap();
        Streamed {
            utterances: utterances.into_inner().unwrap(),
            errors: errors.into_inner().unwrap(),
            segments: segments.into_inner().unwrap(),
            partials: partials.into_inner().unwrap().len(),
            requests: requests.into_inner().unwrap(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_gets_every_frame_and_its_phrases_in_order() {
        let stt = fake_stream(0, None, false);
        let run = stream_run(&stt).await;
        // 130 frames: a phrase after each 25, and what was heard on the way.
        let ends: Vec<String> = run
            .utterances
            .iter()
            .map(|u| format!("{:.1}", u.end()))
            .collect();
        assert_eq!(ends, ["0.8", "1.6", "2.4", "3.2", "4.0"]);
        assert_eq!(run.partials, 5);
        assert!(run.errors.is_empty());
        assert_eq!(run.segments, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_ends_with_its_capture() {
        let stt = fake_stream(0, None, false);
        let heard = Mutex::new(0);
        let listeners = Listeners {
            utterance: &|_| *heard.lock().unwrap() += 1,
            failure: &|_, _| {},
            partial: &|_| {},
            request: &|_| {},
            billed: &|_, _| {},
        };
        // An unfold, which must not be polled again once it ended.
        let captured = stream::unfold(0, |index| async move {
            (index < 30).then(|| (Captured::Frame(index, vec![0; FRAME_SAMPLES]), index + 1))
        });
        transcribe(
            "them",
            captured.boxed(),
            Transcriber::Stream(&stt),
            &listeners,
        )
        .await;
        assert_eq!(*heard.lock().unwrap(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_stream_reconnects_and_keeps_the_time() {
        let stt = fake_stream(0, Some(60), false);
        let Streamed {
            utterances,
            errors,
            requests,
            ..
        } = stream_run(&stt).await;
        assert_eq!(errors, ["them: connection reset"]);
        assert_eq!(*stt.connections.lock().unwrap(), 2);
        // Each connection bills its own request.
        assert_eq!(requests, ["request 1", "request 2"]);
        // Two phrases before the drop; then the second connection's, timed
        // after the 61 frames the first took.
        let ends: Vec<String> = utterances
            .iter()
            .map(|u| format!("{:.2}", u.end()))
            .collect();
        assert_eq!(ends[..2], ["0.80", "1.60"]);
        assert_eq!(ends[2], format!("{:.2}", (61 + 25) as f64 * 0.032));
    }

    /// What a paced capture's transcription did: each phrase's end, each
    /// failure, and when each connection opened and the transcription ended,
    /// in seconds from its start.
    struct Paced {
        ends: Vec<String>,
        errors: Vec<String>,
        opened: Vec<f64>,
        over: f64,
    }

    /// Transcribe a capture of `frames` silent frames heard one every 32 ms.
    async fn paced_run(stt: &FakeStream, frames: usize) -> Paced {
        let (ends, errors) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()));
        let listeners = Listeners {
            utterance: &|u| ends.lock().unwrap().push(format!("{:.2}", u.end())),
            failure: &|who, detail| errors.lock().unwrap().push(format!("{who}: {detail}")),
            partial: &|_| {},
            request: &|_| {},
            billed: &|_, _| {},
        };
        let captured = stream::unfold(0, |index| async move {
            sleep(Duration::from_millis(32)).await;
            (index < frames).then(|| (Captured::Frame(index, vec![0; FRAME_SAMPLES]), index + 1))
        });
        let start = Instant::now();
        transcribe(
            "them",
            captured.boxed(),
            Transcriber::Stream(stt),
            &listeners,
        )
        .await;
        let since =
            |at: &Instant| (at.duration_since(start).as_secs_f64() * 1000.0).round() / 1000.0;
        let opened = stt.opened.lock().unwrap().iter().map(since).collect();
        Paced {
            ends: ends.into_inner().unwrap(),
            errors: errors.into_inner().unwrap(),
            opened,
            over: since(&Instant::now()),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_stream_backs_off_until_it_is_back() {
        // Refused three times, then accepted and closed after 60 frames.
        let stt = fake_stream(3, Some(60), true);
        let run = paced_run(&stt, 300).await;
        // Waits of 1, 2 and 4 s; the fourth takes the 61 frames held at once
        // and closes, and after a connection that worked the wait is 1 s again.
        assert_eq!(run.opened, [0.0, 1.0, 3.0, 7.0, 8.0]);
        // One report per outage.
        assert_eq!(
            run.errors,
            ["them: unreachable", "them: the provider closed the stream"]
        );
        // The audio heard while down went to the connection that came back,
        // each phrase timed from the capture's start.
        assert_eq!(run.ends[..2], ["0.80", "1.60"]);
        assert_eq!(run.ends[2], format!("{:.2}", (61 + 25) as f64 * 0.032));
        assert_eq!(run.ends.len(), 2 + (300 - 61) / 25);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_closed_before_its_capture_is_opened_again() {
        let stt = fake_stream(0, Some(60), true);
        let run = paced_run(&stt, 300).await;
        assert_eq!(run.opened.len(), 2);
        assert_eq!(run.errors, ["them: the provider closed the stream"]);
        assert_eq!(run.ends[..2], ["0.80", "1.60"]);
        assert_eq!(run.ends[2], format!("{:.2}", (61 + 25) as f64 * 0.032));
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_that_never_comes_back_stops_with_its_capture() {
        // 3750 frames: two minutes of capture.
        let stt = fake_stream(usize::MAX, None, false);
        let run = paced_run(&stt, 3750).await;
        // Waits double up to 30 s; when the capture ends, the audio held gets
        // one try more, right away, and nothing waits out the backoff.
        assert_eq!(
            run.opened,
            [0.0, 1.0, 3.0, 7.0, 15.0, 31.0, 61.0, 91.0, 120.032]
        );
        assert_eq!(run.over, 120.032);
        assert_eq!(run.errors, ["them: unreachable"]);
        assert!(run.ends.is_empty());
    }

    #[tokio::test]
    async fn monitoring_reports_signal_and_transcribes_nothing() {
        let signals = Mutex::new(Vec::new());
        let mut source = Recorded(two_utterances());
        let mut probability = loud;
        monitor_channel(
            &mut source,
            &mut probability,
            &|l, s| signals.lock().unwrap().push((l, s)),
            0.5,
        )
        .await
        .unwrap();
        let signals = signals.into_inner().unwrap();
        assert_eq!(signals.len(), two_utterances().len() / FRAME_SAMPLES);
        assert_eq!(signals.iter().filter(|(_, speech)| *speech).count(), 40);
    }
}
