//! One audio source, segmented and transcribed — or only measured.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep};

use crate::domain::loudness::loudness;
use crate::domain::segmenter::{Segment, Segmenter, SegmenterConfig};
use crate::ports::{
    AudioError, AudioSource, FRAME_SAMPLES, Frame, Heard, Phrase, SAMPLE_RATE, SpeechToText,
    StreamingSpeechToText, Transcript,
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

/// Each segment the VAD closes goes to the STT on its own, a few at a time, their
/// lines kept in order.
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
            let result = stt.transcribe(&segment.pcm).await;
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

/// Every frame goes to the provider, which says where speech ends. A
/// connection that drops after working is opened again, its phrases timed
/// from the first frame it received.
async fn streamed(
    who: &str,
    captured: CapturedStream<'_>,
    stt: &dyn StreamingSpeechToText,
    listeners: &Listeners<'_>,
) {
    // Fused: a connection may poll it again after the capture ended, and so does
    // the drain below.
    let mut frames = captured
        .filter_map(|captured| {
            std::future::ready(match captured {
                Captured::Frame(index, frame) => Some((index, frame)),
                Captured::Segment(..) => None,
            })
        })
        .fuse();
    // When the capture's first frame was heard, for the latency of each phrase.
    let origin: OnceLock<Instant> = OnceLock::new();
    // The number of the first frame the connection received, and whether the
    // capture ended.
    let first = AtomicUsize::new(usize::MAX);
    let ended = AtomicBool::new(false);
    loop {
        first.store(usize::MAX, Ordering::Relaxed);
        let fed = futures::stream::poll_fn(|context| {
            let polled = frames.poll_next_unpin(context);
            match &polled {
                Poll::Ready(Some((index, _))) => {
                    origin.get_or_init(|| {
                        Instant::now() - Duration::from_secs_f64(seconds(index * FRAME_SAMPLES))
                    });
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
        let mut failed = false;
        while let Some(next) = heard.next().await {
            match next {
                Ok(Heard::Partial(words)) => (listeners.partial)(words),
                Ok(Heard::Request(id)) => (listeners.request)(id),
                Ok(Heard::Phrase(phrase)) => {
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
                Err(error) => {
                    (listeners.failure)(who, &error.0);
                    failed = true;
                }
            }
        }
        drop(heard);
        // A connection that never took audio is not tried again.
        let took = first.load(Ordering::Relaxed) != usize::MAX;
        if !failed || ended.load(Ordering::Relaxed) || !took {
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }
    // Given up on: the rest of the audio is dropped, not kept.
    while frames.next().await.is_some() {}
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
    use std::sync::Mutex;

    use futures::future::BoxFuture;
    use futures::stream::{self, BoxStream};

    use super::*;
    use crate::ports::TranscriptionError;

    struct Recorded(Vec<i16>);

    impl AudioSource for Recorded {
        fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
            stream::iter(self.0.chunks(FRAME_SAMPLES).map(|c| Ok(c.to_vec()))).boxed()
        }
    }

    struct FakeStt {
        calls: Mutex<Vec<usize>>,
        fail_first: bool,
    }

    impl SpeechToText for FakeStt {
        fn transcribe<'a>(
            &'a self,
            pcm: &'a [i16],
        ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
            Box::pin(async move {
                let mut calls = self.calls.lock().unwrap();
                calls.push(pcm.len());
                if self.fail_first && calls.len() == 1 {
                    return Err(TranscriptionError("down".into()));
                }
                let end = pcm.len() as f64 / f64::from(SAMPLE_RATE);
                let half = |text: &str, start: f64, end: f64| Phrase {
                    start,
                    end,
                    text: format!("{text} {}", calls.len()),
                };
                Ok(Transcript {
                    phrases: vec![
                        half("segment", 0.0, end / 2.0),
                        half("part", end / 2.0, end),
                    ],
                    request: Some(format!("request {}", calls.len())),
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

    struct Run {
        utterances: Vec<Utterance>,
        signals: Vec<(f64, bool)>,
        errors: Vec<String>,
        segments: Vec<usize>,
        billed: Vec<(String, f64)>,
        calls: Vec<usize>,
    }

    async fn run(fail_first: bool) -> Run {
        let stt = FakeStt {
            calls: Mutex::new(Vec::new()),
            fail_first,
        };
        let (utterances, signals, errors, segments, billed) = (
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
            Mutex::new(Vec::new()),
        );
        let listeners = Listeners {
            utterance: &|u| utterances.lock().unwrap().push(u),
            failure: &|who, detail| errors.lock().unwrap().push(format!("{who}: {detail}")),
            partial: &|_| {},
            request: &|_| {},
            billed: &|id, seconds| billed.lock().unwrap().push((id, seconds)),
        };
        let mut source = Recorded(two_utterances());
        let mut probability = loud;
        transcribe_channel(
            "them",
            &mut source,
            &mut probability,
            Transcriber::Segments(&stt),
            &listeners,
            &|level, speech| signals.lock().unwrap().push((level, speech)),
            &|s| segments.lock().unwrap().push(s.start),
            SegmenterConfig::default(),
        )
        .await
        .unwrap();
        Run {
            utterances: utterances.into_inner().unwrap(),
            signals: signals.into_inner().unwrap(),
            errors: errors.into_inner().unwrap(),
            segments: segments.into_inner().unwrap(),
            billed: billed.into_inner().unwrap(),
            calls: stt.calls.into_inner().unwrap(),
        }
    }

    #[tokio::test]
    async fn replays_audio_into_ordered_utterances() {
        let run = run(false).await;
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

    #[tokio::test]
    async fn transcription_failure_skips_only_that_segment_and_reports_it() {
        let run = run(true).await;
        let texts: Vec<String> = run.utterances.iter().map(Utterance::text).collect();
        assert_eq!(texts, ["segment 2 part 2"]);
        assert_eq!(run.errors, ["them: down"]);
    }

    #[tokio::test]
    async fn every_frame_reports_its_signal() {
        let run = run(false).await;
        assert_eq!(run.signals.len(), two_utterances().len() / FRAME_SAMPLES);
        assert_eq!(run.signals.iter().filter(|(_, speech)| *speech).count(), 40);
        assert_eq!(run.signals[0].0, 0.0);
        assert!((0.6..0.8).contains(&run.signals[30].0));
    }

    /// A provider that names each connection's request first, then says one
    /// phrase per 25 frames it receives (0.8 s), timed from its stream's start,
    /// with what it has heard so far at the 13th; the first connection drops
    /// after `drop_after`.
    struct FakeStream {
        connections: Mutex<usize>,
        drop_after: Option<usize>,
    }

    impl StreamingSpeechToText for FakeStream {
        fn transcribe<'a>(
            &'a self,
            frames: BoxStream<'a, Frame>,
        ) -> BoxStream<'a, Result<Heard, TranscriptionError>> {
            let connection = {
                let mut connections = self.connections.lock().unwrap();
                *connections += 1;
                *connections
            };
            let limit = self.drop_after.filter(|_| connection == 1);
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
                Some(_) => phrases
                    .chain(stream::once(async {
                        Err(TranscriptionError("connection reset".into()))
                    }))
                    .boxed(),
                None => phrases.boxed(),
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
        let stt = FakeStream {
            connections: Mutex::new(0),
            drop_after: None,
        };
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
        let stt = FakeStream {
            connections: Mutex::new(0),
            drop_after: None,
        };
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
        let stt = FakeStream {
            connections: Mutex::new(0),
            drop_after: Some(60),
        };
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
