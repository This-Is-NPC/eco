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
use crate::domain::channel::{Listeners, Transcriber, Trouble, Utterance, transcribe_channel};
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
    let content = match content(&request.path).await {
        Ok(content) => content,
        Err(detail) => return failed(&emit, detail),
    };
    let started_at = started(&request.path, &content, request.started_at).await;
    match content {
        Content::Transcript => transcript(request, started_at, &*log, emit).await,
        Content::Media(probe) => {
            let at = (started_at, probe.duration);
            media(request, at, stt, vad, &*log, &*people, emit).await;
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
    stt: Transcriber<'_>,
    vad: SileroModel,
    log: &dyn SessionLog,
    people: &dyn PeopleStore,
    emit: Emit,
) {
    let heard = HeardBy::Model(request.model.clone(), 1, request.per_minute);
    let mut importing = begin(&request, log, &emit, (started_at, total), heard);
    let id = importing.session.lock().expect("not poisoned").id.clone();
    let progress = |done: f64| {
        emit(json!({"type": "import_progress", "id": id, "done_s": done, "total_s": total}));
    };
    let mut source = FfmpegSource::new(request.path.clone());
    let mut vad = SileroVad::new(vad);
    let mut probability = |frame: &[i16]| {
        vad.probability(frame)
            .map_err(|e| AudioError(e.to_string()))
    };
    // Without the model (no `eco setup`), lines keep the participant.
    let model = paths::speaker_model();
    let diarization = model.exists().then(|| diarizing(&model));
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
        &mut probability,
        stt,
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
