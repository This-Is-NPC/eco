//! Importing a file into a session: a WebVTT transcript is read as it is; audio or
//! video is decoded by ffmpeg, segmented by the VAD and transcribed by the
//! configured STT, as fast as they go, while its speakers are told apart.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::UNIX_EPOCH;

use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;

use crate::adapters::audio_ffmpeg::{FfmpegSource, Probe, probe};
use crate::adapters::diarizer_process;
use crate::adapters::vad_silero::{SileroModel, SileroVad};
use crate::adapters::webvtt;
use crate::domain::assistant::Emit;
use crate::domain::channel::{
    Listeners, SpeechProbability, Transcriber, Trouble, Utterance, transcribe_channel,
};
use crate::domain::diarization::{Diarization, label_lines};
use crate::domain::events::{error, transcription};
use crate::domain::segmenter::{Segment, SegmenterConfig};
use crate::domain::session::{ENDED, HeardBy, IMPORT, Session, now, recorded_at};
use crate::paths;
use crate::ports::{AudioError, PeopleStore, SessionLog};

/// A file to import and what the session it becomes is.
pub struct Request {
    pub path: PathBuf,
    pub title: String,
    pub kind: String,
    pub language: String,
    /// Who the whole file is heard as: a file is one stream.
    pub participant: String,
    /// The transcription model that transcribes audio or video, by name.
    pub model: String,
    /// What the user says a minute of that model costs.
    pub per_minute: Option<f64>,
    /// When the recording began, in seconds since the epoch, as the user gave it;
    /// otherwise the file tells.
    pub started_at: Option<f64>,
}

/// The session being imported; it ends however the import stops — finished, failed
/// or cancelled — and clients are told whether the whole file was transcribed.
struct Importing {
    session: Mutex<Session>,
    complete: bool,
    emit: Emit,
}

impl Drop for Importing {
    fn drop(&mut self) {
        let mut session = self.session.lock().expect("not poisoned");
        session.set_state(ENDED);
        let done = json!({"type": "import_done", "id": session.id, "complete": self.complete});
        (self.emit)(done);
    }
}

fn failed(emit: &Emit, detail: String) {
    emit(error(
        "import.failed",
        format!("import: {detail}"),
        json!({"detail": detail}),
    ));
}

/// Open the session a `total_s`-long file recorded at `started_at` becomes,
/// heard by `heard`, and tell clients.
fn begin(
    request: &Request,
    log: &dyn SessionLog,
    emit: &Emit,
    (started_at, total): (f64, f64),
    heard: HeardBy,
) -> Importing {
    let mut session = Session::begin_at(
        &request.title,
        &request.kind,
        IMPORT,
        &request.language,
        started_at,
        log.writer(),
    );
    session.transcribe_with(heard);
    emit(json!({
        "type": "import_started",
        "session": {
            "id": session.id, "title": session.title, "kind": session.kind, "source": session.source,
            "language": session.language, "state": session.state, "started_at": session.started_at,
        },
        "total_s": total,
    }));
    Importing {
        session: Mutex::new(session),
        complete: false,
        emit: Arc::clone(emit),
    }
}

/// What a file to import holds.
enum Content {
    Transcript,
    Media(Probe),
}

/// What the file at `path` holds; neither a WebVTT transcript nor media ffprobe
/// reads is an error.
async fn content(path: &Path) -> Result<Content, String> {
    let mut head = [0u8; 16];
    let read = match tokio::fs::File::open(path).await {
        Ok(mut file) => file.read(&mut head).await.unwrap_or(0),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if webvtt::is_webvtt(&head[..read]) {
        return Ok(Content::Transcript);
    }
    probe(path)
        .await
        .map(Content::Media)
        .ok_or_else(|| format!("{} is not audio or video", path.display()))
}

/// When the recording in the file at `path` began: `given`, else the date media
/// was recorded with, else when the file last changed.
async fn started(path: &Path, content: &Content, given: Option<f64>) -> f64 {
    let tagged = match content {
        Content::Media(probe) => probe.recorded,
        Content::Transcript => None,
    };
    let modified = tokio::fs::metadata(path)
        .await
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64());
    recorded_at(given, tagged, modified)
}

/// When the recording in the file at `path` began, as an import without a date
/// of the user's dates it; none when the file cannot be imported.
pub async fn date(path: &Path) -> Option<f64> {
    let content = content(path).await.ok()?;
    Some(started(path, &content, None).await)
}

/// Import `request` into a new session; a file that is neither a transcript nor
/// media ffprobe reads fails before any session exists.
pub async fn run(
    request: Request,
    stt: Transcriber<'_>,
    vad: SileroModel,
    log: Arc<dyn SessionLog>,
    people: Arc<dyn PeopleStore>,
    emit: Emit,
) {
    let mut vad = SileroVad::new(vad);
    let mut probability = |frame: &[i16]| {
        vad.probability(frame)
            .map_err(|e| AudioError(e.to_string()))
    };
    let hearing = Hearing {
        stt,
        speech: &mut probability,
        speakers: || speakers(&paths::speaker_model()),
    };
    import(request, hearing, &*log, &*people, emit).await;
}

/// What hears a media file: its STT, the speech probability of each frame, and
/// what tells its speakers apart, started once its audio is read.
struct Hearing<'a, S> {
    stt: Transcriber<'a>,
    speech: SpeechProbability<'a>,
    speakers: S,
}

/// Import `request` as `run` does, hearing media with `hearing`.
async fn import(
    request: Request,
    hearing: Hearing<'_, impl FnOnce() -> Option<Diarizing>>,
    log: &dyn SessionLog,
    people: &dyn PeopleStore,
    emit: Emit,
) {
    let content = match content(&request.path).await {
        Ok(content) => content,
        Err(detail) => return failed(&emit, detail),
    };
    let started_at = started(&request.path, &content, request.started_at).await;
    match content {
        Content::Transcript => transcript(request, started_at, log, emit).await,
        Content::Media(probe) => {
            let at = (started_at, probe.duration);
            media(request, at, hearing, log, people, emit).await;
        }
    }
}

/// A WebVTT transcript: each cue a line, said by the speaker the file names (or
/// the request's participant), at its time in the recording.
async fn transcript(request: Request, started_at: f64, log: &dyn SessionLog, emit: Emit) {
    let cues = match tokio::fs::read_to_string(&request.path).await {
        Ok(text) => webvtt::parse(&text),
        Err(e) => Err(e.to_string()),
    };
    let cues = match cues {
        Ok(cues) => cues,
        Err(e) => return failed(&emit, format!("{}: {e}", request.path.display())),
    };
    let total = cues.iter().map(|cue| cue.end).fold(0.0, f64::max);
    let mut importing = begin(&request, log, &emit, (started_at, total), HeardBy::Nothing);
    {
        let mut session = importing.session.lock().expect("not poisoned");
        for cue in &cues {
            let who = cue.speaker.as_deref().unwrap_or(&request.participant);
            session.hear_at(who, &cue.text, started_at + cue.start);
        }
        let id = session.id.clone();
        emit(json!({"type": "import_progress", "id": id, "done_s": total, "total_s": total}));
    }
    importing.complete = true;
}

/// The speakers of a file, told apart beside its transcription by a child
/// process that holds the embedding model only while the import runs.
struct Diarizing {
    regions: mpsc::Sender<Segment>,
    found: oneshot::Receiver<Result<Diarization, String>>,
}

/// The speakers of a file, told apart with the embedding model at `model`
/// (`eco setup` installs it); none without it, and lines keep the participant.
fn speakers(model: &Path) -> Option<Diarizing> {
    model.exists().then(|| diarizing(model))
}

fn diarizing(model: &Path) -> Diarizing {
    let (send, found) = oneshot::channel();
    let regions = diarizer_process::spawn(model, move |result| {
        let _ = send.send(result);
    });
    Diarizing { regions, found }
}

/// Audio or video recorded at `started_at` and `total` seconds long: transcribed
/// as speech arrives, reporting progress; one line per phrase, said by the
/// speaker diarization finds once the file is done.
async fn media(
    request: Request,
    (started_at, total): (f64, f64),
    hearing: Hearing<'_, impl FnOnce() -> Option<Diarizing>>,
    log: &dyn SessionLog,
    people: &dyn PeopleStore,
    emit: Emit,
) {
    let heard = HeardBy::Model(request.model.clone(), 1, request.per_minute);
    let mut importing = begin(&request, log, &emit, (started_at, total), heard);
    let id = importing.session.lock().expect("not poisoned").id.clone();
    let progress = |done: f64| {
        // A phrase padded past the last frame still reports the file done, not beyond.
        emit(
            json!({"type": "import_progress", "id": id, "done_s": done.min(total), "total_s": total}),
        );
    };
    let mut source = FfmpegSource::new(request.path.clone());
    let diarization = (hearing.speakers)();
    let spans = Mutex::new(Vec::new());
    let utterance = |heard: Utterance| {
        let mut session = importing.session.lock().expect("not poisoned");
        let mut spans = spans.lock().expect("not poisoned");
        for phrase in &heard.phrases {
            session.hear_at(&heard.who, &phrase.text, started_at + phrase.start);
            spans.push((phrase.start, phrase.end));
        }
        progress(heard.end());
    };
    let trouble = |who: &str, trouble: Trouble| emit(transcription(who, &trouble));
    // Each segment is a request of its own, the session's alone.
    let billed = |request_id: String, seconds: f64| {
        let mut session = importing.session.lock().expect("not poisoned");
        session.bill(&request_id, &request.model, (seconds, 1.0), now());
    };
    let segment = |region: &Segment| {
        if let Some(diarization) = &diarization {
            let _ = diarization.regions.send(region.clone());
        }
    };
    let listeners = Listeners {
        utterance: &utterance,
        trouble: &trouble,
        partial: &|_| {},
        request: &|_| {},
        billed: &billed,
    };
    let finished = transcribe_channel(
        &request.participant,
        &mut source,
        hearing.speech,
        hearing.stt,
        &listeners,
        &|_, _| {},
        &segment,
        SegmenterConfig::default(),
    )
    .await;
    match finished {
        Ok(()) => progress(total),
        Err(failure) => return failed(&emit, failure.0),
    }
    if let Some(Diarizing { regions, found }) = diarization {
        drop(regions);
        match found.await {
            Ok(Ok(diarization)) => {
                let spans = spans.into_inner().expect("not poisoned");
                if let Some(lines) = label_lines(&diarization.turns, &spans) {
                    let mut session = importing.session.lock().expect("not poisoned");
                    session.assign_speakers(&lines.labels);
                    let voices = lines.voices(&diarization.voices);
                    if let Err(e) = people.keep_voices(&id, &voices) {
                        emit(error(
                            "people.failed",
                            format!("people: {e}"),
                            json!({"detail": e.0}),
                        ));
                    }
                    let who = &lines.labels;
                    let names: Vec<&str> = who.iter().map(|label| session.name_of(label)).collect();
                    emit(json!({"type": "diarized", "session": id, "who": who, "names": names}));
                }
            }
            Ok(Err(detail)) => emit(error(
                "diarization.failed",
                format!("diarization: {detail}"),
                json!({"detail": detail}),
            )),
            Err(_) => {}
        }
    }
    importing.complete = true;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    use futures::StreamExt;
    use futures::future::BoxFuture;
    use futures::stream::{self, BoxStream};
    use serde_json::Value;
    use tokio::sync::Notify;

    use super::*;
    use crate::adapters::audio_file::tests::write_wav;
    use crate::adapters::people_files::PeopleFiles;
    use crate::adapters::session_files::SessionFiles;
    use crate::domain::diarization::Turn;
    use crate::ports::{
        Frame, Heard, Phrase, SAMPLE_RATE, SpeechToText, StreamingSpeechToText, Transcript,
        TranscriptionError,
    };

    const RATE: usize = SAMPLE_RATE as usize;
    /// When the recordings the tests import began, as the user gives it.
    const STARTED: f64 = 1_790_000_000.0;

    /// An STT that says `line N` over each whole segment, N counting its
    /// calls, billed as `request N`; a `down` one fails every call.
    #[derive(Default)]
    struct Stt {
        calls: AtomicUsize,
        down: bool,
    }

    impl SpeechToText for Stt {
        fn transcribe<'a>(
            &'a self,
            pcm: &'a [i16],
        ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            Box::pin(async move {
                if self.down {
                    return Err(TranscriptionError("down".into()));
                }
                Ok(Transcript {
                    phrases: vec![Phrase {
                        start: 0.0,
                        end: pcm.len() as f64 / RATE as f64,
                        text: format!("line {call}"),
                    }],
                    request: Some(format!("request {call}")),
                })
            })
        }
    }

    /// An STT that tells `asked` when the first segment reaches it, and never answers.
    #[derive(Default)]
    struct Stalled {
        asked: Notify,
    }

    impl SpeechToText for Stalled {
        fn transcribe<'a>(
            &'a self,
            _: &'a [i16],
        ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
            self.asked.notify_one();
            Box::pin(std::future::pending())
        }
    }

    /// A streaming STT that hears the whole stream, then names its request,
    /// says words as they come, and finishes one phrase from 1 s to `.0` s.
    struct Streamed(f64);

    impl StreamingSpeechToText for Streamed {
        fn transcribe<'a>(
            &'a self,
            frames: BoxStream<'a, Frame>,
        ) -> BoxStream<'a, Result<Heard, TranscriptionError>> {
            let heard = async move {
                frames.count().await;
                let phrase = Phrase {
                    start: 1.0,
                    end: self.0,
                    text: "all of it".into(),
                };
                let heard = [
                    Heard::Request("stream 1".into()),
                    Heard::Partial("all".into()),
                    Heard::Phrase(phrase),
                ];
                stream::iter(heard.map(Ok))
            };
            stream::once(heard).flatten().boxed()
        }
    }

    /// A frame is speech when it is loud.
    fn loud(frame: &[i16]) -> Result<f32, AudioError> {
        let speech = frame.iter().any(|sample| sample.unsigned_abs() > 1000);
        Ok(if speech { 1.0 } else { 0.0 })
    }

    /// Five seconds: a second of silence, a second of a 400 Hz tone, twice, and silence.
    fn two_phrases(directory: &Path) -> PathBuf {
        let silence = vec![0; RATE];
        let tone: Vec<i16> = (0..RATE)
            .map(|i| if i % 40 < 20 { 8000 } else { -8000 })
            .collect();
        let path = directory.join("talk.wav");
        let samples = [&silence, &tone, &silence, &tone, &silence].map(Vec::as_slice);
        write_wav(&path, &samples.concat(), SAMPLE_RATE);
        path
    }

    /// `wav` made into `name` beside it by ffmpeg, with `arguments` before the output.
    fn convert(wav: &Path, name: &str, arguments: &[&str]) -> PathBuf {
        let converted = wav.with_file_name(name);
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(wav)
            .args(arguments)
            .arg(&converted)
            .status()
            .unwrap();
        assert!(status.success());
        converted
    }

    fn request(path: &Path) -> Request {
        Request {
            path: path.to_path_buf(),
            title: "Standup".into(),
            kind: "meeting".into(),
            language: "en".into(),
            participant: "Ana".into(),
            model: "whisper".into(),
            per_minute: Some(0.006),
            started_at: Some(STARTED),
        }
    }

    /// An emitter and the events it kept.
    fn recorder() -> (Emit, Arc<Mutex<Vec<Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| kept.lock().unwrap().push(event));
        (emit, events)
    }

    /// Where a test keeps its files, sessions and people.
    struct Stores {
        directory: tempfile::TempDir,
        log: SessionFiles,
        people: PeopleFiles,
    }

    fn stores() -> Stores {
        let directory = tempfile::tempdir().unwrap();
        Stores {
            log: SessionFiles::new(directory.path().join("sessions")),
            people: PeopleFiles::new(directory.path().join("people")),
            directory,
        }
    }

    impl Stores {
        /// Import `request` with `stt` segment by segment, telling speakers
        /// apart with `speakers`, into these stores: the events it emitted.
        async fn import(
            &self,
            request: Request,
            stt: &dyn SpeechToText,
            speakers: impl FnOnce() -> Option<Diarizing>,
        ) -> Vec<Value> {
            self.hear(request, Transcriber::Segments(stt), speakers)
                .await
        }

        /// Import `request` with `stt` into these stores: the events it emitted.
        async fn hear(
            &self,
            request: Request,
            stt: Transcriber<'_>,
            speakers: impl FnOnce() -> Option<Diarizing>,
        ) -> Vec<Value> {
            let (emit, events) = recorder();
            let mut speech = loud;
            let hearing = Hearing {
                stt,
                speech: &mut speech,
                speakers,
            };
            import(request, hearing, &self.log, &self.people, emit).await;
            events.lock().unwrap().clone()
        }

        /// The stored records of the session `events` began.
        fn stored(&self, events: &[Value]) -> Vec<Value> {
            let id = events[0]["session"]["id"].as_str().unwrap();
            let records = self.log.read(id).unwrap();
            records.into_iter().map(Value::Object).collect()
        }
    }

    fn types(items: &[Value]) -> Vec<&str> {
        items
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect()
    }

    fn of_type<'a>(items: &'a [Value], kind: &str) -> Vec<&'a Value> {
        items.iter().filter(|item| item["type"] == kind).collect()
    }

    /// The speech records: who said what, in seconds from the session's start.
    fn lines(records: &[Value]) -> Vec<(String, String, f64)> {
        of_type(records, "speech")
            .into_iter()
            .map(|r| {
                let who = r["who"].as_str().unwrap().to_string();
                let text = r["text"].as_str().unwrap().to_string();
                (who, text, r["at"].as_f64().unwrap() - STARTED)
            })
            .collect()
    }

    /// A diarizer that answers what `answer` makes of the regions it was
    /// sent, or stops without a word when it makes nothing.
    fn diarizer(
        answer: impl FnOnce(Vec<Segment>) -> Option<Result<Diarization, String>> + Send + 'static,
    ) -> impl FnOnce() -> Option<Diarizing> {
        move || {
            let (regions, received) = mpsc::channel();
            let (send, found) = oneshot::channel();
            std::thread::spawn(move || {
                if let Some(answer) = answer(received.iter().collect()) {
                    let _ = send.send(answer);
                }
            });
            Some(Diarizing { regions, found })
        }
    }

    /// Each region said by one of `voices` voices, taking turns region by region.
    fn turns(voices: usize) -> impl FnOnce(Vec<Segment>) -> Option<Result<Diarization, String>> {
        move |regions| {
            let seconds = |samples: usize| samples as f64 / RATE as f64;
            let turns = regions.iter().enumerate().map(|(index, region)| Turn {
                start: seconds(region.start),
                end: seconds(region.start + region.pcm.len()),
                speaker: index % voices,
            });
            Some(Ok(Diarization {
                turns: turns.collect(),
                voices: (0..voices).map(|v| vec![v as f32, 1.0]).collect(),
            }))
        }
    }

    /// Audio is transcribed phrase by phrase, its progress told as it goes,
    /// into a session with the request's title, kind, language and date.
    #[tokio::test]
    async fn imports_audio_line_by_line() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores.import(request(&wav), &Stt::default(), || None).await;

        assert_eq!(
            types(&events),
            [
                "import_started",
                "import_progress",
                "import_progress",
                "import_progress",
                "import_done"
            ]
        );
        let session = &events[0]["session"];
        let id = session["id"].clone();
        assert_eq!(session["title"], "Standup");
        assert_eq!(session["kind"], "meeting");
        assert_eq!(session["source"], "import");
        assert_eq!(session["language"], "en");
        assert_eq!(session["state"], "importing");
        assert_eq!(session["started_at"], STARTED);
        assert_eq!(events[0]["total_s"], 5.0);
        let done: Vec<f64> = events[1..4]
            .iter()
            .map(|e| {
                assert_eq!((&e["id"], &e["total_s"]), (&id, &json!(5.0)));
                e["done_s"].as_f64().unwrap()
            })
            .collect();
        assert!((1.9..2.3).contains(&done[0]), "{done:?}");
        assert!((3.9..4.3).contains(&done[1]), "{done:?}");
        assert_eq!(done[2], 5.0);
        assert_eq!(
            events[4],
            json!({"type": "import_done", "id": id, "complete": true})
        );

        let records = stores.stored(&events);
        assert_eq!(records[0]["title"], "Standup");
        assert_eq!(records[0]["kind"], "meeting");
        assert_eq!(records[0]["started_at"], STARTED);
        let heard = lines(&records);
        assert_eq!(heard.len(), 2, "{heard:?}");
        for ((who, text, at), (said, from)) in heard.iter().zip([("line 1", 1.0), ("line 2", 3.0)])
        {
            assert_eq!((who.as_str(), text.as_str()), ("Ana", said));
            assert!((at - from).abs() < 0.15, "{text} at {at}");
        }
        let transcriber = of_type(&records, "transcriber");
        assert_eq!(transcriber[0]["model"], "whisper");
        assert_eq!(transcriber[0]["per_minute"], 0.006);
        let billed: Vec<&Value> = of_type(&records, "billed")
            .into_iter()
            .map(|r| &r["request"])
            .collect();
        assert_eq!(billed, [&json!("request 1"), &json!("request 2")]);
        assert_eq!(of_type(&records, "state").last().unwrap()["state"], "ended");
    }

    /// A video's sound track is what is transcribed.
    #[tokio::test]
    async fn imports_the_sound_of_a_video() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let color = ["-f", "lavfi", "-i", "color=size=16x16:duration=5"];
        let video = convert(&wav, "talk.mkv", &[&color[..], &["-c:v", "mpeg4"]].concat());
        let events = stores
            .import(request(&video), &Stt::default(), || None)
            .await;

        assert_eq!(events.last().unwrap()["complete"], true);
        let heard = lines(&stores.stored(&events));
        let said: Vec<&str> = heard.iter().map(|(_, text, _)| text.as_str()).collect();
        assert_eq!(said, ["line 1", "line 2"]);
    }

    /// A streaming STT's finished phrases are the lines; its words so far and
    /// its request are not kept.
    #[tokio::test]
    async fn imports_through_a_streaming_transcriber() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .hear(request(&wav), Transcriber::Stream(&Streamed(4.0)), || None)
            .await;

        let done: Vec<&Value> = of_type(&events, "import_progress")
            .into_iter()
            .map(|e| &e["done_s"])
            .collect();
        assert_eq!(done, [&json!(4.0), &json!(5.0)]);
        assert_eq!(events.last().unwrap()["complete"], true);
        let records = stores.stored(&events);
        assert_eq!(lines(&records), [("Ana".into(), "all of it".into(), 1.0)]);
        assert!(of_type(&records, "listening").is_empty());
        assert!(of_type(&records, "billed").is_empty());
    }

    /// A phrase that ends past the file, from padding, reports the file done.
    #[tokio::test]
    async fn progress_never_passes_the_end_of_the_file() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .hear(request(&wav), Transcriber::Stream(&Streamed(5.3)), || None)
            .await;
        let done: Vec<&Value> = of_type(&events, "import_progress")
            .into_iter()
            .map(|e| &e["done_s"])
            .collect();
        assert_eq!(done, [&json!(5.0), &json!(5.0)]);
    }

    /// Without the embedding model, nothing tells speakers apart.
    #[test]
    fn no_model_tells_no_speakers() {
        let directory = tempfile::tempdir().unwrap();
        assert!(speakers(&directory.path().join("missing.onnx")).is_none());
    }

    /// Diarization says who spoke each line once the file is done, and keeps
    /// the session's voices.
    #[tokio::test]
    async fn the_speakers_found_say_the_lines() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .import(request(&wav), &Stt::default(), diarizer(turns(2)))
            .await;

        let id = events[0]["session"]["id"].as_str().unwrap();
        let who = json!(["Speaker 1", "Speaker 2"]);
        assert_eq!(
            of_type(&events, "diarized"),
            [&json!({"type": "diarized", "session": id, "who": who, "names": who})]
        );
        assert_eq!(events.last().unwrap()["complete"], true);
        let records = stores.stored(&events);
        assert_eq!(of_type(&records, "diarized")[0]["who"], who);
        let voices = stores.people.voices(id);
        assert_eq!(voices["Speaker 1"], [0.0, 1.0]);
        assert_eq!(voices["Speaker 2"], [1.0, 1.0]);
    }

    /// One voice saying every line leaves them the participant's.
    #[tokio::test]
    async fn one_voice_keeps_the_participant() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .import(request(&wav), &Stt::default(), diarizer(turns(1)))
            .await;

        assert!(of_type(&events, "diarized").is_empty(), "{events:?}");
        let records = stores.stored(&events);
        assert!(of_type(&records, "diarized").is_empty());
        assert!(lines(&records).iter().all(|(who, ..)| who == "Ana"));
    }

    /// A diarizer that fails is told; the import is still whole.
    #[tokio::test]
    async fn a_failing_diarizer_is_told() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let broke = diarizer(|_| Some(Err("model broke".into())));
        let events = stores.import(request(&wav), &Stt::default(), broke).await;

        let errors = of_type(&events, "error");
        assert_eq!(errors.len(), 1, "{events:?}");
        assert_eq!(errors[0]["code"], "diarization.failed");
        assert_eq!(errors[0]["params"]["detail"], "model broke");
        assert_eq!(events.last().unwrap()["complete"], true);
    }

    /// A diarizer that stops without a word says nothing more.
    #[tokio::test]
    async fn a_silent_diarizer_says_nothing() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .import(request(&wav), &Stt::default(), diarizer(|_| None))
            .await;

        assert!(of_type(&events, "error").is_empty(), "{events:?}");
        assert!(of_type(&events, "diarized").is_empty(), "{events:?}");
        assert_eq!(events.last().unwrap()["complete"], true);
    }

    /// Voices that cannot be kept are an error; the lines still say who spoke.
    #[tokio::test]
    async fn voices_that_cannot_be_kept_are_told() {
        let mut stores = stores();
        let blocked = stores.directory.path().join("people");
        std::fs::write(&blocked, b"a file where the directory goes").unwrap();
        stores.people = PeopleFiles::new(blocked);
        let wav = two_phrases(stores.directory.path());
        let events = stores
            .import(request(&wav), &Stt::default(), diarizer(turns(2)))
            .await;

        let errors = of_type(&events, "error");
        assert_eq!(errors.len(), 1, "{events:?}");
        assert_eq!(errors[0]["code"], "people.failed");
        assert_eq!(of_type(&events, "diarized").len(), 1);
    }

    /// A segment the STT keeps failing is tried four times, then lost and
    /// told; the import still reaches the end of the file.
    #[tokio::test(start_paused = true)]
    async fn a_failing_stt_loses_each_segment_after_its_retries() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let stt = Stt {
            down: true,
            ..Stt::default()
        };
        let events = stores.import(request(&wav), &stt, || None).await;

        assert_eq!(stt.calls.load(Ordering::SeqCst), 8);
        let errors = of_type(&events, "error");
        assert_eq!(errors.len(), 2, "{events:?}");
        for error in errors {
            assert_eq!(error["code"], "transcription.failed");
            assert_eq!(error["params"], json!({"who": "Ana", "detail": "down"}));
        }
        assert_eq!(events.last().unwrap()["complete"], true);
        assert!(lines(&stores.stored(&events)).is_empty());
    }

    /// Media whose audio cannot be decoded fails once its session has begun:
    /// the session ends, not whole.
    #[tokio::test]
    async fn media_without_sound_fails_and_ends_its_session() {
        let stores = stores();
        let video = stores.directory.path().join("silent.mkv");
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-f", "lavfi"])
            .args(["-i", "color=size=16x16:duration=1", "-c:v", "mpeg4"])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let events = stores
            .import(request(&video), &Stt::default(), || None)
            .await;

        assert_eq!(types(&events), ["import_started", "error", "import_done"]);
        assert_eq!(events[1]["code"], "import.failed");
        let detail = events[1]["params"]["detail"].as_str().unwrap();
        assert!(detail.starts_with("ffmpeg: "), "{detail}");
        assert_eq!(events[2]["complete"], false);
        let records = stores.stored(&events);
        assert_eq!(of_type(&records, "state").last().unwrap()["state"], "ended");
    }

    /// An import dropped midway — the user cancelled it — ends its session
    /// and tells clients it is not whole.
    #[tokio::test]
    async fn a_cancelled_import_ends_its_session() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let stt = Stalled::default();
        let (emit, events) = recorder();
        let mut speech = loud;
        let hearing = Hearing {
            stt: Transcriber::Segments(&stt),
            speech: &mut speech,
            speakers: || None,
        };
        let importing = import(request(&wav), hearing, &stores.log, &stores.people, emit);
        tokio::select! {
            () = importing => panic!("a stalled STT never lets the import finish"),
            () = stt.asked.notified() => {}
        }

        let events = events.lock().unwrap().clone();
        assert_eq!(types(&events), ["import_started", "import_done"]);
        assert_eq!(events[1]["complete"], false);
        let records = stores.stored(&events);
        assert_eq!(of_type(&records, "state").last().unwrap()["state"], "ended");
    }

    /// A WebVTT transcript's cues are the lines, said by the speaker each names
    /// or else the participant, at their time in the recording.
    #[tokio::test]
    async fn a_transcript_is_read_as_it_is() {
        let stores = stores();
        let path = stores.directory.path().join("call.vtt");
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\n<v Bruna>Hello.</v>\n\n\
                   00:00:04.000 --> 00:00:06.000\nShall we start?\n";
        std::fs::write(&path, vtt).unwrap();
        let events = stores
            .import(request(&path), &Stalled::default(), || None)
            .await;

        assert_eq!(
            types(&events),
            ["import_started", "import_progress", "import_done"]
        );
        assert_eq!(events[0]["total_s"], 6.0);
        assert_eq!(events[0]["session"]["started_at"], STARTED);
        assert_eq!(events[1]["done_s"], 6.0);
        assert_eq!(events[1]["total_s"], 6.0);
        assert_eq!(events[2]["complete"], true);
        let records = stores.stored(&events);
        assert_eq!(
            lines(&records),
            [
                ("Bruna".into(), "Hello.".into(), 1.0),
                ("Ana".into(), "Shall we start?".into(), 4.0)
            ]
        );
        assert_eq!(of_type(&records, "transcriber")[0]["model"], Value::Null);
    }

    /// A transcript without cues, or not text, fails before any session exists.
    #[tokio::test]
    async fn a_broken_transcript_fails_before_a_session() {
        let stores = stores();
        for (name, bytes) in [
            ("empty.vtt", b"WEBVTT\n\nno cue here\n".as_slice()),
            ("binary.vtt", b"WEBVTT\n\n\xff\xfe\n".as_slice()),
        ] {
            let path = stores.directory.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            let events = stores
                .import(request(&path), &Stt::default(), || None)
                .await;
            assert_eq!(types(&events), ["error"], "{name}");
            assert_eq!(events[0]["code"], "import.failed");
            let detail = events[0]["params"]["detail"].as_str().unwrap();
            assert!(detail.starts_with(&path.display().to_string()), "{detail}");
        }
        assert!(stores.log.all().is_empty());
    }

    /// A file that is missing, a directory, or neither transcript nor media
    /// fails before any session exists.
    #[tokio::test]
    async fn what_is_not_a_recording_fails_before_a_session() {
        let stores = stores();
        let directory = stores.directory.path();
        let garbage = directory.join("notes.txt");
        std::fs::write(&garbage, b"not a recording").unwrap();
        let missing = directory.join("missing.wav");
        let cases = [
            (
                missing.clone(),
                format!("{}: No such file", missing.display()),
            ),
            (
                directory.to_path_buf(),
                format!("{} is not audio or video", directory.display()),
            ),
            (
                garbage.clone(),
                format!("{} is not audio or video", garbage.display()),
            ),
        ];
        for (path, told) in cases {
            let events = stores
                .import(request(&path), &Stt::default(), || None)
                .await;
            assert_eq!(types(&events), ["error"]);
            let detail = events[0]["params"]["detail"].as_str().unwrap();
            assert!(detail.starts_with(&told), "{detail}");
            assert_eq!(events[0]["message"], format!("import: {detail}"));
        }
        assert!(stores.log.all().is_empty());
    }

    /// The date a file was recorded with, in ISO 8601, as ffmpeg tags it.
    const TAGGED: [&str; 2] = ["-metadata", "creation_time=2026-09-30T09:15:00Z"];

    /// A file's date is the one media was recorded with, else when the file
    /// last changed; none when it cannot be imported.
    #[tokio::test]
    async fn dates_a_file_as_an_import_would() {
        let directory = tempfile::tempdir().unwrap();
        let wav = two_phrases(directory.path());
        let tagged = convert(&wav, "tagged.m4a", &TAGGED);
        assert_eq!(date(&tagged).await, Some(1_790_759_700.0));

        let changed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let vtt = directory.path().join("call.vtt");
        std::fs::write(&vtt, "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHi.\n").unwrap();
        for path in [&wav, &vtt] {
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(changed).unwrap();
            assert_eq!(
                date(path).await,
                Some(1_700_000_000.0),
                "{}",
                path.display()
            );
        }
        assert_eq!(date(&directory.path().join("missing.wav")).await, None);
    }

    /// A date the user gives wins over the one the file was recorded with.
    #[tokio::test]
    async fn the_date_given_wins() {
        let stores = stores();
        let wav = two_phrases(stores.directory.path());
        let tagged = convert(&wav, "tagged.m4a", &TAGGED);
        let events = stores
            .import(request(&tagged), &Stt::default(), || None)
            .await;
        assert_eq!(events[0]["session"]["started_at"], STARTED);
        let undated = Request {
            started_at: None,
            ..request(&tagged)
        };
        let events = stores.import(undated, &Stt::default(), || None).await;
        assert_eq!(events[0]["session"]["started_at"], 1_790_759_700.0);
    }

    /// `run` imports with the VAD model `eco setup` installs.
    #[tokio::test]
    async fn runs_with_the_installed_vad() {
        let vad = SileroModel::load(&paths::vad_model());
        let vad = vad.expect("run `mise run setup` first: the VAD model is missing");
        let stores = stores();
        let path = stores.directory.path().join("call.vtt");
        std::fs::write(&path, "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHi.\n").unwrap();
        let log = Arc::new(SessionFiles::new(stores.directory.path().join("sessions")));
        let people = Arc::new(PeopleFiles::new(stores.directory.path().join("people")));
        let (emit, events) = recorder();
        let stt = Stt::default();
        run(
            request(&path),
            Transcriber::Segments(&stt),
            vad,
            log,
            people,
            emit,
        )
        .await;

        let events = events.lock().unwrap().clone();
        assert_eq!(events.last().unwrap()["complete"], true);
        assert_eq!(lines(&stores.stored(&events)).len(), 1);
    }
}
