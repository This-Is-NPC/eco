# How it is built

This page is for contributors. Setting eco up belongs in the
[README](../README.md), each task in [the how-to pages](README.md), every
command and flag in [the command line](cli.md), the latency and audio
measurements in [benchmarks](benchmarks.md), and the language packs in
[translations](i18n.md); this one is the model, the
architecture, the protocol and the state on disk that those pages rest on.

The section numbers below are load-bearing. Code comments, error messages and
the other pages cite them as `docs/design.md §N`, so a section keeps its number:
a new one is added after the last, never in between.

---

## 1. The map

eco keeps voice sessions beside a conversation on the laptop. During a meeting
(Meet, Zoom, Teams, Discord, etc.), a conversation, a class or a spoken idea, it:

1. Captures the audio the laptop plays and, optionally, the user's microphone.
2. Transcribes it in real time, each line labelled with who said it.
3. Answers with an LLM, from the transcript and context the user gives, when the
   user asks: a skill, a free question, a summary afterwards.
4. Shows everything in a discreet window that never steals focus or gets in the
   way of the call.

Live or afterwards is the same session: a stored one keeps answering.

### Target environment

| Item | Value |
|---|---|
| OS | Arch Linux (Omarchy), Hyprland/Wayland — Hyprland config in **Lua** (`~/.config/hypr/*.lua`) |
| Audio | PipeWire 1.6 (`pw-record` available); `ffmpeg`/`ffprobe` to import files |
| Daemon | Rust (edition 2024, tokio), toolchain pinned by `mise`, built with `cargo` |
| Interface | QML on **Qt 6** (`qt6-base`, `qt6-declarative`; already installed, the omarchy-shell uses it), run by `eco-window`, a small C++ program built with CMake |
| Terminal | Ghostty |
| Hardware | 12 cores, 46 GB RAM, **no dedicated GPU** — Radeon 860M iGPU (Vulkan) + NPU (`/dev/accel0`, unused) |
| LAN model server (development) | Windows machine with a Radeon RX 9060 XT serving models over HTTP — see [how to use a model on the LAN](how-to-use-a-model-on-the-lan.md) |

---

## 2. The model

Six nouns, and the rest of the page is what happens to them.

**Session** — everything eco hears: a title, a **kind** (one of `kinds` in the
config — `meeting`, `conversation`, `other`, `idea` by default), a **source**
(`live`, or `import` for a file) and a language. Nothing is transcribed or stored
outside a session. §7.

**Audio source** — a name in the conversation (`Eu`, `Recrutador`…;
`[[participants]]` in the config) holding one or more PipeWire devices. One may
be marked as the user. Each device is its own stream, so a line already knows
whose it is. §4.

**Model** — registered once in `[[models]]`: a name, a type (`chat` or
`transcription`), the provider, its key and the provider's id. Everything else
names a model rather than a provider. §6.

**Action** — a user-defined request run against the transcript: a name, a
prompt, an output format and, optionally, a model. The interface calls them
**skills**; the config (`[[actions]]`), the socket and the CLI say action. §6.

**Person** — someone the user names, kept apart from any session, with up to 16
voiceprints. A session's speaker label becomes a person only when the user says
so. §7.4.

**Tag** — the user's grouping of sessions, several per session, apart from kinds
and people. §7.5.

---

## 3. Architecture

Two processes:

```
┌────────────────────────────── eco (Rust daemon) ─────────────────────────────┐
│                                                                              │
│  AudioSource ──PCM──▶ STT ──text──▶ ┌───────────── core ─────────────┐       │
│  (pw-record /         (Deepgram /   │ session: buffer, window        │       │
│   WAV file)            whisper.cpp /│ actions                        │──▶ LLM (OpenAI-compatible)
│                        Groq…)       │ prompts                        │       │
│                                     └───────────────┬────────────────┘       │
│                                                     │                        │
│                                  TranscriptStore ◀──┴──▶ EventSink           │
└──────────────────────────────────────────────────────────┬───────────────────┘
                                                           │ JSON lines
                                                           ▼ $XDG_RUNTIME_DIR/eco.sock
                                              ┌────────────────────────────┐
                                              │ eco-window (Qt 6, QML)     │
                                              │ floating Hyprland window   │
                                              └────────────────────────────┘
```

The daemon does capture, VAD, transcription, prompts and the LLM call. The
overlay only renders events: it never calls a provider, never holds a secret and
never writes the config — it sends a draft and the daemon validates and saves it.

**The window host.** `eco-window <file.qml>` (`window/`, C++ on Qt 6, built
by `mise run window:build` into `target/window`; on Windows with MSVC and the
Qt `mise run qt:install` puts under `.qt`, whose libraries it copies beside
`eco-window.exe`) loads one entry of
`overlay/` — `shell.qml`, the eco window, `tray.qml`, the tray icon (§12.7),
or `lab.qml`, the control lab — with a QApplication and a
QQmlApplicationEngine, puts the entry's folder on the engine's import path,
and does nothing else. A QApplication (Qt Widgets) rather than a
QGuiApplication because the tray's menu, `Qt.labs.platform`'s `Menu`, falls
back to a widget menu where the platform draws none, as on Windows; it is one
application type for every entry, on both platforms. Beyond that, no logic lives in it beyond the
bridge it hands QML as the module `EcoHost`:

| Type | What QML gets |
|---|---|
| `Host` (singleton) | `pid` (the window's process id), `env(name)` (an environment variable, or `""`), `localPath(url)` (the local path a `file:` URL names, as file dialogs and drops give them), `copy(text)` (the clipboard: on Linux through `wl-copy` started detached, which keeps serving it after the window exits; on Windows through Qt, then `OleFlushClipboard`, which hands the text to Windows at once, so it stays after the window exits or is killed, as `eco stop` kills it — `tests/window.rs` sees it over ssh: killed, the window leaves the text on the clipboard with the flush and nothing without it), `raise(window)` (the window to the front with the keyboard; on Windows over another program's foreground, §15); on Windows only, `hideFromCapture(hidden)` (every window of the process left out of screen capture, `SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE`, now and as each later one shows; `false` shows them again) |
| `LineSocket` | the daemon's socket at `path` — a Unix socket, or on Windows a named pipe — one line per message each way: `send(line)`, `received(line)`, `connected`; it connects again every second while down, and an empty path closes it. On Windows its first line on every connection is `auth <token>`, with its `token` (§10) |
| `TextFile` | the file or directory at `path`, watched: `text` (the file as last read), `changed()` (it changed on disk), `reload()` |

The daemon tells each window it starts where its socket is, in `ECO_SOCKET`
(the Unix socket's path, or the pipe's name), and the token of the run, in
`ECO_TOKEN`; `overlay/Eco/Core/Eco.qml` connects there with that token.
Its windows are QtQuick `Window`s; its Wayland app id is `eco`, the class
`packaging/hypr/eco.lua` matches. It logs to stderr, which the daemon passes on. The QML is shipped and loaded
as files, as `mise run shots` and the control lab load it from the checkout,
and compiled on first use into Qt's disk cache (`~/.cache/eco/qmlcache`), so a
window opens faster from its second start (measured in
[benchmarks.md](benchmarks.md#the-window-on-plain-qt-6-2026-10-09)).

**The QML modules.** The overlay is split by role into QML modules under
`overlay/Eco/`, each folder a module with a `qmldir` that lists its types;
files import modules by name (`import Eco.Kit`), never by relative path:

| Module | Holds | Imports of the overlay |
|---|---|---|
| `Eco.Core` | the singletons `Eco` (the socket and the state), `Theme`, `I18n`, their root `Singleton`, `InputSignal`, the language packs in `i18n/`, and the JavaScript libraries `Draft` (`draft.js`), `Focus` (`focus.js`), `Markdown` (`markdown.js`) and `Packs` (`packs.js`), declared in its `qmldir` | none |
| `Eco.Kit` | the generic components every view composes: buttons, chips, fields, dropdowns, menus, dialogs' frames, tables, panels, the toast stack, the tab rail and command bar, `Stage` | `Eco.Core` |
| `Eco.Live` | the live session: timeline, speech turns, answer and note cards, composer, session control, speaker guesses | `Eco.Core`, `Eco.Kit` |
| `Eco.Sessions` | history, a session's details (attendees, speakers, context, translation), people, cost, the import strip | `Eco.Core`, `Eco.Kit` |
| `Eco.Dialogs` | start, import, rename, speaker assignment | `Eco.Core`, `Eco.Kit` |
| `Eco.Settings` | the settings window: its sections, the screen of each kind of item, and the draft an item is edited in | `Eco.Core`, `Eco.Kit` |
| `Eco.Window` | the composing root: `OverlayPanel`, the eco window, and `SessionView`, which joins the live session to its details | every module above |
| `Eco.Lab` | the control lab | any |

Live, Sessions, Dialogs and Settings never import each other: a piece two of
them share belongs in `Eco.Kit`, and what joins them lives in `Eco.Window`.
`shell.qml` composes `OverlayPanel` with the settings window; `tray.qml` is
the tray icon, on `Eco.Core` alone; `lab.qml` runs `ControlLab`.

`mise run overlay:check` (`scripts/overlay-check`, part of `mise run check`)
enforces this table: it fails on an import a module may not make, a relative
`import "…"`, a file undeclared in its folder's `qmldir`, or a Kit component
never used, directly or through composition, under `Eco/Lab`.

**Rust (daemon) + QML on Qt 6 (overlay).**

- The first version was written in Python for the fastest iteration on the
  prompt, the segmenter and the interface. Once it was stable the daemon was
  rewritten in Rust for a small, always-on process (memory, start-up, no
  interpreter); the socket protocol did not change, so the overlay was
  untouched.
- The daemon's work is almost all I/O (network, STT, LLM); latency is dominated
  by external services, not by the language.
- Two processes (daemon ↔ overlay over a socket) let either side be rewritten
  without touching the other.
- The global shortcut does not start the daemon: it runs `eco window …`, which
  talks to the socket.

### 3.1 Ports and adapters (light hexagonal)

The core (`src/domain/`) knows no external service; it only talks to ports
defined as traits in `src/ports.rs`. No dependency-injection framework —
composition happens in `src/session.rs` from `config.toml`. The crate forbids
`unsafe`.

| Port | Adapters | Why |
|---|---|---|
| `AudioSource` | `pw-record` (any input, or what any sink plays), **WAV file** | The file adapter replays recorded meetings (`--replay <file.wav>`) to tune prompt and trigger and for automated tests. |
| `AudioDevices` | PipeWire (`pw-dump`, `pw-record`, echo cancellation through `pw-cli`); on Windows WASAPI through `cpal`, without echo cancellation | A platform seam (§15): it lists the devices, builds an `AudioSource` for one, and runs echo cancellation on a microphone. Tests compose inputs with a fake. |
| `STT` | Deepgram (streaming), ElevenLabs Scribe (streaming), OpenAI-compatible transcription (`/v1/audio/transcriptions`: LAN whisper.cpp server, Groq, OpenAI) | There is no common real-time STT standard: each protocol needs its own adapter. |
| `LLM` | a single OpenAI-compatible adapter | Covers OpenRouter, OpenAI, Groq, Ollama, llama.cpp — switching = `base_url` + key + model. |
| `EventSink` | Unix socket (overlay), text on stdout | stdout is written only when it is a terminal (`mise run start`); under the user service it is the journal, which never gets transcript, note or answer text. |
| `TranscriptStore` | file in `~/.local/share/eco/`, null (`--no-save`) | |
| `WindowControl` | Hyprland (`hyprctl dispatch`, its event socket); on Windows the windows themselves, told by events on the socket (`HostWindows`) | A platform seam (§15): it focuses a window process's window and leaves its windows out of screen sharing, both by pid. Tests open windows with a fake. |
| `Notifier` | `notify-send` on Linux; Windows toast notifications on Windows | A platform seam (§15): `notify(title, body, urgency)` shows one system notification. The daemon decides when (§12.2); tests decide with a fake. |

Rule: only add a port when two real implementations exist or a test clearly
benefits.

### 3.2 Project layout

```
eco/
├── AGENTS.md
├── Cargo.toml
├── mise.toml
├── README.md
├── docs/
├── scripts/                 # one job per script, consumed by mise tasks
├── src/
│   ├── main.rs              # the command line and its dispatch
│   ├── cli.rs               # session commands over the socket (§11)
│   ├── lifecycle.rs         # start, stop and status of the daemon (§15)
│   ├── setup.rs, skill.rs   # `eco setup`: models, desktop rules, agent skill
│   ├── import.rs            # a file into a session (§7.7)
│   ├── installer.rs         # `eco update` and `eco version`: fetch, verify, install (§13)
│   ├── doctor.rs            # `eco doctor` (§11)
│   ├── config.rs            # config.toml: schema, validation, atomic save (§9)
│   ├── paths.rs             # every path eco reads or writes (§8, §15)
│   ├── session.rs           # adapter composition + orchestration
│   ├── session/update.rs    # the daily release check, the install, the deferred restart (§13)
│   ├── ports.rs             # traits
│   ├── domain/              # segmenter, sessions, assistant, prompts, people, billing
│   ├── adapters/            # audio, vad, stt, llm, socket, terminal, overlay, files
│   └── bench/               # the measurements behind docs/benchmarks.md
├── overlay/                 # the window's QML: shell.qml, lab.qml, Eco/* modules
├── window/                  # eco-window, the Qt 6 program that runs it (§3)
├── packaging/               # Hyprland rules, user service, launcher, icon
├── skills/eco/SKILL.md      # the agent skill, embedded in the binary
├── benchmark/               # the answer benchmark, a self-contained tool
└── tests/                   # i18n checks and the Python-parity fixtures
```

---

## 4. Audio

- **Others:** monitor of the default sink (`@DEFAULT_MONITOR@`) via `pw-record`.
  Optional: `target` to capture only the meeting app's node (avoids
  notifications/music).
- **Me:** default microphone source (`@DEFAULT_SOURCE@`).
- **Audio sources:** the user groups devices into audio sources (`Eu`,
  `Recrutador`…; `[[participants]]` in the config); each device is its own
  stream, labelled with its audio source, so no diarization is needed. One
  audio source may be marked as the user (YOU, under Settings › Audio ›
  AUDIO SOURCES).
  Devices missing from `pw-dump` are reported and skipped, since `pw-record`
  silently falls back to the default device for an unknown target.
- Format: mono, 16 kHz, s16le, in ~100 ms blocks.
- **VAD:** Silero (the If-less ONNX export) on `tract`, pure Rust — no ONNX
  Runtime, no PyTorch. Used to cut silence, detect end of speech, and **only
  send speech segments to the STT** (lowers cost).
- **Outside a session** the inputs are only measured: each 32 ms frame's
  loudness and whether it is speech reach the clients as `signal`, for the input
  traces (§12.4). Nothing is transcribed or stored.
- **Live voices:** while a session records, each input that is not the
  user's sends its speech to a child `eco diarize` (when `eco setup` installed
  the speaker model), which keeps only embeddings in memory; nothing reaches
  disk. When the capture stops (pause or end), each of that input's lines goes
  to the voice that said most of it: a voice close (≥ 0.70) to one the session
  already keeps takes that speaker's label, a single voice keeps the
  audio source's, and others become `Speaker N` — a `diarized` record. The
  voices are kept like an import's, so their speakers get guesses.
- **Echoes:** without headphones, or with headphones loud, the user's
  microphone hears the call and duplicates "their" speech as "me". Headphones
  are recommended; two answers are built in. With `[audio] drop_echoes` (on by default; Settings › General ›
  DROP ECHOES), a line of the user's whose words mostly follow, in
  order, what the others said in the last 20 s (at least 60% of them, and 4
  words or more) is their audio: dropped when it comes after theirs, taken back
  when it came first (an `unheard` record; clients get `transcript_removed`).
  Live sessions only; imports have one stream. With `[audio] echo_cancel`
  (Settings › General › ECHO CANCELLATION, off by default), while a
  session records eco runs PipeWire's `libpipewire-module-echo-cancel` (WebRTC,
  `monitor.mode`) inside a `pw-cli` it owns: the user's first microphone is
  captured through the module's source, which takes out what the **default
  output** plays (the reference cannot be another output). Pausing, ending or
  stopping kills the `pw-cli` and every `eco.aec.*` node goes with it; those
  nodes are never listed as devices. If PipeWire refuses, `echo_cancel.failed`
  is reported and the raw microphone is used. Windows has no echo cancellation
  for eco: `echo_cancel.unavailable` is reported and the raw microphone is used
  (§15). It costs ~3% of one core while a
  session records.
- **Errors show:** a failed transcription or a capture that stops mid-session
  reaches the overlay as an error toast (§12.2), and the terminal when the daemon runs in
  one. A streaming transcription down, and back, is not an error: it is a
  `transcription` event (§10.1), shown under the session capsule (§12.2) and
  printed in the terminal.
- **A device that fails stops only itself:** a device that cannot be opened
  (no default microphone in a remote desktop session) or that stops is a
  `transcription` event with `"code":"capture.down"` for its participant, at
  its first failure only, and is opened again after 1 s, the wait doubling up
  to 30 s, as a streaming transcription is (§5); its first frame after is
  `"state":"back"`. The other inputs keep capturing and transcribing
  meanwhile, and the device's frames go on from where they stopped, so its
  time never restarts. A file replayed is not opened again: its failure ends
  the capture, as `capture.stopped`.

---

## 5. Transcription

| Backend | Pros | Cons |
|---|---|---|
| **Deepgram nova-3 (streaming)** | Phrases ~1.6 s after speech ends (AMI); files over HTTPS at ~17× | Paid, audio leaves the machine, own WebSocket protocol |
| **ElevenLabs Scribe v2 Realtime (streaming)** | Phrases ~1.1 s after speech ends (AMI); files streamed | Paid, audio leaves the machine, own WebSocket protocol |
| **whisper.cpp + Vulkan on the LAN server** — development | Free, private, fast on the RX 9060 XT | Per segment (no real streaming); only at home |
| **Groq/OpenAI (`/v1/audio/transcriptions`)** | Cheap, fast, same format as the LAN server | Per segment (no streaming) |
| **Gemini (audio straight into the LLM)** | Removes the STT | Audio input via OpenRouter unverified — not built until validated |

- `faster-whisper` dropped: CTranslate2 only accelerates on CUDA (NVIDIA).
- **Choice:** the transcription model's `base_url` picks the adapter. An
  `http(s)://` URL is the
  OpenAI-compatible endpoint, segment by segment (up to four segments in
  flight, lines kept in order). A segment that fails is sent again after 1,
  2 and 4 s, no attempt starting later than 15 s after its first, its audio
  held in memory; every error is tried again, since the error carries only
  the provider's text. Then the failure is reported and the next segment
  goes on. While one is tried again the lines after it wait, so a session's
  lines stay in the order they were said. `wss://api.deepgram.com/v1/listen` and
  `wss://api.elevenlabs.io/v1/speech-to-text/realtime` (or a regional host)
  are streaming providers: while a session records, every frame goes to them as
  16 kHz PCM and they decide where a phrase ends (Deepgram: 300 ms of silence,
  or 1 s by `UtteranceEnd`; Scribe: its VAD at 0.6 s); outside a session nothing
  is sent — the local VAD only measures the inputs. The local VAD still cuts
  segments, for imports' diarization. While audio goes out, a WebSocket ping
  goes every 5 s; a connection that brings nothing back — no pong, no message
  — for 15 s is stalled and counts as dropped. Frames flow without a gap while
  a source is heard, so Deepgram's `KeepAlive` is never needed. A connection
  opens at the source's first frame, not before; when the source goes unheard
  (`capture.down`, §4) its connection is closed once the audio held is sent,
  and the next opens at its first frame after it is back: a source not heard
  holds no connection and bills nothing, and its strip says the capture is
  down, not the transcription. A connection
  that drops, closes or is refused while the source is heard is opened
  again after 1 s, the wait doubling up to 30 s while it keeps failing. The
  first failure of an outage is a `transcription` event with `"state":"down"`;
  the rest of it is not reported. When a connection takes audio again — its
  handshake done — the outage ends with `"state":"back"`, how long it lasted
  and the seconds of audio it dropped (§10.1); a connection that fails after
  that is a new outage. The audio heard while no
  connection takes it — down, waiting or being opened — is kept in memory,
  up to five minutes (about 9.6 MB), and goes first to the next connection,
  whose phrases are timed from the first frame it receives. Past five
  minutes the oldest audio is dropped, and the seconds dropped are said by
  `back`; dropped by an outage still under way when the capture ends, they
  are a `transcription.failed` error. Outages are not written to the session
  log, which keeps no errors either. An import
  streams to Scribe with waiting frames joined into chunks of up to a second
  (frame by frame it refuses them as too frequent); Deepgram streams only as
  fast as the audio plays, so an import posts each segment to the same path
  over HTTPS with `utterances=true`. The config window's presets fill both;
  their model lists come from Deepgram's `/v1/models` (streaming ones) and
  Scribe's one realtime model.
- **Words as they are said:** a streaming provider's interim results
  (Deepgram `is_final: false`, with the finals held so far; Scribe
  `partial_transcript`) reach the clients as `transcript_partial {who, name,
  text}` and are never stored. The open session shows, dimmed below its lines,
  what each speaker is saying; it gives way to the line when the phrase ends,
  and goes when the session pauses or ends. Segment-by-segment STTs show a line
  only when it is finished.
- **Phrases:** the OpenAI-compatible adapter asks for `verbose_json`, whose
  timed segments become phrases (whisper.cpp, Groq and `whisper-1` give them);
  a server that refuses it (400, e.g. `gpt-4o-transcribe`) is asked for plain
  `json` from then on, the whole segment one phrase. Imports keep a line per
  phrase; a live session joins a segment's phrases into one line.
- **Language:** `stt.languages` lists the codes the user works in (`pt`, `en`,
  `ja`…, plus `auto`, which leaves detection to the server per segment) and
  `stt.language` is the one new sessions start in. Each session is transcribed
  in its own language: the overlay's dropdown offers exactly that list, each
  code with its native name from Qt, and switches the open session mid-meeting
  (`session.language <json>` with `{"id","language"}` on the socket; a
  `language` record), changing only that session's transcriber; the config
  window edits the list and the default. The LAN server runs with `-l auto` so
  each request decides.
- **Transcribers:** each input is captured once, into a hub; a transcriber
  reads it for one model in one language — the model of a session's kind, in
  the session's language — and every recording session listening with the same
  pair shares it. One starts with the first session that needs it and stops
  with the last; another pair runs beside it on the same audio. Starting,
  pausing or ending a session never restarts the capture while another
  records.

---

## 6. Answers

An **action** is a user-defined request run against the transcript. "Suggest a
reply" is just the `ask` action; a recruiter's technical question can call a
`probe` action that suggests grounded questions to ask back, each with why it
matters.

- **Provider:** any OpenAI-compatible API (OpenRouter, the LAN `llama-server`,
  Groq, Ollama), SSE streaming. Models are registered once in `[[models]]`
  — a name, a type (`chat` or `transcription`), the provider, its key and the
  provider's id — and the assistant, the reviewer and actions name the chat
  model they use, `[stt]` the transcription model sessions use by default. A
  model may serve session kinds (`kinds`): each kind has at most one model of
  each type, and a session of that kind transcribes with it and answers with
  it instead of the assistant's model — questions, actions that name no model,
  and the reviewer's pass; an action that names a model always answers with
  it — so an `idea` session can run on local models. If such
  a model is down, its sessions say so; nothing falls back to another model.
  Each model's provider-specific fields live in its `extra` and are sent as-is.
  Reasoning a model streams before its answer
  (`reasoning_content` from llama.cpp and LM Studio, `reasoning` from
  OpenRouter) is shown while it lasts and never kept. A request fails only when
  the model sends nothing for 120 s (10 s to connect), so a long answer that
  keeps arriving is never cut off. A model that cannot be set up — its key
  missing, say — leaves the rest of eco working: audio sources, actions and
  capture are configured as usual, every client is told why
  (`model.unavailable`, with the model's name, and likewise `stt.unavailable`
  for transcription and `context.unavailable` for an unreadable context file),
  and an action fails with that reason. `completion.failed` carries the
  answer's `id` and why. A reply that is not a stream is read whole: the answer
  when it is one, otherwise the server's own error (with a hint when the base
  URL has no path, as servers usually serve under `/v1`); an answer that ends
  without a word fails too, rather than leaving an empty card.
- **Rules:** `rules` holds the rules every answer follows (language,
  brevity, never putting facts in the user's mouth); Settings › Prompts
  edits them. An action's prompt and format override them, so a new action states
  its own behaviour instead of changing the shared rules.
- **Reviewer:** with `[reviewer] enabled`, every answer — an action or a free
  question — is first drafted unseen by its model, then the reviewer's `model`
  reads the same conversation, the draft and the
  review `prompt`, and only its rewrite streams into the card and is kept. It is
  there to cut what the transcript, notes and context do not support. A failed
  draft fails the answer before the reviewer is called. `verbose` also streams
  the draft (`suggestion_draft`) and keeps it on the answer, shown dimmed above
  it, to inspect what the reviewer changed. A reviewer whose model cannot be set
  up fails answers with that reason until it is fixed or turned off.
  In the window, Models' DEFAULT FOR REVIEW picks `model`, General turns
  `enabled` and `verbose` on, and Prompts holds `prompt`.
- **Default model:** `[llm] model` names the assistant's; an action's `model`
  names another (e.g. a slower, better model for a deep action). Only the models
  something uses are set up. Latency measurements: [benchmarks.md](benchmarks.md).
- **Prompt, built for the provider's cache:** a request is a conversation that
  only grows at its end. The system message (framing, `rules`, user context)
  comes first, then every earlier turn rebuilt byte for byte — the speech heard
  since the turn before, the exact request text (kept on each suggestion), and
  the kept answer — and finally the new speech and the new request. OpenAI,
  Gemini, DeepSeek and llama.cpp reuse such a prefix on their own; for Anthropic
  models the adapter adds `cache_control` at the end of the system message and
  of the last kept answer. Removing a suggestion changes the prefix only from
  that turn on. When the context outgrows `max_context_chars`, its start jumps
  forward until it fills 60% of the budget, so it moves once per overflow.
  Each answer reports prompt tokens, cached tokens and cost when the provider
  sends them (`stream_options.include_usage`).
- **Context of a request:** the session's kind and title, its speech and kept
  answers within `max_context_chars` (see above), plus the action's prompt and
  format or the user's question.
- **Context slots:** a slot is a name, files and the kinds that start with it
  on. A session nobody chose for has the slots of its kind on; the user turns
  them on and off per session (a `context` record, `{slots}`;
  `session.context <json>` with `{"id","contexts"}`, and `session_context` to
  clients; `eco context <id> --add/--remove` from the CLI). A request's system
  message carries the global context, then each slot on under `## <name>`.
  Files are read when the setup is built: an edit counts once the settings are
  saved or the daemon restarts; an unreadable one is reported
  (`context.unavailable`).
- **Free questions:** the composer sends `ask <text>`; the question and its
  answer form one removable card (action `chat`), labelled ASK where a skill's
  card carries the skill's name.
- **Notes:** the composer's note button, or Shift+Enter, sends `note <text>`
  instead: a fact the user writes during the conversation, kept in the timeline
  as a removable note and carried, among the lines heard, by every later request
  as `Nota do usuário: …`. It asks nothing; the next action or question uses it.
- **Translation:** each line, once saved, and each answer, once finished, can
  be translated below it, dimmer, by the translation model — a kind's own
  (`translates` on a chat model), `[translation] model`, or the assistant's.
  A session translates nothing until the user turns it on in that session:
  its TRANSLATION line (`TRANSLATION: OFF`) picks a language other than its
  own, or OFF (`session.translation <json>` with `{"id","language"}`, ""
  for off; a `translation` record; `session_translation {session,
  translating}` to clients). The languages are the daemon's list
  (`LANGUAGES` in `src/domain/prompts.rs`, sent as `language_codes` in
  `snapshot`); a code not in it is refused (`translation.unknown`), and so is
  the session's own language (`translation.own`). Turning it on, or opening a stored
  session that translates, translates what it holds and lacks; a language
  translated before shows again at once. Each translation is one request with
  only the text — no transcript — so it stays fast and cheap; up to three run
  at once, apart from transcription and answers, and none is asked twice. Kept
  as a `translated` record (`{at}` for a line, `{id}` for an answer, with
  `language` and `text`) and sent as `translated {session, at|id, language,
  text, ms}`; a line corrected or removed loses it. An answer's card also
  translates it on demand (`entry.translate <json>` with
  `{"session","id","language"}`): into the session's translation language, else
  `language`, the interface's.
  Translations never reach the context of later requests. `eco translate <id>
  --lang <code>` or `--off` from the CLI; `eco show` carries them.
- **One at a time per session:** each session streams one answer at a time: an
  action triggered while another of the same session streams waits for it and
  is then sent with that answer in its context; a newer one replaces the one
  waiting. Sessions answer side by side — the live one while a stored one is
  summarized — up to `[llm] concurrency` answers at once (8 by default); past
  it, an answer waits for one to end before its request is sent. Saving the
  config stops only the answers whose model or reviewer changed; the rest
  finish.
- **Hooks:** an action may have a **hook**: a
  shell command run (`sh -c`, on Windows `cmd /S /C`, §15; at most 60 s) with only the session it answered
  for, in `ECO_SESSION` (its id) and `ECO_TITLE`; the other system reads the
  rest with `eco show "$ECO_SESSION"`. With `hook_auto` it runs once each answer
  is complete; either way the answer's card has a send button that runs it
  (again), and says SENDING, SENT or NOT SENT (`hook.send <json>` with
  `{"session","id"}`; `hook_started`, `hook_sent`, or a `hook.failed` error with
  the last line the command wrote to stderr).

### 6.1 Triggers

The trigger is manual: a global Hyprland shortcut per action sends `action
<name>` to the socket through `eco window action` (on Windows the daemon's own
shortcut sends the same line, §15), and the overlay shows one button per
action.

```lua
o.bind("SUPER + ALT + A", "eco: ask", "eco window action ask")
o.bind("SUPER + ALT + Q", "eco: probe", "eco window action probe")
```

An automatic trigger and a running recap are not built; see Open questions.

---

## 7. Sessions

A session moves through `recording ⇄ paused → ended`, and a stored interrupted
or ended session can be resumed. A session's kind is translated by the overlay;
any other label shows as written.

- **Start:** `session.start` takes an optional title, the kind, the session's
  language (from `stt.languages`) and optional tags. The session is transcribed
  in that language by its kind's model. Without a title it is titled by its kind
  and the time the start dialog opened ("Meeting 14:05", in the interface
  language). §12.3 has the dialog.
- **Several at once:** a session can start, or a stored one resume, while
  others record — a call and a class, an interview in two languages. Each is
  live until it ends. Each overlay window shows its own session, and its
  composer and buttons address only that one; the session a window showed last
  is the one the shortcuts and commands without a session address. All of them
  hear the same inputs: sessions alike share a transcriber, and each other model
  or language runs its own (§5, Transcribers).
- **Pause/resume:** pausing stops the session's transcription and its clock;
  the session stays open, the others keep recording, and actions still work on
  what was said. A session's duration is the time it recorded (or imported, up
  to its file's last line): pauses, and a daemon that was down, do not count. A
  live session carries `active_s`, the seconds of its earlier runs, and
  `running_since`, when the current run began (null while paused); the capsule
  and the live rows of SESSIONS add the time since.
- **End:** closes the session; the session the window showed before it shows
  again, or the window returns to its start screen.
- **Timeline:** speech and suggestions in order. Each suggestion has an id and
  can be removed; a removed suggestion disappears and leaves the context.
- **Stored sessions keep answering:** questions and actions on a stored
  session stream into its timeline and are appended to its log, so minutes or a
  summary are just actions. Stored sessions asked about stay in memory beside
  the open one, eight at most: past that, the least recently used one with no
  answer under way leaves, its log still holding all of it. An interrupted one
  can be resumed, and an ended one reopened, to reuse its context — either
  records again in its own language, appending to the same log.
- **Edits:** its title and kind can be edited, open or stored (a `meta` record,
  which does not count toward the duration). The user can remove any line
  (`unheard`, as a dropped echo) or correct it in place: an `edited` record, so
  the context, the export and the CLI read the correction.
- **Delete:** an interrupted or ended session can be deleted; deletion removes
  its log and associated voice links. A live session, recording or paused,
  cannot be deleted: the daemon refuses with `session.live`. A log the disk
  will not remove stays listed, and the daemon reports `session.delete_failed`.
- **Resilience:** an unreadable log is skipped in the list with a warning, and a
  command that fails is reported to the clients without dropping the socket.

### 7.1 Search

`sessions.search <text>` answers `sessions_found` with the ids, newest first: the
sessions whose title, tags, lines, notes, questions or answers hold the text, in
any case or accents, as they read now (removed and corrected lines count as
they are).

### 7.2 Speakers

A line keeps the label it was heard as (the audio source, or the name a
transcript gave); naming a speaker in a session is a `speaker` record. Two
labels given the same name merge on screen, and the label itself, or no name,
restores it. Names reach every reading: the timeline, the prompt, the export and
the speakers of the session's summary.

### 7.3 Voices

A diarized speaker's voice (the mean of its embeddings) is kept in
`~/.local/share/eco/people/voices/<session>.json`, apart from the session.
eco never names a speaker on its own: a speaker whose voice scores ≥ 0.70
against someone (cosine to the mean of their voiceprints; on AMI the closest
wrong person scored 0.52) carries a **guess**. The user confirms it (the speaker
becomes that person, as naming them by hand) or clears it (a `guess_dismissed`
record `{label, person}`: that guess is not made again for that speaker). People
from 0.40 are suggested when a speaker is named by hand.

### 7.4 People

The people the user names live in `people/<id>.json`: a name, an optional color,
and up to 16 voiceprints, each tied to the session speaker it came from. Naming
a speaker as a person (new or known) adds a `person` record (`{label, person}`)
and a `speaker` record with their name, and their voice joins the person's.

- Renaming or merging people appends records to every session that names them;
  undoing an assignment takes the voice back; forgetting a person deletes their
  file and the voices kept for their speakers, and sessions keep the name; a
  person id sessions name but no file holds is forgotten the same way.
- A speaker without a kept voice (a live session, a transcript) can be someone
  known too — linked, without teaching eco a voice. Naming a speaker creates or
  selects a person for live and imported sessions.
- An assignment can take one line alone (`person.assign_line`), or every line of
  a session at once (`person.assign_all`, from the CLI only).
- A person's color changes across sessions; a local speaker color is saved in
  the session log.
- A session can also name people who never spoke: an `attendee` record links or
  unlinks their person id independently of speaker labels. SESSIONS and the CLI
  filter on the union of the people added and identified speakers.

### 7.5 Tags

A tag is the text given with its spaces trimmed and collapsed; an empty one is
`tag.invalid`. Tags are one whatever their case, and a tag keeps the casing it
was first written in: tagging a session `acme` when another is tagged `Acme`
gives it `Acme`. Each change appends a `tags` record with the session's whole
list (`{"tags": [...]}`), so a log from before tags has none. Renaming a tag or
deleting it applies to every session carrying it — a `tags` record in each of
their logs — renaming into a tag that exists joins the two under the new
spelling, and a tag no session carries is `tag.not_found`. Listed sessions, the
session shown, every live one and a session read back carry `tags`; search
matches them; `eco sessions --tag` filters by one, combined with `--kind`,
`--person` and `--search`.

### 7.6 Export

A session's transcript as WebVTT — one cue per line, the speaker in a
`<v Name>` span, times from the session's start. Lines keep only their start,
so a cue ends at the next line or after its words (0.35 s each, 1–10 s);
importing the export gives the same lines, speakers and starts back. From the
CLI (`eco export <id> > session.vtt`) or the session's detail (COPY VTT in its ⋯
menu, to the clipboard).

### 7.7 Import

A WebVTT transcript (`.vtt`, recognised by its `WEBVTT` header) becomes a
session as it is — one line per cue, speakers from Teams' `<v Name>` spans or
Zoom's `Name: text`, times from the cues, numeric character references decoded,
no STT. Any file ffmpeg decodes (mp4, mkv, webm, m4a, mp3, ogg, flac, wav…)
becomes a session — from the overlay (§12.3), or with
`window.call {"call":"import","path":"<path>"}` on the socket (§10.2), which
opens the import dialog with the file filled in.

Both get the path as `file:<path>` with `-protocol_whitelist file`: a name that
begins with `-` is never an option, and nothing in the file makes them open a
URL. ffmpeg decodes it to 16 kHz mono through a pipe (never to disk), ffprobe gives
its length, and the VAD and the configured STT transcribe it as fast as they
go, one line per phrase the STT times, each at its time in the recording.
The lines start as the audio source chosen at import; meanwhile a child
`eco diarize` (WeSpeaker CAM++ from `eco setup`, loaded only for the import)
embeds the same speech, and when the file ends each line goes to the speaker
who talks most over it — `Speaker 1`, `Speaker 2`… in the order they first
speak, a `diarized` record, renamed like any speaker. One voice found leaves
the audio source. Measurements in [benchmarks.md](benchmarks.md). The session
is `importing` until the file ends, then `ended`; it can be stopped
(`import.cancel`), and what was transcribed stays. One import at a time, beside
live capture; an import a daemon left behind ends at the next start.

The session starts when the recording did, not when it was imported: at the
date the user gives (`--date`, or the import dialog), else the `creation_time`
tag ffprobe reads in the same call as the length (the container's, else a
stream's), else the file's modification time — the only date a WebVTT
transcript has. Its log is named by that date, SESSIONS files it under that
day, and its states, transcribers and speaker turns run on the recording's
clock; what the import cost is kept at the time it was paid.

### 7.8 Cost

Each answer, reviewer pass and translation appends a `spent` record with what
its provider reported (`usd`, null when it reported none); a removed answer was
still paid. Whenever a session starts transcribing with another model, on
another number of inputs, or at another price, a `transcriber` record says so
(`model` null for a WebVTT import, which nothing transcribes; `per_minute`, the
model's `price_per_minute` then, when it has one). `inputs` counts the inputs
heard: one whose source cannot be heard (`capture.down`, §4) streams nothing, so
it leaves the count when it goes down and comes back into it when it is heard
again, each time with a record of its own; capture is not told, so no running
transcriber restarts.

A provider that bills by request names it: Deepgram's `dg-request-id` header on
the WebSocket upgrade, and `metadata.request_id` in the reply to each segment of
an imported file. A streamed request is shared by the sessions its transcriber
served, each by how long it listened while the request was open. As the request
opens, and each time a session starts listening to it again, that session gets a
`listening` record (`request`, `model`, `at`). When the request closes (the next
opens, or the transcriber stops), each of them gets a `billed` record
(`request`, `model`, `seconds`, `share`, and `per_minute` when the model has a
price). A segment's request is closed as soon as it is answered: an import's is
the import session's alone, for the segment's seconds; a live one is shared
alike by the sessions listening.

A request the daemon never closed — it stopped or crashed while the request was
open — is closed at the next setup: each session's seconds are read from its
`listening` records, each stretch lasting until the session paused, ended or
changed models, or else until its last line, and the sessions share it by them
(alike, if none has any). A request that any session has a `billed` record for,
or that a transcriber has open now, is not closed again.

The provider is then asked what each billed request cost — Deepgram's
`GET /v1/projects/{project}/requests/{request}` → `response.details.usd`, the
project being the key's first — off everything else: 5 s after it closes, then
after waits about as long as the request has been closed, from 5 s up to an
hour, for a day. A failed ask is asked again like an unlisted one. The answer is
a `spent` record (`for` `transcription`, `request`, `usd` × the share; null when
the provider cannot tell, with `unknown` saying why: `no_scope` for a key
without the `usage:read` scope — Deepgram's 403 — and `unreported` for a
request not listed a day after it closed). The queue is the logs: a `billed`
record with no `spent` record is asked for again at every setup — the next
start, a saved config, an import's end — the waits going on from the request's
age. An answer's or review's `spent` record names the answer (`answer`).

A session's `cost` is read from its log: `llm_usd` sums the completions'
reported costs, `transcribed_s` gives the seconds of audio each model heard —
its run time once per input heard, as every input streams on its own — and
`transcription_usd` sums the requests' reported costs. `price_per_minute` is
optional and empty by default: when the user gives one, it prices only what no
reported cost covers — a billed request not priced (yet), or the time of a model
whose provider names no requests (ElevenLabs, OpenAI-compatible endpoints). It
prices them at the `per_minute` the log kept with that request or that stretch
of audio, so changing a price later does not change past sessions; only what
the log kept no price for — a log from before eco kept prices, or audio heard
while the model had none — takes the price the model has now. A model with
billed requests is costed by them alone. `llm_unknown` and
`transcription_unknown` say a part is left out of `total_usd`: a provider
reported no cost (or not yet) and no price stands in, an answer has no `spent`
record (a log from before eco recorded costs), or a log from before eco recorded
its transcriber.

Clients get `cost` with each listed session and read back; `session.cost <id>`
answers a `session_cost` event, also sent whenever a completion or a request
adds to a session's cost. It adds `parts` — what answers, reviews, translations
and transcription each add up to, of those the session has — and `items`, every
charge, newest first and those of no one moment (the time a model heard with no
request billed, the audio of an unrecorded transcriber, the answers that kept no
cost) first: `for`, `at`, `model`, `usd`, `estimate` (priced at
`price_per_minute`), `seconds` (transcription), `prompt` (an answer or review
still in the timeline), `count` (answers that kept no cost) and, when `usd` is
null, `unknown`: `unreported`, `no_scope`, `pending` (a billing provider has not
told it yet), `no_price`, `unrecorded` or `untracked`. `eco sessions` lists each
session's cost; the overlay shows one only when asked (§12.3).

---

## 8. State on disk

Never audio. Everything below is text the user can read. Every path
below is built in `src/paths.rs` (§15).

| Path | Holds | Written by |
|---|---|---|
| `~/.config/eco/config.toml` | the configuration (§9) | the daemon, from the config window's draft; hand edits work too, but comments are not kept |
| `~/.local/share/eco/sessions/<start>-<title or kind>.jsonl` | one session, append-only JSON Lines; a second session with the same start second and title gets `-2`, then `-3`, before `.jsonl` | the daemon |
| `~/.local/share/eco/people/<id>.json` | a person: name, color, voiceprints (§7.4) | the daemon |
| `~/.local/share/eco/people/voices/<session>.json` | the voices of a session's diarized speakers (§7.3) | the daemon |
| `~/.local/share/eco/models/` | the Silero VAD and WeSpeaker CAM++ models | `eco setup` |
| `~/.local/state/eco/update.json` | the last check for a release that GitHub answered (§13): `checked_at` (Unix seconds), `latest` (the tag's version) and `notes` (the release page) | the daemon |
| `$XDG_RUNTIME_DIR/eco.sock` | the socket (§10) | the daemon |
| `$XDG_RUNTIME_DIR/eco.token` | the token for this run: the windows' (§9), and on Windows every client's admission (§10), `0600`, removed when the daemon exits | the daemon |

The config, session logs, people and voices are the user's alone: eco creates
their files `0600` and the directories it makes for them `0700`. A file eco
rewrites (the config, a person, a session's voices) comes back `0600` on its next
save; a session log made before keeps the mode it had.

On Windows the same files sit elsewhere, and have no modes: under the user
profile, each inherits an access list that admits only the user, the system
and administrators (§15).

| On Linux | On Windows |
|---|---|
| `~/.config/eco/config.toml` | `%APPDATA%\eco\config.toml` |
| `~/.local/share/eco/` (sessions, people, voices, models) | `%LOCALAPPDATA%\eco\` |
| `$XDG_RUNTIME_DIR/eco.sock` | the named pipe `\\.\pipe\eco-<user>`, no file |
| `$XDG_RUNTIME_DIR/eco.token` | `%LOCALAPPDATA%\eco\eco.token` |
| `~/.local/state/eco/update.json` (`$XDG_STATE_HOME/eco/` when that is set) | `%LOCALAPPDATA%\eco\update.json` |
| the journal of `eco.service` | `%LOCALAPPDATA%\eco\daemon.log`, the output of the daemon `eco start` started, emptied at each start |
| the line `eco setup` adds to `~/.config/hypr/bindings.lua` | nothing: eco does not start at login; `eco setup` removes the `Startup\eco.cmd` an earlier eco wrote |
| the launcher `eco.desktop` the package installs | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\eco.lnk`, written by `eco setup`: it runs `eco start` through `conhost.exe --headless`, with the icon of `eco-window.exe` |
| the icon `eco` the package installs, which names `notify-send`'s notifications | the registry key `HKCU\Software\Classes\AppUserModelId\This-Is-NPC.eco`, written by `eco setup` and by the daemon before a notification: `DisplayName` `eco`, `IconUri` the installed `share\eco\eco.ico` |

A session log holds `session`, `state`, `speech`, `suggestion`, `removed`,
`meta`, `speaker`, `diarized`, `person`, `attendee`, `unheard`, `edited`,
`line_person`, `transcriber`, `listening`, `billed`, `spent`, `tags` records, and also
`language`, `translation`, `translated`, `context` and `guess_dismissed` (§5–§7).
`--no-save` keeps it in memory. A removed suggestion is a `removed` record, and
it leaves the context of later actions as well as the screen.

A daemon that stops mid-session leaves it open; the next start marks it
`paused` and opens with no session. A stored session left open that no daemon
holds is listed and shown as `interrupted`, never reopened by itself.

---

## 9. Configuration

`~/.config/eco/config.toml` (written by the config window; hand edits work too,
but comments are not kept). No file is the empty config: no model, no
participant, no action, the defaults for the rest — in the system language
(`sys-locale`: `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG` on Linux, the
user's display language on Windows): `ui.language` its pack (`pt-BR`,
`ja-JP`, `en-US`), `stt.language` its code, first after `auto` in
`stt.languages`, and `rules` the default rules in it; any language without a
pack starts in English. A file that exists is read as written: a field it
leaves out takes the default below, whatever the system says. The daemon
starts on it, so the window opens and its settings write the file at the first save; a
file that does not parse or validate still stops the daemon. `stt.model` and
`llm.model` may be empty, naming no default: a session that needs that model
is told `no transcription model is chosen`, as an `stt.unavailable` error. A
question or an action that no chat model answers in a session of its kind is
refused before anything is asked, with `chat.none` (`{kind}`); one that waited
while the setup lost its model fails with `completion.failed`. Turning
translation on, or translating an answer, where no model translates the kind
is refused with `translation.none` (`{kind}`); a session that still translates
after its translator went asks for nothing more. Every field:

```toml
kinds = ["meeting", "conversation", "other", "idea"]  # what a session can be
rules = """
- Responda no idioma da transcrição.
- Seja curto: o usuário pode estar lendo enquanto a conversa continua.
"""                                         # every answer's rules; actions override them

[stt]
model = "deepgram"                          # the transcription model sessions use by default
language = "pt"                             # one of `languages`
languages = ["auto", "pt", "en", "es", "ja"] # offered by the overlay's dropdown

[llm]
model = "gemini"                            # the assistant: a registered model's name
max_context_chars = 60000                   # transcript + answers per request (~15k tokens)
concurrency = 8                             # answers running at the same time, all sessions

[translation]                               # optional: the model of sessions that translate
model = "gemini"                            # a chat model; the assistant's when absent

[reviewer]                                  # optional: rewrites every answer before it shows
enabled = true
model = "grok"
verbose = false                             # true: show and keep the draft it rewrote
# prompt = "…"                              # what it is asked; a default cuts unsupported facts

# A model, registered once: [stt] names a transcription one; [llm], [reviewer]
# and actions name chat ones; `kinds` gives it the sessions of those kinds.
[[models]]
name = "deepgram"
type = "transcription"
base_url = "wss://api.deepgram.com/v1/listen" # streaming; or an OpenAI-compatible URL
model = "nova-3"
api_key_env = "DEEPGRAM_API_KEY"
# price_per_minute = 0.0077                 # optional: USD per minute, only where the provider reports no cost

[[models]]
name = "whisper-lan"
type = "transcription"
base_url = "http://192.168.0.200:8081/v1"   # LAN whisper.cpp server
model = "whisper-1"
kinds = ["idea"]                            # idea sessions transcribe here

[[models]]
name = "qwen-lan"
type = "chat"
base_url = "http://192.168.0.200:8080/v1"   # LAN llama-server
model = "qwen3-14b"
kinds = ["idea"]                            # and answer here unless the action names a model

[[models]]
name = "gemini"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"          # or api_key_omapass = "<account>", or api_key_credential = "<name>"; one
model = "google/gemini-3.5-flash-lite"      # the provider's id
extra = { reasoning = { effort = "minimal" } }

[[models]]
name = "grok"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
api_key_omapass = "openrouter"
model = "x-ai/grok-4.20"
extra = { reasoning = { enabled = false } }

[audio]
drop_echoes = true                          # the user's lines that repeat the others' are dropped
echo_cancel = false                         # PipeWire echo cancellation on the user's microphone

[context]
files = ["~/.config/eco/context.md"]       # global: sent with every request

# A context slot: files sent only while a session has it on. Sessions of its
# kinds start with it on; the user turns slots on and off per session.
[[contexts]]
name = "entrevista"
files = ["~/cv.md", "~/projetos.md"]
kinds = ["meeting"]

[ui]
language = "auto"                           # interface language pack, or auto
hide_from_share = false                     # eco's windows hidden from screen shares (and screenshots)
tray = false                                # an icon in the system tray (§12.7)
notifications = false                       # system notifications while no eco window is active (§12.2)

[update]
check = true                                # once a day, ask GitHub for the latest release's tag (§13); false never asks

[colors]                                    # trace colour per device (optional)
"@default-input" = "#ffb000"

# Windows only (on Linux the keys are Hyprland binds, packaging/hypr/eco.lua):
# keys -> the `eco window` command they run; without the table, the seven
# Win+Alt keys of §15, and an empty table binds none. Read when the daemon starts.
[shortcuts]
"Win+Alt+A" = "action ask"                  # Ctrl, Alt, Shift, Win; A-Z, 0-9, F1-F24, Space…
"Ctrl+Alt+M" = "sessions"

# An audio source is a name in the conversation; it may hold several PipeWire
# devices (node names, or @default-input / @default-output). A device has one owner.
[[participants]]
name = "Eu"
user = true
devices = ["@default-input"]

[[participants]]
name = "Recrutador"
devices = ["@default-output"]

[[actions]]
name = "probe"
prompt = "Sugira perguntas para devolver, cada uma com o porquê."
format = "2 a 4 itens numerados."
model = "grok"                              # a registered model; the assistant's when absent
hook = "~/bin/crm-push --board leads"       # run with ECO_SESSION and ECO_TITLE (optional)
hook_auto = true                            # after every answer; else from its send button
```

The config window saves one item or value at a time, sending the saved config with
that item's change whole (§12.6); the daemon validates it, writes
`config.toml` and restarts capture. A restart first stops the running pipeline
and its capture or monitor task completely, then starts the new one, so a save
never leaves a second `pw-record` duplicating frames or transcribing twice.

A hook runs a shell command, a context file is sent to a model, and a model's
key is sent to its `base_url`, so only the user adds them. The daemon starts
every window with one random token per run in `ECO_TOKEN`, and the window's
`config.set` carries it. A `config.set` from any other socket client that adds
or changes an action's `hook`, adds a file to `[context]` or a `[[contexts]]`
slot, or adds a model or changes a model's `base_url`, `api_key_env` or
`api_key_omapass`, is not saved: the daemon holds it, announces
`config_pending` with its `id` and each hook command, file path, and model's
address and key source (a variable's or an omapass account's name, never a
key), and answers the error `config.pending`. The window shows them in a
dialog; APPROVE sends `config.approve <token> <id>` and saves it, REJECT sends
`config.reject <token> <id>`, which drops it with the error `config.rejected`.
Approving or rejecting without the token answers `config.not_window`; naming
another id than the held change's answers `config.stale` and saves and drops
nothing. The id is a counter that starts at 1 for each run of the daemon, so a
held change never has the id of an earlier one. One change is held at a time,
and while it waits only the window changes the config: any other client's
`config.set`, risky or not, answers `config.busy` and is not saved, so the
dialog never changes under the user's pointer. The window's own save drops the
held change. Every other change from any client is saved at once, as before.

The daemon also writes the token to `$XDG_RUNTIME_DIR/eco.token`, mode `0600`,
once it holds the socket, and removes it when it exits; a file left by a killed
daemon is replaced by the next one, and a second daemon, refused the socket,
leaves the running one's file alone. On Windows the same token admits every
client of the pipe (§10). A window the daemon did not start, `mise run overlay`
beside `mise run start -- --headless`, reads it into `ECO_TOKEN`; started
before the daemon, or kept across a restart, it has no token or an old one, and
its saves of these fields are held like any other client's until it is started
again.

The token keeps a client that only talks to the socket, such as an agent
driving it with a prompt it was handed, from saving or approving these changes
itself. It does not stop a program running as the same user that wants to:
such a program can read `eco.token`, exactly as it can read a window's
environment in `/proc/<pid>/environ`, and it can edit `config.toml` directly.
Neither the file nor the environment is a secret from the user's own
processes.

### 9.1 Secrets

Secrets come from environment variables: `OPENROUTER_API_KEY`,
`DEEPGRAM_API_KEY`, `ELEVEN_LABS_API_KEY`, `GROQ_API_KEY` — or from omapass, the
user's password plugin over gnome-keyring: `api_key_omapass = "<account>"`
instead of `api_key_env` makes the daemon run `omapass get -- <account>` (piped,
never logged) each time it builds that provider. omapass is found on `PATH` or
in `~/.config/omarchy/plugins/io.github.this-is-npc.omapass/bin/`, and is
optional: the `config` event says whether it is there and where its install page
is, and without it the config window's OMAPASS is off, with an info button that
opens that page. The window's KEY FROM lists its passwords (not its Nostr
keys) among the key names it offers, never their secrets. Since the daemon may be launched from a
Hyprland shortcut (which does not read `.bashrc`), set them in
`~/.config/uwsm/env`. Only the daemon reads the keys; the shortcuts do not need
them. During development, mise loads them from `.env` at the project root
(git-ignored).

On Windows a key may come from the Windows Credential Manager instead:
`api_key_credential = "<name>"` makes the daemon read the generic credential
whose target is `eco:<name>` each time it builds that provider
(`src/adapters/credentials.rs`, through the `keyring-core` crate and its
Windows store, `windows-native-keyring-store`; the
credential is the UTF-16 password the Credential Manager's own dialog writes,
so one added by hand reads the same). `eco key set <name>` stores one, with
`persistence` local to the machine, reading the key hidden from the console
(`rpassword`) or from stdin when it is not a terminal; `eco key remove <name>`
deletes it; neither talks to the daemon. The daemon lists them by name for the
`keys` command (§10.2) through the store's search (its `search` feature,
`Store::search` with the pattern `^eco:`, then each entry's `get_attributes`
for its `target_name`): eco asks for no password, the crate zeroes each one
the enumeration brings before it lets go of it, and a credential gone between
the search and the read is left out. A credential that is not there fails
the model with `stt.credential_missing` or `model.credential_missing`
(`params` `{"name","credential"}`), and a `models` listing with
`models.credential_missing` (`{"credential"}`), each saying how to add it. The
`config` event's `credential_manager` is true only on Windows: the window then
offers CREDENTIAL MANAGER in place of OMAPASS, its credentials listed by name
and a name typed by hand taken too, and never sees the key. A logon session without the Credential Manager — an ssh
login by key — gets `NoStore`, said as such; the tests that need the real
store stop there with a line on stderr. On Linux the field parses and the
provider fails with the message that names omapass and the environment.

The window's KEY FROM picks a key by name from one list: the `keys` command
(§10.2) answers the names of the daemon's environment variables that may hold a
key and of the Credential Manager's `eco:<name>` credentials, and the `omapass`
command the accounts. Only names cross the socket; no value is kept or sent.
A variable may hold a key when it is named `ECO_<NAME>` — the convention
for a key of the user's own — or is one of the presets' variables (`PRESETS` in
`src/session.rs`) and is set, even empty (the provider build then says it is
empty). The `ECO_` variables eco itself sets or reads for its own work
(`ECO_SOCKET`, `ECO_TOKEN`, `ECO_WINDOW`, `ECO_SESSION`, `ECO_TITLE`, …) are
`OWN_VARIABLES` in `src/session.rs`, one list, and are never offered. The daemon
reads its environment when it starts, so a new `ECO_` variable is listed after
`eco restart`: on Linux set it in `~/.config/uwsm/env`, on Windows among the
user's environment variables. A credential added on Windows is listed at once,
since the store is searched on each request. `eco key list` prints the three
lists (§11).

The `models` command (§10.2) lists a provider's models with a key, so it reads a
key only where a saved provider would: the key source of a saved model, sent to
that model's `base_url` (a trailing `/` aside), or one of the window's presets —
`DEEPGRAM_API_KEY`, `ELEVEN_LABS_API_KEY`, `GROQ_API_KEY`, `OPENAI_API_KEY`,
`OPENROUTER_API_KEY` — sent to that preset's own URL. The presets are `PRESETS`
in `src/session.rs`, and the window gets them from the `config` event (§10.1),
so the window and this rule read one table. Any other request
is refused before a key is read, so whatever can write to the socket, an agent
included, cannot send a secret to a server of its choosing. A draft model with a
custom URL and a key, or an omapass key, lists once it is saved.

---

## 10. The socket

The daemon listens on `$XDG_RUNTIME_DIR/eco.sock`, on Windows on the named pipe
`\\.\pipe\eco-<user>` (§15). Every client receives all events as JSON lines and
may send commands, one per line. The first event on every connection is
`daemon` with `pid`, `exe` (the path of the executable it runs), `version`, and
`overlay` fields. A
daemon error carries a `code`, which the overlay translates as `error.<code>`.

On Windows a client is admitted first. The pipe keeps Windows' default access
list, which lets any local account open it, and setting another takes a call
tokio offers only as `unsafe`, which the crate forbids. So the first line a
client sends is `auth <token>`, the token of this run that the daemon wrote to
`%LOCALAPPDATA%\eco\eco.token` (§8), a file only its user can read. Until that
line arrives the daemon sends nothing and takes no command; a first line that
is anything else, longer than it, or late by five seconds (`ADMIT_WITHIN`)
closes the connection without a byte sent. The comparison takes the same time
whatever bytes differ. The CLI and `eco start`, `stop`, `restart` and `status`
read the file and send the line (`connect` in
`src/adapters/control_socket.rs`); while the file is not there, as in the
moment the daemon starts or stops, they treat the daemon as not running. On
Linux there is no such line: the socket is `0600`, so only its user can open
it. `admitted` and `introduce` in `src/adapters/control_socket.rs` are the one
place that decides this per platform. A window gets the token from the daemon
that starts it, in `ECO_TOKEN`, and its `LineSocket` sends the line first on
every connection, a reconnection too (§3).

Two limits keep one client from growing the daemon's memory. A command line
longer than 1 MiB (`MAX_LINE` in `src/adapters/control_socket.rs`; a
`config.set` with a full config is a few kilobytes) closes that connection
without running it. Each client has a queue of at most 4096 lines
(`MAX_QUEUED`) still to write. While that queue is full, the client skips
`signal` events, the live input meter whose next reading replaces the last;
any other event it cannot take drops it and closes its connection, while the
daemon and the other clients go on.

A client learns that the daemon has read its lines by sending `sync` after
them. The connection reads lines in order and answers `{"type":"synced"}` to
that client alone, with no command run; a queue that cannot take it closes the
connection (`SYNC` in `src/adapters/control_socket.rs`). `eco window` uses it
(§11): a CLI that exits right after writing could otherwise leave before the
daemon reads its line, which then may never run. It is one mechanism on both
platforms, for a named pipe cannot close its writing half alone.

### 10.1 Events

A new client first receives `snapshot`; a window asks for the timeline of the
session it shows:

```json
{"type":"snapshot","session":{"id":"…","title":"Entrevista","kind":"meeting","source":"live","language":"pt","state":"recording","started_at":1791083833.3,"active_s":312.4,"running_since":1791084400.0},"kinds":["meeting","conversation","other","idea"],"kind_models":{"meeting":{"transcription":"deepgram","chat":"gemini","translation":"gemini","actions":["ask","probe"]},"idea":{"transcription":"whisper-lan","chat":"qwen-lan","translation":"gemini","actions":["ask","probe"]}},"default_models":{"transcription":"deepgram","chat":"gemini","translation":"gemini","actions":["ask","probe"]},"hooks":["probe"],"participants":[{"name":"Eu","user":true},{"name":"Eles","user":false}],"inputs":[{"id":"@default-input","label":"Default microphone","participant":"Eu","color":"#ffb000"}]}
{"type":"signal","input":"@default-input","level":0.42,"speech":true}
{"type":"session","session":{…} or null,"live":[{…} for each live session],"transcribers":[{"model":"deepgram","language":"pt","sessions":2}]}
{"type":"session_opened","id":"…","window":2}
{"type":"window_call","window":2,"call":"import","path":"/home/me/retro.mp4"}
{"type":"window_focus","pid":10872}
{"type":"window_share","pids":[10872,11240],"hidden":true}
{"type":"synced"}
{"type":"session_timeline","session":"…","timeline":[{"type":"transcript",…},{"type":"note",…},{"type":"suggestion",…}]}
{"type":"transcript","session":"…","who":"Eles","name":"Ana","text":"...","at":1791083840.1,"latency_ms":990}
{"type":"transcript_partial","session":"…","who":"Eles","name":"Ana","text":"so the next…"}
{"type":"transcription","who":"Eles","state":"down","code":"stt.down","detail":"stalled: nothing heard for 15s"}
{"type":"transcription","who":"Eu","state":"down","code":"capture.down","detail":"Default microphone: not connected"}
{"type":"transcription","who":"Eles","state":"back","down_s":318.4,"dropped_s":18.2}
{"type":"suggestion_start","id":"54401f25","session":"…","action":"chat","model":"google/gemini-3.5-flash-lite","prompt":"Ele citou Kafka?","at":1791083860.0}
{"type":"suggestion_thinking","id":"54401f25","text":"..."}
{"type":"suggestion_draft","id":"54401f25","text":"..."}
{"type":"suggestion_delta","id":"54401f25","text":"..."}
{"type":"suggestion_end","id":"54401f25","ttft_ms":350,"total_ms":3400,"prompt_tokens":1919,"cached_tokens":1791,"cost_usd":null}
{"type":"suggestion_removed","id":"2f45de79"}
{"type":"session_cost","session":"…","cost":{"llm_usd":0.0031,"llm_unknown":false,"transcription_usd":0.0924,"transcription_unknown":false,"total_usd":0.0955,"transcribed_s":{"deepgram":720.0},"parts":[{"for":"answer","usd":0.0031,"unknown":false},{"for":"transcription","usd":0.0924,"unknown":false}],"items":[{"for":"transcription","at":1791084120.2,"model":"deepgram","usd":0.0924,"unknown":null,"estimate":false,"seconds":720.0,"prompt":null,"count":null},{"for":"answer","at":1791083863.4,"model":"google/gemini-3.5-flash-lite","usd":0.0031,"unknown":null,"estimate":false,"seconds":null,"prompt":"Ele citou Kafka?","count":null}]}}
{"type":"hook_started","session":"…","id":"54401f25"}
{"type":"hook_sent","session":"…","id":"54401f25"}
{"type":"session_translation","session":"…","translating":"en"}
{"type":"translated","session":"…","at":1791083840.1,"language":"en","text":"…","ms":640}
{"type":"note","id":"9c1d2e3f","session":"…","text":"Migrei um app WPF para MVVM.","at":1791083870.4}
{"type":"note_removed","id":"9c1d2e3f"}
{"type":"sessions","sessions":[{"id":"…","title":"…","kind":"idea","source":"live","language":"pt","state":"paused","started_at":…,"duration_s":312,"speech":40,"suggestions":3,"speakers":["Eu","Ana"],"cost":{…as in session_cost, without parts and items…}}]}
{"type":"sessions_found","query":"prazo","ids":["…"]}
{"type":"session_detail","session":{…summary…},"timeline":[…transcript, note and suggestion events…],"speakers":[{"label":"Speaker 1","name":"Ana","person":"…","voice":true,"suggestions":[],"guess":null},{"label":"Speaker 2","name":"Speaker 2","person":null,"voice":true,"suggestions":[{"person":"…","name":"Bruno","score":0.82}],"guess":{"person":"…","name":"Bruno","score":0.82}}]}
{"type":"session_deleted","id":"…"}
{"type":"transcript_reassigned","session":"…","who":"Speaker 1","at":1791083841.2,"label":"Speaker 1#abc12345","name":"Ana"}
{"type":"person_assigned_all","session":"…","speakers":2}
{"type":"people","people":[{"id":"…","name":"Ana","voices":3,"sessions":["…"]}]}
{"type":"session_renamed","id":"…","title":"…","kind":"idea"}
{"type":"speaker_renamed","session":"…","label":"Eles","name":"Ana"}
{"type":"transcript_removed","session":"…","who":"Eu","at":1791083841.2}
{"type":"diarized","session":"…","who":["Speaker 1","Speaker 2",…],"names":["Ana","Speaker 2",…]}
{"type":"import_date","path":"/home/me/retro.mp4","at":1790759700.0}
{"type":"import_started","session":{…},"total_s":1834.2}
{"type":"import_progress","id":"…","done_s":612.5,"total_s":1834.2}
{"type":"import_done","id":"…","complete":true}
{"type":"config","config":{...},"devices":[{"id":"@default-input","label":"...","kind":"input"}],"omapass":{"installed":false,"page":"https://plugins.omarchy.org/..."},"credential_manager":false,"presets":{"transcription":[{"name":"DEEPGRAM","values":{"base_url":"wss://api.deepgram.com/v1/listen","model":"nova-3","api_key_env":"DEEPGRAM_API_KEY"}},…],"chat":[{"name":"OPENROUTER","values":{"base_url":"https://openrouter.ai/api/v1","model":"…","api_key_env":"OPENROUTER_API_KEY","extra":{}}},…]},"echo_cancel":null}
{"type":"config_saved"}
{"type":"config_pending","id":"1","hooks":[{"action":"minutes","command":"~/bin/crm-push"}],"files":["~/notes/cv.md"],"models":[{"name":"fast","base_url":"http://192.0.2.10:8000/v1","api_key_env":null,"api_key_omapass":"openrouter","api_key_credential":null}]}
{"type":"models","target":"llm","models":["..."],"error":"..."}
{"type":"keys","env":["ECO_LAN_KEY","OPENROUTER_API_KEY"],"credentials":null,"credential_error":""}
{"type":"update","current":"0.4.0","latest":"0.5.0","newer":true,"notes":"https://github.com/This-Is-NPC/eco/releases/tag/v0.5.0","checked_at":1791083833,"check":true,"error":""}
{"type":"update_progress","stage":"installing","version":"0.5.0","detail":""}
{"type":"error","code":"session.none","params":{},"message":"start a session first"}
```

`update` is what the daemon knows of releases (§13): the version it runs
(`current`), the latest release's version (`latest`, `null` before any check
answered), whether it is `newer`, the release page (`notes`, `""` without
one), when it last asked (`checked_at`, Unix seconds, or `null`), whether
`[update] check` is on, and the last check's failure (`error`, `""` when it
answered). It is sent whenever one of these changes and to every client after
`snapshot`. `update_progress` follows one install: `stage` is `downloading`,
`verifying`, `installing`, `waiting` (installed, the restart waits for the
last live session to end), `restarting`, `done` or `failed`, with `detail`
saying why it failed.

`window_focus` and `window_share` are window control on Windows (§15), sent
only there: the window whose process is `pid` comes to the front with the
keyboard, and the windows of the processes in `pids` leave screen capture
while `hidden`, or come back to it. A client that connects is greeted, after
the rest, with the last `window_focus` and, while windows are hidden, the
last `window_share`, so a window that connects just after it opens hears both.

`transcription` is a streaming transcriber's connection, for the source `who`
(a participant), whichever sessions listen to it: `down` once per outage, with
the `code` a window translates as `error.<code>` and the provider's `detail`;
`back` when a connection takes the audio again, with the seconds it was down
(`down_s`) and the seconds of audio dropped meanwhile (`dropped_s`, 0 unless
the outage outlasted the five minutes held, §5). It is told as it happens; a
client that connects while a source is down is greeted, after `snapshot`, with
that source's `down` event, until it is back or no transcriber reads it any
more. An import streamed to its model is a source too, named by its
participant, kept until it is back or the import stops.

### 10.2 Commands

`action <name>`, `ask <question>`,
`config` (current config and devices, and in `echo_cancel` the error code that says why echo cancellation cannot run on this platform, `echo_cancel.unavailable` on Windows, or `null`; General's ECHO CANCELLATION row is disabled with that code's text as its tip), `config.set <json>` (the config whole;
a window adds `"token"`; one from another client that adds a hook, a context
file or a model, or moves a model's key, is held, §9; while one is held, any
other client's answers `config.busy`), `config.approve <token> <id>` and
`config.reject <token> <id>` (the held change `id`; another id answers
`config.stale`; once nothing is held, `config_pending` with a null `id` and
empty lists; a client that connects while a change is held is greeted with its
`config_pending`),
`session.language <json>`, `models <json>`
(`{"target","base_url","api_key_env","api_key_omapass","api_key_credential"}`; the key is read only
with no key source, a saved model's own source at that model's `base_url`, or a
preset's variable at the preset's URL, as in §9.1; anything else answers `models`
with `"code":"models.key_refused"` and `params` `{"base_url"}`), `omapass` (its
passwords: `{"type":"omapass","installed","accounts":[{"account","folder"}],"error"?}`;
not installed is `"installed":false`, not an error), `keys` (the names a
model's key can come from, answered in the background:
`{"type":"keys","env":["ECO_DEEPGRAM","OPENROUTER_API_KEY"],"credentials":["deepgram"],"credential_error":""}`;
`env` is sorted and holds the `ECO_<NAME>` variables but eco's own and the
presets' variables that are set, §9.1; `credentials` is the Credential
Manager's `eco:<name>` names, sorted, `null` on Linux, and `[]` with
`credential_error` saying why when the store could not be read; no value is
ever kept or sent),
`session.start <json>` (`{"title","kind","language","tags"}`; the kind defaults
to the first, the tags are added as by `session.tag`;
it records beside the live ones and is shown; with `"window"`, `session_opened`
tells that window to show it), `session.timeline <id>` (`session_timeline` of a
live session),
`window.show <json>` (`{"window","session"}`: the session a window shows now, or
`""`), `window.closed <json>` (`{"window"}`: the window is closing and its
process exiting; it is no longer open, so `overlay.open` opens a new one, and a
process still running 2 s later is ended), `window.active <json>`
(`{"window","active"}`: the window has the system's focus, or lost it; each
window sends it on every change, and the daemon notifies, §12.2, only while no
window it knows is active; a closed window counts as not active), `window.call <json>` (`{"call","path"}`, what a shortcut asks of a
window: `focus` only gives it the keyboard, `config` opens or closes the
settings, `new_session` the start dialog, `sessions` the sessions list when no
session is on screen, `import` the import dialog with `path`, the only call
that takes one; the newest window open gets `window_call` with its number,
`focus` excepted, and the keyboard; with none open the daemon opens one, as
`overlay.open` does, which makes the call once the daemon has greeted it
(`ECO_CALL`); any other call is `command.unknown`),
`session.pause [<id>]`, `session.resume [<id>]`, `session.end [<id>]` (the live
session named, or the one shown; ending it shows the one shown before),
`session.toggle` (the one shown),
`sessions` (list), `sessions.search <text>`, `session.show <id>` (read back),
`session.delete <id>`, `session.reopen <id>` (resume
an interrupted or ended one beside the live ones, or show a live one),
`session.rename <json>` (`{"id","title","kind"}`; a configured kind or the one
it has),
`session.speaker <json>` (`{"id","label","name"}`; an empty name restores the
label),
`session.speakers <id>` (`session_speakers` for that session),
`session.cost <id>` (`session_cost` for that session, with its parts and
charges), `session.translation <json>`, `session.context <json>`,
`entry.translate <json>`, `hook.send <json>`,
`tags` (`{"type":"tags","tags":[{"tag","sessions"}]}`, sorted by name),
`session.tag <json>` and `session.untag <json>` (`{"id","tag"}`: a `tags` record
and `session_tags` with `{"session","tags"}`, then `tags`), `tag.rename <json>`
(`{"from","to"}`:
`tag_renamed` with `{"from","to","sessions"}`, the ids changed, then `tags` and
`sessions`) and `tag.delete <json>` (`{"tag"}`: `tag_deleted` with
`{"tag","sessions"}`, then `tags` and `sessions`), `session.line.edit <json>`
(`{"id","who","at","text"}`: an `edited` record and `transcript_edited`; an
empty text removes the line), `session.line.remove <json>`
(`{"id","who","at"}`: removes a line of speech, an
`unheard` record and `transcript_removed`; `line.not_found` when it is gone),
`people`, `person.assign <json>` (`{"session","label","person","name"}`: a known
person's id, or `""` and a name — the person called that, or a new one),
`person.assign_line <json>` (`{"id","who","at","person","name"}`),
`person.assign_all <json>` (`{"session","person","name"}`),
`person.unassign <json>` (`{"session","label"}`), `person.guess.clear <json>`
(`{"session","label"}`: a `guess_dismissed` record and `session_speakers`;
`speaker.invalid` when there is no guess), `person.add <json>` (`{"name"}`: a
new person; `person.exists` with their `id` and `name` when someone has that
name in any case), `person.rename <json>` (`{"id","name"}`),
`person.attend <json>` (`{"session","person","name"}`), `person.leave <json>`
(`{"session","person"}`),
`person.merge <json>` (`{"into","from"}`), `person.forget <id>`,
`session.ask <json>` (`{"id","question"}`), `session.action <json>`
(`{"id","name"}`)
and `session.note <json>` (`{"id","text"}`)
(ask or note any session, open or stored; `ask`, `action` and `note` go to the
open one),
`session.export <id>` (the transcript as WebVTT:
`{"type":"session_export","id","format":"vtt","text"}`),
`overlay.open` (open the app: another window on a live session no window
shows, else the newest window brought forward, else a window; then
`overlay_status`, with `"raised": true` when an open window was brought
forward),
`session.import <json>` (`{"path"}`, optionally `"title"`, `"kind"`,
`"language"`, `"participant"`, `"started_at"` in seconds since the epoch),
`import.date <path>` (when the file was recorded, as an import without
`"started_at"` dates it: `import_date`, `at` null for a file that cannot be
imported), `import.cancel`, `entry.remove <json>`
(`{"session","id"}`: an answer or a note of the open session, the one read back,
or a stored one, whose log gets a `removed` record; clients get `note_removed`
for a note and `suggestion_removed` otherwise, even when it was already gone)
`update.check` (asks GitHub now, whatever `[update] check` says, and answers
`update`), `update.install <json>` (`{"version"}`, optional, the latest when
absent: installs that release through the installer port, the code `eco
update` runs, and reports each step as `update_progress`; on Linux `pkexec`
shows its own password prompt at `installing`), `daemon.restart <json>`
(`{"force"}`: restarts the daemon as `eco restart` does, by spawning the
installed `eco restart` detached — on Linux `systemd-run --user --collect
--quiet <eco> restart`, a transient unit stopping `eco.service` does not take
with it — and the windows reconnect; without `force`, while a session is live,
it is refused with `daemon.recording`, and the window asks before it sends
`force`; a restart that cannot be spawned is `daemon.restart_failed`
`{detail}`; a second `update.install` while one runs or waits is
`update.busy`),
`sync` (no command: the connection answers `synced` to this client alone once
it has read every line before it, §10) and `stop`.

---

## 11. The command line

Every command and flag is in [cli.md](cli.md), generated by `usage generate md`
from the usage spec the binary prints (`eco usage`, hidden from the help).
How the commands are built:

- `setup` downloads the VAD and speaker models and publishes the agent skill;
  `bench` measures model latency and diarization. Neither needs a daemon.
- `start`, `stop`, `restart` and `status` manage the daemon. The user service
  runs `eco daemon --headless`; `eco start` starts it if needed and waits for
  `overlay_status` after opening the window (`--headless` skips it). The
  daemon launches the windows, so one left running by an upgrade would
  launch a window it cannot serve: `eco start` and `eco window` first read
  its greeting, and when its `version` is not the CLI's, or it runs another
  executable than the CLI (by device and inode of `/proc/<pid>/exe` on
  Linux, by the greeting's `exe` on Windows, the check `status
  --expect-current-exe` makes; one it cannot see, from a sandbox with its
  own pids, is not taken for another), stop it, start the service again,
  and say
  `eco: restarted the daemon <old> → <new>` on stderr
  (`lifecycle::current` in `src/lifecycle.rs`). `eco status` and the other
  commands report or ask without restarting. A
  daemon that has not opened the socket within 10 seconds is reported with
  its own reason: the last line of its output that starts with `eco: ` or
  `Error: ` (what it prints as it exits on an error, with the `Caused by:`
  lines after it), read from the journal of `eco.service` since the start
  (`journalctl --user _SYSTEMD_USER_UNIT=eco.service`) or from
  `daemon.log` on Windows. No other line is shown: the output may hold
  transcripts.
- The session commands talk to the running daemon over its socket (start one
  with `eco start --headless` when the overlay is not wanted) and print one
  JSON object, as `okt` does: `{"ok":true,"data":…}`, or
  `{"ok":false,"code":"…","message":"…"}` with a non-zero exit —
  `argument.invalid` (an argument holds a line feed or a carriage return,
  which could end the command line early; nothing is sent),
  `daemon.offline`, `daemon.access_denied`, `daemon.unavailable`,
  `session.not_found`, `session.delete_failed`, `action.unknown`, `chat.none`,
  `completion.failed`, `suggestion.removed` (replaced by a newer request),
  `translation.none`, `import.busy`,
  `import.failed`, `person.not_found`, `person.invalid`, `person.exists`,
  `people.failed`, `line.not_found`, `tag.invalid`, `tag.not_found`.
  They read
  the broadcast for their own reply, so they work beside the overlay. `export`
  prints the document itself, to pipe or redirect.
- `window` sends what a global shortcut asks — `window.call` (`focus`,
  `config`, `new` as `new_session`, `sessions`, `import [<path>]`),
  `action <name>` or `session.toggle` (§10.2) — then `sync`, and prints
  `{"ok":true,"data":null}` once `synced` comes back, so the daemon has read
  the line, without waiting for the window; a connection that closes first
  is `daemon.closed`.
  None of these needs the windows' token.
- `key list` sends `keys` then `omapass` and prints
  `{"ok":true,"data":{"env":[…],"credentials":[…]|null,"omapass":[…]|null}}`:
  the `keys` event's two lists and the omapass account names (`null` when
  omapass is not installed); a source that could not be listed adds its
  `credential_error` or `omapass_error`. Names only (§9.1). `key set` and
  `key remove` do not talk to the daemon.
- `update` installs a release (§13): `--check` only asks GitHub and reports
  `{"current","latest","newer","notes"}`; without it the latest release is
  installed when it is newer, and `--version <version>` installs that one, an
  older one too. It prints one JSON object, `{"current","version","installed",
  "restart"}`, and each step on stderr. A running daemon installs it
  (`update.install`, §10.2) and restarts on the new version, its window
  reopened, as `eco restart` does: `restart` is `"now"`, or
  `"when_session_ends"` while a session records. With no daemon, or one that
  answers `command.unknown` (a daemon from before updates), the command
  installs it itself and `restart` is `"none"`. Codes: `update.unavailable`
  (GitHub could not be asked, or has no such release), `update.failed` (the
  download, the checksum or the install failed) and `update.busy`.
- `version` prints `{"installed","daemon","latest","channel"}`: this
  executable's version, the running daemon's (`null` when none runs), the
  latest release — what the running daemon last found, `null` before its first
  check, or with no daemon asked of GitHub — and `"release"`.
- `doctor` checks the installed files, the VAD and speaker models, that the
  config loads, that each model's key source resolves (by name; no value is
  read out), that the daemon runs, and the global shortcuts (Hyprland's binds
  on Linux, the registered keys on Windows). It prints one JSON object whose
  list holds `{"check","ok","detail","fix"}` for each, and exits non-zero when
  any fails.
- `show` carries the session's speakers and storage path/byte count; `speaker`
  names one speaker, while `assign` can identify one line or every line.

Agents get no MCP server: the `eco` skill (`skills/eco/SKILL.md`, embedded in
the binary) teaches them the CLI. `eco setup --harnesses agents,claude-code`
publishes it to `~/.agents/skills/eco/` and `~/.claude/skills/eco/`; every
`eco setup` refreshes the copies eco owns (its front matter says
`owner: eco`) and never overwrites a foreign file at that path. A change to the
CLI updates the skill in the same commit.

---

## 12. The overlay

### 12.1 Visual language and the kit

Clean cyberpunk HUD — monospace text, hairlines, outlined
surfaces, small tracked uppercase labels with two-digit
indices, one accent from the active Omarchy theme
(`$XDG_STATE_HOME/omarchy/current/theme/colors.toml`, with `~/.local/state` as
the default). The overlay reloads the palette after file edits and theme
directory replacements, including light and dark mode changes. The
kit lives in `overlay/Eco/Kit/` (`Panel`, `SurfaceFrame`, `TraceButton`, `Chip`, `Tab`,
`TextBox`, `Field`, `NumberField`, `Kicker`, `Masthead`, `ToastStack`,
`Icon`, `EcgTrace`, `DataTable`, `TabRail`, `PageBar`, `CommandRow`); views
compose it and hold no logic beyond binding `Eco`, the singleton that owns the
socket. `TabRail` is an app's pages as a carousel down the left edge, the
one on screen at its head and the others after it in order, folded to their
numbers and unfolding over what is beside it, with their names, under the
pointer, with the keyboard on it or after a tap on its head; a page `marked`
shows a dot, and ↑/↓/Home/End move along the carousel;
`PageBar`, beside its head, the commands of the page on screen, and at its end
the page's readout (a count, such as `7 SESSIONS`), which gives way first when
the bar is narrow; `CommandRow`, a
bar's actions ranked, the main one solid (`TraceButton`'s `filled`) and, when
`spread`, on the left with the others on the right and the guarded ones last,
behind a hairline, each falling back to its short name when the names do not
fit, or, with `iconsWhenTight` (as `PageBar` sets it), to its icon. A
`DataTable` that `picks` picks a row on a click and opens it on a double click,
Enter or Space; the row ↑/↓, Enter and Space act on is always the picked one.
A row's tools may carry their own act, which Enter, Space and a double click
run instead of opening the row. The control lab
(`mise run preview:controls`) shows every kit component.

- **Icons:** one set (`Icon.qml`), thin line drawings on a 16-unit grid with
  one stroke weight and round ends — settings (sliders), play, pause, stop,
  list, close, chevrons, enter, warning, spark (AI) — instead of Unicode
  glyphs that each fallback font draws differently. No connection warning
  while things work; an error toast says when the daemon is unreachable
  (§12.2).
- **Selection always shows:** the chosen option of a dropdown carries a dot and
  an accent tint, distinct from hover; the current row of a list is tinted
  with its title in the accent. No side bars.
- **Languages:** the interface is translated through JSON language packs in
  `overlay/Eco/Core/i18n/` (English, Brazilian Portuguese, Japanese), found and
  reloaded on their own, chosen in Settings › General (`[ui] language`,
  `auto` follows the system). Dates follow the chosen locale; daemon errors
  carry codes the overlay translates. See [i18n.md](i18n.md).

### 12.2 The window

A QtQuick `Window` titled `eco` (720×680), opened by
`eco start` (`--headless` skips it). It is a normal Hyprland window, so the user
moves and resizes it freely; `packaging/hypr/eco.lua` floats it, pins it to
every workspace and keeps its size. Each window is its own `eco-window` process
(§3), numbered by the daemon (`ECO_WINDOW`). Opening the app opens another window
on a live session no window shows, if there is one (`ECO_SHOW`); else it brings
the newest open window forward through the same window control as `eco window
focus`, and only with none open opens one. It
reconnects to the socket on its own.

- **Without a session:** the live trace of every input in the room above the
  buttons (dimmed behind them when the window is too short), in
  the input's colour and nothing else — flat in silence, swinging with audio,
  dashed when the input sends nothing — so a glance says capture works; then
  **NEW SESSION**, the primary button, which opens the start dialog;
  **SESSIONS**, the sessions list, the one place live sessions show (its LIVE
  filter); **IMPORT**; and **PEOPLE** — their keys (`N`, `H`, `I`, `P`) listed
  only in the shortcuts dialog, as no control shows a key. The masthead says
  READY, or OFFLINE without the daemon.
- **During a session**, as a chat, with the conversation taking every pixel the
  header does not need: the session's title in the masthead (a session without
  one is named by its kind, there and everywhere else), with
  the cost button (`$`, the session's cost screen), the details button (what
  it is, people, speakers, tags — added with the existing ones offered, taken
  off by their chip —, context and translation, in a panel that opens over the
  top of the conversation, takes its clicks and scrolls past its height under
  a head that stays) and, top right, the close button, back
  to the sessions — a live one keeps recording; then the
  **session capsule**, one line — the running time (stopped while paused) and
  state (a dot with a ring that keeps leaving it while recording), the live trace
  of every input running through it, a warning only for an input that sends no
  audio while recording (its name on hover, the settings on click), and the
  pause/resume and end segments; clicking the state pauses or resumes, and end
  fills red asking once more —; under it, a line per source whose streaming
  transcription is down (amber, the provider's reason on hover) or just back
  (green, with the audio lost, until its next line or ten seconds on), the
  kit's `TranscriptionStrip`, also under the import strip while a file is
  imported; the timeline — others' speech in a
  block tinted with their colour, the user's in a faint block on the right, every
  line at full brightness (only words still being said are dim), no side rules;
  a speaker's name and time head each turn and repeat after two minutes of
  silence or anything else between their lines; answers as framed cards whose
  Markdown is drawn as it streams (marks still open are closed; images, which
  Qt would fetch, become their alt text, and any `![` left, code included,
  gets a zero-width space so no image can open, `overlay/Eco/Core/markdown.js`) that light up
  while they stream, show the question asked and can be removed; a complete
  answer can be copied (its Markdown to the clipboard, `wl-copy`); it follows the
  newest entry, but an answer streaming taller than the view keeps its top in
  view until the next entry comes, and once the user scrolls up a "↓ N new" pill
  above the composer counts the lines, answers and finished answers that
  arrived below — a click or End goes back to the end and follows again —; and
  at the bottom the composer: the skills as chips numbered for Alt+1…9, which
  run them whether the chips show or not (a window under 560 px tall hides
  them), and a line to ask anything or, with the note button or Shift+Enter, to
  keep what was typed as a note. Typing `/` lists the skills above the line,
  filtered as the word grows: ↑/↓ choose, Tab completes, Enter runs, Esc closes
  the list and leaves the keyboard in the line. `/skill` runs a skill; a lone
  `/word` that names no skill is never asked — an error under the line says so
  and the text stays to be fixed; `/` followed by more words, or a path such as
  `/etc/hosts`, is asked. The interface says skill and note throughout; the
  config, the socket and the CLI keep `action` and `note`. Each finished
  answer's latency and cache share show on hovering its card's heading, in the
  window showing its session only; errors about a session show only there too.
- **Lines:** every line shows its speaker and time; clicking a speaker's name
  names it in that session. × beside a line on hover removes it, or
  `eco line … --remove`; ✎ beside it corrects it in place (Enter saves, Esc
  cancels).
- **Conversation:** a chat — the user's lines on the right, everyone else on
  the left, the speaker's name, time, and color on every line.
- **Notices:** what the window has to say — a save, a refusal, an error of the
  daemon — is a toast in its bottom-right corner, the kit's `ToastStack`, in
  the eco window and in the config window alike, on every screen: in a
  session the stack stands above the composer, and in the config window
  inside the section's frame, so no toast covers the composer or a bar.
  Where that room is too short for every toast, the stack hides the oldest
  until they fit again, keeping the pinned one (the daemon's absence) and the
  newest. `Eco` keeps them as a list
  (id, kind, the text's parts, time): `tell` and `say` add one, `dismiss(id)`
  takes one out. At most three show, the newest at the bottom; a fourth takes
  out the oldest. Each has its kind's icon (info, success, error), its text,
  wrapped, and a × button, which Tab reaches. Info and success close on their
  own after about six seconds, a count that stops while the pointer is over
  the toast or it has the keyboard; an error stays until its × or Esc closes
  it. While the daemon is unreachable an error toast says so, with no
  countdown, and it goes when the window connects again. A toast moves
  only as it comes and goes.
- **System notifications:** with `[ui] notifications` on (Settings › General,
  off by default) the daemon, through the `Notifier` port (§3.1, §15), sends a
  notification of the system when no eco window is active — each window tells
  it with `window.active` (§10.2), active meaning it has the keyboard and is in
  front of the system (`Host.foreground`, §15) — for these events only: a source's
  transcription down, and back (§5); a newer release found, once per version
  (§13); an answer finished, named by its skill or its question, never its
  text; and an error the windows show as an error. Nothing else, and never a
  secret or transcript text.

### 12.3 Screens and dialogs

- **Start dialog:** asks for an optional title, the kind, the session's
  language (from `stt.languages`) and optional tags (offering the existing
  ones as they are typed), and lists who will be heard. The title's placeholder
  is the default title (§7). The dialog starts from the kind and language the
  window's last session used (kept while the window lives, not across
  restarts), offers the context slots with those of the chosen kind already on,
  says which models a kind runs on once it is picked (`kind_models` in the
  snapshot: `{kind: {"transcription", "chat", "translation", "actions"}}`, each
  model by name or `""` for none, and the actions a model answers there;
  `default_models` the same for a kind no longer configured), scrolls in a short window with
  its buttons kept below, and without an input it cannot start: it says so and
  offers the Audio settings.
- **Several windows:** opening the app again opens another window on a live
  session no window shows, and otherwise brings the open one forward. The
  session view shows no other session: leaving
  it (it keeps recording) goes to SESSIONS as it was searched and filtered,
  whose LIVE filter lists the live ones, any of which shows in that window when
  opened.
- **End:** asks for confirmation, then closes the session the window shows.
- **SESSIONS:** a `PageBar` — back, NEW SESSION solid, IMPORT and PEOPLE, and
  the count of sessions listed as its readout — over a table of stored
  sessions — the live
  ones first under LIVE, with a dot before their state, then the others newest
  first under their day (TODAY, YESTERDAY, or the date): title, kind, tags, when
  (`Today 14:30`, `08 Oct 14:30`), duration, people, and state; no cost —,
  filtered to the live ones (LIVE, with TRANSCRIBING and each transcriber
  running — its model, language and `×n` when it feeds several sessions — so a
  second paid model never runs unseen), by kind when there is more than one, by
  tag, or by person — the person's chip (`PERSON: Sadao ×`), LIVE, the kinds
  and the tags share one line of chips that scrolls sideways. Until the first
  list arrives the table says it loads, a search not yet answered shows a
  loading trace in its field, and an empty table says why (no live session,
  nothing matching the search or the filters) with CLEAR FILTERS. A list that
  changes decodes only its new rows. A tag's chip renames or deletes it in
  every session (right click, the menu key, F2 or Delete; deleting asks first,
  saying how many sessions lose it). A field over the table searches it (§7.1;
  `/` goes to the field, and `↑` on the first row back to it). The table stacks
  those fields in narrow windows, a line with nothing in it left out. Selecting
  a row puts the session on screen: the same view as a live one — the
  conversation, the details panel and the composer — with, in place of the
  capsule, a line saying when it was, how long and how much was said, and the
  way to resume it, which turns that line into the capsule. The row's delete
  control (a trash can, as the details' DELETE) asks for confirmation; a live
  row offers no delete. An ended session is reopened from its ⋯ menu.
  Interrupted sessions are untinted: only the live sessions (the LIVE count) and
  an import under way are toned.
- **Keys:** `n` new session, `h` SESSIONS and `p` people on the start screen,
  `n` new session and `↑`/`↓`/`Enter` in the list, `Esc` back, `e` edit, `r` resume; on the People
  screen `F2` renames and `m` merges the selected person. Back returns to the
  screen the user came from: SESSIONS opened from People goes back to People,
  and People opened from SESSIONS back to SESSIONS.
- **Details:** the details panel opens with what the session is, in two lines
  at most — its state, start ("Today 14:30", as the sessions list), running
  duration, and how many lines, answers and notes it holds — beside the edit
  (title and kind) button and the close (an up chevron, as Esc), the title
  itself being in the masthead; that head stays while the rest scrolls under
  it, a hairline under it while it does; a stored session adds
  its absolute storage path, with COPY PATH beside it, and copies its
  transcript. An interrupted or ended session can be deleted after a
  title-specific confirmation; a live session's delete is off. Its CONTEXT line's
  chips turn slots on and off (§6), and its TRANSLATION line picks the session's
  translation language (§6).
- **Voice guesses:** a guess shows on the speaker's lines as `ANA?` and is
  counted on one line of the session (or, on a short window, on its details
  button), whose REVIEW opens the details at their SPEAKERS list.
- **Who is who** has one place in the overlay: the SPEAKERS list of the
  session's details, each speaker label with the person it is, eco's guess
  (CONFIRM or CLEAR), or the name it goes by, CLEAR undoing a person and CHANGE
  asking who they are. Clicking a speaker in the timeline asks the same:
  the people their voice suggests (with their score, when the session kept it),
  then everyone known, or a name. Opened on a line, the assignment can instead
  take THIS LINE alone. The dialog says how many lines will change, as a warning
  when more than one will; assigning every line of a session at once is left to
  the CLI. Its color picker changes the known person's color across sessions, or
  saves a local speaker color in the session log. The overlay never shows a
  person id: it names them as known, else by the session speaker linked to them,
  else as an unknown person the session's people dialog removes. Renaming a
  person to a name someone else has warns and offers to merge the two.
- **People dialog:** shows those FROM SPEAKERS apart, off with the reason on
  hover, and under ADD SOMEONE the ones added (a click takes them back) and
  everyone else known; each chip wears the person's colour, two with one name
  tell apart by their session count, and the dialog's top edge stays put as it
  grows. NOT THEM undoes it.
- **People screen** (PEOPLE on the start screen or in SESSIONS; `p` on the
  start screen), under a `PageBar` with back, ADD solid and the count of people
  as its readout (while a merge waits, only the readout), lists everyone with their voices and sessions, and also
  changes person colors; selecting a person opens
  their sessions. The same screen adds someone by a name no one has (ADD, also
  offered by the empty list), renames, merges or forgets them. MERGE shows a
  notice with CANCEL — the one way out besides Esc, as the way back hides —
  until the other person is picked — picking the same one says so — then asks
  which name stays and what moves, SWAP turning the direction around. FORGET
  says which voices are deleted and that the sessions keep the name. Below
  420 px a row's colour, rename, merge and forget fold into one ⋯ menu.
- **Import:** from the start screen or SESSIONS (`i`; the dialog's BROWSE opens
  a file dialog, and importing without a file says one is needed), or by
  dropping a file on the window. RECORDED shows when the file was recorded
  (`YYYY-MM-DD HH:MM`, read back as SESSIONS writes dates) and takes another;
  one it cannot read is outlined and stops the import. A strip shows the
  progress and stops it.
- **Cost:** shown only when asked, inside the session: the masthead's cost
  button (`$`) opens the session's cost screen, and the details panel's COST
  only when the masthead has no room for it (a short, narrow live window, where
  the capsule joins the masthead) (masthead `COST · <title>`,
  SESSION or Esc back to the session) — the total, the LLM part with answers,
  reviews and translations, the transcription's with its minutes, each unknown
  reason once in full and a note when a part is estimated, then the charges in
  the DataTable (what — the answer's question or action —, time, model, cost, an
  unknown one as "?" with its reason); it follows a live session while open.
  Neither the sessions list nor an answer card shows a cost.

### 12.4 Keyboard, motion and traces

- **Keyboard first, mouse too:** everything the pointer does, the keyboard
  does. Tab and Shift+Tab move through every control, drawn with a focus ring
  apart from its own state; Enter or Space presses it; a control's hint shows on
  focus as on hover. Menus and dropdowns open on the keyboard, start on the
  chosen entry, move with ↑/↓/Home/End and pick with Enter; Esc closes the
  innermost thing open — a menu, then a dialog, then the details panel, then
  the session view, live or stored, as its close button does — and focus
  returns to what opened it, even when the window was inactive as it opened. A
  menu closes when its page or view changes or the window goes inactive.
  Dialogs keep Tab inside and start on their first field, or on the way back
  when they confirm something that cannot be undone.
  Every dialog ends in one footer: what else it offers on the left, then the
  way back (Esc) and the primary action (Enter in its fields) on the right;
  none carries an index, and its top edge stays where it opened as it grows.
  Tables are one Tab stop: ↑/↓ choose, Enter opens, Delete asks to remove, and
  only the selected row's tools are Tab stops. The timeline is one Tab stop too,
  entering on the newest entry: ↑/↓/PgUp/PgDn/Home/End move, the entry shows its
  tools to Tab into, and N names a line's speaker. A view that appears puts the
  keyboard where it is used (the composer in a session), and scrolling views
  follow the focus. `?` or F1 lists every key.
- **Motion only where it means something** — a state, new content, or the
  user's own action; nothing animates on a timer for its own sake. Following
  omakiten (`OutCubic`, 90–300 ms): the recording dot sends out a ring (it is
  recording); an answer still waiting for its first word draws the same
  loading trace as a busy button — it nears the end but reaches it only when
  content comes, reasoning or reply — counts the seconds and shows the
  model's reasoning as it arrives (it is working), then lights its frame while
  the reply grows (it is still arriving) — text being read never scrambles; a
  failed answer stays, in red, saying why; speech slides in from its speaker's
  side, answers rise in, the new-content pill rises above the composer
  and a toast comes in and goes out (something new); traces move only with real audio (the inputs work);
  views slide when the user navigates (see Navigation motion) and visible
  labels decode softly; action buttons grow an accent rail on hover and
  carry an audio trace while waiting for the next state.
- **Navigation motion:** going forward, the view being left slides out to the
  left while the new one comes in from the right. Navigation buttons show a
  short audio trace before the view changes; the entering view reveals its
  title and visible content with the Soft kana decode.
- **Input traces (ECG style):** one trace per captured input, in that input's
  colour. The daemon sends each 32 ms frame's loudness (log-scaled RMS, so
  quiet speech still shows) and whether it is speech; the trace keeps about
  five seconds, swings up and down with the loudness as a smooth curve, glows
  faintly, fades out to the left and is written by a bright head on the right.
  Brighter while it hears speech; a faint dashed line when the input sends
  nothing for a second. Smoothness: samples get evenly spaced times (32 ms
  apart, resynced if they drift) instead of arrival times, the trace runs 70 ms
  behind the newest sample to ride out socket bursts, its geometry (a QtQuick
  `Shape`, Catmull-Rom smoothed) is rebuilt only when a sample arrives, and in
  between it slides once per display frame (`FrameAnimation`). A `Canvas`
  repainted per frame looked as smooth but cost ~77% of a core; this costs
  ~12%. The same trace runs in the start screen's background,
  inside the session capsule, and in the LEVEL column of each device in the
  config window's Audio section.
- **Input colours:** the user picks a colour per device, in its row under
  Settings › Audio (a palette of ten vivid
  colours, or automatic by position) so a trace says which input hears audio
  by colour alone; stored in `[colors]` (device id → `#rrggbb`). The palette is
  the theme's colours made readable, then their hues turned when the theme has
  fewer; no two are alike and none is near the accent, so none reads as a
  selection. A person without a chosen colour gets the first palette colour
  from where their id points that no other listed person wears.

### 12.5 Responsive

Both windows go down to 240×320 px, because Hyprland may tile
a window below its minimum and Qt would then draw it at the minimum, clipped.
No layout row may need more than the window offers: one wide row widens the
whole layout, so buttons wrap, text elides and lists change columns instead.
Under 560 px the overlay's capsule keeps icons only (labels move to hints), its
trace shrinks, the language button shows the code, and the composer's send
button keeps its icon. Under 560 px tall the overlay tightens its margins and
gaps, the voice guesses become a count on the details button (which opens
the details at the speakers), and, from 360 px wide, a live session's capsule
joins the masthead line — state dot with its ring, running time, the
silent-input warning, pause and end, without the trace; on a window also under
560 px wide the name `ECO ╱` drops, the title elides first and the language
picker and the cost button move into the details. The config window scrolls an item's screen when it is long, and its
tables stack each row's cells into lines under 700 px. Long
titles elide and the section name in the masthead keeps its room; toasts
narrow with the window and their text wraps. Nested
layouts are pinned to their natural height so the timeline gets the space.

### 12.6 The config window

A QtQuick `Window` ("eco · settings", fixed and untranslated, as `packaging/hypr/eco.lua` matches it), opened
from the overlay or with `window.call {"call":"config"}` (§10.2).
It is created the first time it opens and then only shown and hidden: Qt writes
its pipeline cache to disk when a window is destroyed, and the interface would
wait for that, often for seconds. Each opening asks the daemon for the config.
It asks for 1040×760, or its screen less a 96 px margin when that is smaller;
the rule in `packaging/hypr/eco.lua` gives it no size, so it keeps that one, and
centres it on the screen (without `center`, Hyprland would centre it on the
overlay, past the screen's edge).

**Sections.** Eight, down the left in the kit's `TabRail`, each named for
the one question it answers: 01 Audio, 02 Models, 03 Languages, 04 Session
kinds, 05 Prompts, 06 Context, 07 Skills, 08 General (`settings.at`: `audio`,
`models`, `languages`, `kinds`, `prompts`, `context`, `skills`, `general`) —
Audio first and Models second, since the sections after it pick from it, and
Models sets the defaults: the chat, translation, review and transcription
models used where nothing more specific picks one. The rail is a carousel in that order: the section on screen at
its head beside the bar, the ones after it below, the ones before it coming
round after the last. Folded, each tab shows only its number; under the
pointer, with the keyboard on one of its tabs, or after a tap on its head, it
unfolds over the bar and the section with the names. Each tab is a stop of the
Tab chain (rail, bar, section, then the close button): ↑/↓/Home/End move along
the carousel, Enter or Space chooses, and the chosen section takes the
keyboard, so the rail folds. A section's number chooses it unless a field
being written holds the keyboard and takes the digit; Ctrl with the number
chooses it anywhere in the window, and `Ctrl+Tab` and `Ctrl+Shift+Tab` turn
to the next or the one before. Each section
owns its own fields: no section edits another's, and what another decides is
only read there, as a link to it. A section whose open item has changes not
saved shows a dot on its tab.

**The bar.** The kit's `PageBar`, beside the rail, holds the commands of what
is on screen, ranked: BACK first and apart; the main action solid, on the
left; across a gap, on the right, the others outlined, the guarded one
(DELETE or DISCARD, at most one a bar) last, behind a hairline. Commands that
act on a picked row are there whether a row is picked or not, dimmed until one
is with the reason in their tip, so the bar does not move as rows are picked.
Icons say what a command does: a trash can destroys, × takes out of a list, a
right chevron opens. When their names do not fit, BACK shows only its
chevron and each action its short name where it has one (DEFAULT FOR CHAT
becomes FOR CHAT); when even those do not fit, each action but the main one
shows only its icon, on a bar narrower still the main action too, each name in
its tip, and narrower again one menu holds them all. The window's close is a corner button on the masthead line;
its messages are toasts in the bottom-right corner (§12.2).

**Tables.** Each section is a `DataTable` under its name and what it is for.
A simple value — ON/OFF, a choice, a name, a number — is changed in its row
and saved at once (below, **Values in the row**); only content — models,
session kinds, prompts, context, skills — has a screen of its own. A click or
↑/↓ picks a row; on a table of items a double click, Enter or Space opens it,
when it has a screen: a spoken language has none, so Languages has no VIEW or
EDIT. The bar offers ADD (ADD MODEL, ADD LANGUAGE, ADD
CONTEXT SLOT, ADD SKILL, ADD KIND), then, for the row picked, VIEW, EDIT, the
section's own and DELETE. EDIT (`Ctrl+E` on the table) opens the row in its
form, and SAVE, CANCEL or Esc returns to the table with the row still picked
(under its new id, when the save renamed it); it is dimmed, with why, on a row
whose screen has no form, as Global context's. The section's own: REFRESH
DEVICES on Audio, which sends `devices` and changes nothing else; DEFAULT FOR
CHAT, DEFAULT FOR TRANSLATION and DEFAULT FOR REVIEW on a chat model and
DEFAULT FOR TRANSCRIPTION on a transcription one, which set `[llm] model`,
`[translation] model`, `[reviewer] model` and `[stt] model`, each dimmed on the
model already that default; MAKE DEFAULT on Languages, which sets `[stt]
language`, and on Session kinds, which moves the kind first. Each of these saves at once. ADD LANGUAGE opens a screen in the
section, the languages not offered yet as a table, where choosing one saves it
and returns to the table. Lists keep the order their entries were added in:
nothing moves an entry. Models' DEFAULT column says which defaults a model is;
a translation default that is not set follows the chat one and reads
inherited there. DELETE, or Delete on a
row, asks first in a dialog that says what follows, then saves at once.
A part of the config that is not a list item — Context's Global row — has
DELETE dimmed, `Always kept`; Prompts, whose two rows are both such parts, has
no ADD or DELETE at all; an item that may not go yet, as the chat default, has
it dimmed, with why in its tip. Until a chat model is registered, Prompts,
Context and Skills hold only NO CHAT
MODEL, what it means, and REGISTER A CHAT MODEL, which opens a new model in
Models; a kind offers no assistant or translation model then.

**An item's screen.** An item opens read (`ItemPage`): its kind, its name, ABOUT — its facts — and RELATED, what it uses and what uses it, side
by side from 640 px wide; with nothing related, RELATED is left out and ABOUT
takes the width. Long texts, such as a skill's prompt, follow across the
screen as they were written, their own lines kept. Each related
entry is a link that opens that item in its own section; BACK returns to where
it was followed from, and choosing a section on the rail starts the way back
over. A link, or REGISTER A CHAT MODEL, into a section whose open item has
changes not saved shows that item instead, and a toast says
`Save or cancel <name> first`. EDIT (`Ctrl+E`) turns the screen into its form;
opened by the table's EDIT, the item goes back to the table once saved or
cancelled. Every field is named by a
caption right above its control, pickers (models, languages) as much as text
inputs: no label sits in a column of its own. Prompt areas (rules, a skill's
prompt and format, the reviewer's prompt) grow to ten lines, then scroll
inside. Number fields take digits only and keep the last valid number. A
form holds only single values — text, pickers, and chips that choose among
fixed options, as a slot's kinds — never a collection.

**Child tables.** A collection that belongs to an item — the files of the
global context and of each context slot — is a titled `DataTable` on the
item's read screen, under ABOUT and RELATED (`tables` in `ItemPage`), its rows
in the order they were added. Its rows are changed from the bar: the table's
ADD (ADD FILE) — the main action of an item with no form, as the global
context, otherwise beside the others —, then REMOVE named for what it takes
out (REMOVE FILE), with × and not guarded, as ADD undoes it in one step; it is
there whether a row is picked or not, dimmed until one is. Each **saves at
once**, as DELETE does, on a copy of the saved config, finding what it changes
by its name, and the row stays picked. ADD opens a **child screen**
(`ChildScreen`) inside the item: the item's name small over its title, BACK
(or Esc) back to the item without saving, and a small form: a file's PATH with
BROWSE, which adds every file chosen in the file dialog at once. A new item
opens in its form, so its collections fill only after its first SAVE. A screen
that saves and closes — a child screen, a section's choosing screen
(`ChooseView`: ADD LANGUAGE), an item deleted — closes first, then saves: a
table destroyed while the save changes its rows crashed the window.

**Values in the row.** Audio and General are tables of values, each row's
control inline: a `Dropdown`, a `ColorPicker`, a lit/dim ON/OFF `Chip`, or a
field for a name (`SyncedBox`, `inline`) or a number (`NumberField`,
`inline`). They are the table's `tools` (with `toolsTitle` naming the column,
`toolsAlways` keeping them shown on every row and `toolsLive` keeping every
row's working, not only the picked row's), and Enter, Space and a double click
on the row run its `act()`: open the list, turn the switch, or start writing,
where Enter, or leaving the field, saves and Esc puts the value back; either
gives the keyboard back to the rows. Each saves at once, on a copy of the
saved config; the controls show the saved config, so a refused save leaves
them as they were, and a toast says why. Tab from the rows
reaches the control of the row picked, and goes on through the next rows'
controls, picking each row it enters. General is a section's table; Audio's
two tables are its `body`, a component the section draws in the table's place
(`AudioBody`), whose rows the bar's commands act on as on a table's.

**Saving one item.** An item is edited in an `ItemDraft`: a copy of the saved
config that its form changes only where the item is. SAVE (`Ctrl+S`) sends
the saved config plus that item's change, whole, and the daemon validates and
saves it (§9); a rename follows wherever the item is named — a model into
every use, a kind into the models and context slots — and the screen follows
the new name. Renaming as it is typed, the uses follow only a name that is
not empty and no other model's or kind's, so clearing a name or passing
through another's moves nothing. One save goes to the daemon at a time: while
one is under way, SAVE, CANCEL, DISCARD and Esc wait. DISCARD, shown once a
saved item changed, reads it again; CANCEL stops editing and drops the
changes, and a new item not saved goes back to the table; either, or Esc,
asks first (DISCARD CHANGES or KEEP EDITING) when something was changed. The
title's line marks NEW, UNSAVED and EDITING.
Each section keeps its open item and its edit while another is on screen.
When the saved config changes under a changed item (another program saved,
or another item here), its changes are made again on the new one while the
lists it lives in, and every list its changes reached (a kind's models pick
reaches the models), are as they were there; otherwise it is marked CHANGED
ELSEWHERE, SAVE says `Settings changed elsewhere. Discard your draft to reload
before saving.` and saves nothing until DISCARD — shown then for a new item
too, which it starts again — reads it again.

What keeps an item from saving shows on its own field — an outline in the
error colour and the reason under it — and the first such field takes the
keyboard: a repeated name or a space in a skill's name as it is typed, a
missing value once a save is tried. Each clears as soon as its field is fixed.
A problem outside the item's fields, and a daemon's refusal, shows under its
title until the item changes again; a refused DELETE or quick change shows as an
error toast (§12.2). The default models are not required, as the daemon does not
require them (§9): a config with no model registered saves, and a default it
names must be a registered model of its type (`overlay/Eco/Core/draft.js`,
`problems`). Naming a new model makes it the default of nothing; each default
is set on the Models table.

Closing the window with an item unsaved asks first: SAVE AND CLOSE saves
each changed item in turn, the one on screen first, and closes once none is
left; one that does not save stops it, shown, with why in the bar; it is off
while an item is CHANGED ELSEWHERE. KEEP EDITING; or DISCARD CHANGES, which
drops every one. Closing the eco window ends its process, the settings with it, without
asking: unsaved changes are dropped. Leaving is what the user asked for, and
Qt's own quit, which asks every window first, would stop at the kept settings
window, which never closes, and leave the process running unseen.

**Keys.** `Ctrl+1`…`Ctrl+8` a section, `Ctrl+Tab` / `Ctrl+Shift+Tab` the next
or previous one; on the rail ↑/↓/Home/End move and Enter or Space opens the
section. In a section's table ↑/↓ pick a row, Enter or Space opens it, Delete
deletes it (asking first); in a child table ↑/↓ pick, Delete removes the
row, and on a child screen's table Enter adds it. On a row with a value, Enter
or Space acts on its control — opens the list, turns the switch, or starts
writing a name or a number, which Enter saves and Esc puts back — and Tab
reaches the control. `Ctrl+E` edits the item picked on a table, or the item open, `Ctrl+S`
saves it. Esc goes back one level: out of a child
screen, out of editing (asking first when that drops changes), back to the
table, then closes the window. Keys show only in the shortcuts dialog
(`ShortcutsDialog`), never on a button or in a tip.

**The sections and their items.**

- **Audio** (`AudioSection`), who is heard through which device: two tables
  of values, no item screens. The first has a row per input and output of the
  computer (`Eco.devices`), and one, dim and marked unavailable, per device a
  source names that is not there: DEVICE, its label; KIND, MIC or OUT; LEVEL,
  its live trace (`EcgTrace`) in its colour while it has signal
  (`Eco.signals`); HEARD AS, a dropdown of every source, then Not heard, then
  New source…; and COLOUR, a colour picker. Choosing a source gives the device
  to it, taking it from the source that had it; Not heard takes it from its
  source; New source… asks a name and creates that source with this device.
  Each saves at once. The bar holds REFRESH DEVICES, which sends `devices` and
  changes nothing else. Under it, AUDIO SOURCES, a row per audio source: NAME,
  written in the row (Enter or leaving the field saves, Esc puts it back), and
  YOU, a chip lit on the user's source (one source at most is you; lighting
  one dims the other). DELETE deletes the source picked there, asking first,
  naming the devices no longer heard, whether nobody else is heard (it held the
  only output device) and whether nobody is the user any more. Echoes are set
  under General.
- **Models** (`ModelsSection`): the registry and the defaults — a row per
  model, chat and transcription alike: its name, type, the provider's model
  id, DEFAULT (what it is the default for: CHAT, TRANSLATION, REVIEW,
  TRANSCRIPTION, or TRANSLATION (INHERITED) on the chat default while no
  translation default is set) and what else uses it. With a row picked,
  DEFAULT FOR CHAT, DEFAULT FOR TRANSLATION and DEFAULT FOR REVIEW on a chat
  model, DEFAULT FOR TRANSCRIPTION on a transcription one, each saved at once
  and dimmed on the model that is already that default (`Draft.defaults`,
  `Draft.makeDefault`); with none picked, DEFAULT FOR CHAT, dimmed. DEFAULT
  FOR REVIEW sets `[reviewer] model` and leaves `enabled` as it is; nothing
  else in the window picks the reviewer's model. Read: its type, what it is
  the default for, provider, base URL, model id, where its key comes from, and
  for chat its REASONING and extra fields, for transcription its price per
  minute; RELATED, what else uses it, grouped by the section that sets each
  use (the skills that name it, and each kind once, under a
  group that says what the kinds in it use it for: assistant, translation or
  both, as the overlay's one resolver, `answering` in
  `overlay/Eco/Core/draft.js`, mirrors the daemon's precedence). Edited: its
  TYPE while nothing uses it (changing it keeps the name and model id; the
  provider and key change only when its base URL does not fit the new type),
  its NAME, for chat REASONING (the provider's — or IN EXTRA FIELDS when
  `extra` sets one —, off, low, medium or high: `reasoning`, sent as
  OpenRouter's `reasoning` field and winning over one in `extra`), the
  provider (a preset of its type or CUSTOM, which shows the base URL), model
  (typed, with the preset's model as an example, or picked from LIST, which
  closes the list again, as Esc does), key from an environment variable or an
  omapass password, for transcription a fallback price per minute and, for
  chat, the extra JSON sent with each request (EXTRA (JSON)). The chat and
  transcription defaults cannot be deleted; deleting another asks first, listing what
  falls back to which model (as `answering` picks it) and whether the reviewer
  turns off. A new model starts from the base URL and key source of the last
  model of its type.
- **Languages** (`LanguagesSection`): a row per offered spoken language,
  DEFAULT on the one new sessions start in; a language has no screen. ADD
  LANGUAGE opens `AddLanguageView`, the languages eco knows that are not
  offered as a table, where ADD LANGUAGE or Enter offers the one picked, saves
  and returns to the table with it picked. For the row picked, MAKE DEFAULT and
  DELETE, which asks first and, for the default, names the first language
  left, the new default; the last language stays. The default transcription
  model is set on Models.
- **Session kinds** (`KindsSection`): the kinds, the default first (suggested
  by the start and import dialogs), each row the kind, DEFAULT on the first and
  the models its sessions transcribe and, once a chat model is registered,
  answer with. Read: whether it is the default and its transcription,
  assistant and translation models, each its own or the default named with the
  model it inherits; its models and the context slots on from the start in it
  as links; MAKE DEFAULT. Edited: its name, never empty or another kind's, and
  those three models, noting that its assistant model answers questions and
  the actions that name no model. Renaming or deleting a kind follows it into
  the models and context slots; the last kind cannot be deleted, and deleting
  another asks first, saying that its sessions keep it, which kind becomes the
  default and that its own models and slots stop applying.
- **Prompts** (`PromptsSection`), what instructions the models get: two rows,
  nothing added or deleted — RULES FOR EVERY ANSWER, `rules`, and REVIEWER
  PROMPT, `[reviewer] prompt`. Read, a prompt shows its text as written
  (`ItemText`); EDIT holds only that text, in a `SettingsTextArea`. It holds NO
  CHAT MODEL until a chat model is registered.
- **Context** (`ContextSection`): the files sent with answers, each row its
  name, when it is sent and how many files: Global, sent with every answer,
  whose files, in the order they were added, are its child table FILES (ADD
  FILE, REMOVE FILE), with no form, so no EDIT; and a row per context slot
  (`On in <kinds>` or `When a session turns it on`), the only rows ADD and
  DELETE reach — a slot's files are the same child table, its form only its
  name and kinds, and deleting one says which kinds no longer start with it
  on.
- **Skills** (`SkillsSection`): the actions in the composer's order, the order
  they were added in, each row its name, its
  model — or the default, named with the model it inherits (`default ·
  <model>`) — and its hook. Read: its model, hook and whether the hook sends on
  its own, then its prompt and output format, its model a link. Edited: its
  name, prompt, output format, model and hook, sent on its own (SEND ON ITS
  OWN, once a hook is set) or from the answer. An action that names a model
  answers with it in every session kind. A name takes no spaces and no other
  skill's name; renaming one warns that global shortcuts call it by name
  (`action <name>`), and deleting one asks first, saying those shortcuts stop
  working and the skills after it move up.
- **General** (`GeneralSection`), how eco behaves: one table of values under
  titles (the `DataTable`'s `section` groups), each value beside its name as
  the control that changes it, saved at once. INTERFACE: the interface
  language, a dropdown of `auto` and the packs; HIDE FROM SCREEN SHARING (off
  by default), whose help says eco then shows black in the user's screenshots
  and recordings too (§13); SHOW ECO IN THE TRAY (off by default, §12.7);
  SYSTEM NOTIFICATIONS (`[ui] notifications`, off by default, §12.2).
  REVIEWER: REVIEW ANSWERS (`[reviewer] enabled`), the reviewer's model named
  beside its switch, on or off, as a hint; turned on with no reviewer
  model, `Draft.review` takes the chat default; and SHOW THE DRAFT
  (`verbose`), dim while review is off. AUDIO: DROP ECHOES and ECHO
  CANCELLATION (`[audio] drop_echoes`, `echo_cancel`), the latter disabled
  while off where the daemon cannot cancel echoes (its `config` reply's
  `echo_cancel` code, on Windows), with that code's text as its tip and note;
  on, it can still be turned off. ANSWERS:
  MAXIMUM CONTEXT (CHARACTERS) (`[llm] max_context_chars`) and ANSWERS AT THE SAME TIME
  (`[llm] concurrency`), numbers written in the row. The note says what the
  picked row does. No screens, so no VIEW or EDIT, and no DELETE: the bar is
  empty.

### 12.7 The tray icon

With `[ui] tray` on, the daemon runs one more `eco-window` process, on
`overlay/tray.qml`: a `Qt.labs.platform` `SystemTrayIcon` bound to the `Eco`
singleton. On Linux Qt exports it as a StatusNotifierItem over D-Bus, with its
menu as a `com.canonical.dbusmenu`, which the bar's tray shows (on Omarchy a
StatusNotifierHost is on the session bus); on Windows it is a notification
area icon, its menu a Qt Widgets menu. Its icon is `overlay/icons/tray.png`,
eco's bars in white, or `tray-recording.png`, with a red dot, while any live
session records; its tooltip is `eco · idle` or `eco · recording · <title>`.
A click sends `window.call {"call":"focus"}` and the menu's **Open** the same,
**New session** `new_session`, **Settings** `config` — each the line `eco
window` sends (§10.2) — and **Stop eco** `stop`, the line `eco stop` sends.
`tests/window.rs` runs it offscreen and checks the lines and the states.

The tray is part of the windows' lifecycle in `src/adapters/overlay.rs`, but
not a window: it has no number, is never focused nor hidden from sharing, does
not count for `overlay` in the greeting (`eco status --window-open`) and never
gets a `window.call`. The daemon starts it when it starts with the setting on
and after any line that leaves it on, ends it (`SIGTERM`, terminated on
Windows) after a line that turns it off, and ends it with the windows when it
stops. A tray process that exits, or cannot start, starts again after 1 s,
then a pause that doubles to at most 60 s; one that ran a minute starts the
count again.

---

## 13. Packaging

**The release package.** `packaging/arch/PKGBUILD` builds eco for pacman from
the committed tree (`git archive HEAD`, never uncommitted changes) and installs
`/usr/bin/eco`, the window program `/usr/lib/eco/eco-window` (in `lib/eco`,
not on `PATH`: only the daemon starts it), the overlay it runs under
`/usr/share/eco/overlay`, the Hyprland rules under
`/usr/share/eco/hypr/eco.lua`, the launcher, the icon, the licence, the user
service in `/usr/lib/systemd/user`, and the bash, zsh and fish completions in
their shells' vendor directories. `scripts/completions` writes those with
`usage generate completion` from the spec `eco usage` prints; they call
`usage complete-word` as they complete, so `usage` (in Arch's `extra`) is a
dependency to build and to run. Building also needs `cargo`, `cmake`, `ninja`
and Qt 6 (`qt6-base`, `qt6-declarative`); running needs the two Qt packages.
`mise run package`
(`scripts/package`) runs `makepkg` into `target/arch/pkg`; `install.sh`
downloads the package and the release's `SHA256SUMS` from a GitHub release,
checks one against the other and runs `pacman -U`.

**The Windows package.** On Windows, `mise run package` builds `eco-window`
first (`window:build`), then `scripts/package` builds the release binary from
the working tree and writes `dist/eco-<version>-windows-x86_64.zip`, the
version read from `Cargo.toml`. The zip holds one folder, `eco`, laid out as
the prefix of the Arch package, so the daemon finds what it ships the same way
on both (`paths::shipped`, `paths::shipped_program`: `share/eco` and `lib/eco`
under the folder above the binary's):

```
eco/bin/eco.exe              the only program on PATH
eco/lib/eco/eco-window.exe   the window, with its Qt runtime beside it
eco/share/eco/overlay/       the QML it runs
eco/share/eco/eco.ico        the icon of eco's notifications
eco/LICENSE  eco/README.txt
```

`eco-window.exe` carries eco's icon, `packaging/windows/eco.ico`, as the
resource `IDI_ICON1` (`window/eco-window.rc`, built only on Windows), which Qt
also gives the windows it opens (read in Qt's source, not seen); the Start-menu
shortcut takes its icon from there, so `eco.exe` carries none. `mise run icon`
(`scripts/icon`) renders the `.ico` from `packaging/eco.svg`, and the tray's
icons into `overlay/icons` (§12.7), shipped with the overlay, on Python's
standard library: the SVG is rounded rectangles, which the script rasterises
itself and refuses any other element, so no SVG renderer is pinned for it.

The Qt runtime is what `windeployqt` copied into `target/window` as
`scripts/window-build` built it there (the build itself goes to
`target/window-msvc`), taken as it is. It leaves out the compiler runtime, so
the machine needs the Microsoft Visual C++ Redistributable (x64), as `eco.exe`
does. The script is Python, as `scripts/window-build` is, because it runs on
both systems under mise and its `zipfile` needs nothing more; the `bash` on a
Windows `PATH` may be WSL's. The release attaches the zip (below). The
programs are not code-signed, so SmartScreen may warn the first time one runs.

**The Windows installer.** `install.ps1`, at the root of the repository, is
to Windows what `install.sh` is to Arch, run as `irm <raw URL> | iex`, on
Windows PowerShell 5.1 and no module. It reads the latest tag from GitHub's
API (or `VERSION`), downloads the release's `SHA256SUMS` and the zip it
names, and refuses the zip when `Get-FileHash` disagrees. Then it stops the
installed daemon (`eco stop` through the installed `eco.exe`) and the
`eco-window` processes run from the install, replaces
`%LOCALAPPDATA%\Programs\eco` with the zip's `eco` folder, adds its `bin`
to the user's `PATH`, runs `eco setup`, and starts again a daemon that was
running, with its window if it was open. `ECO_UNINSTALL=keep` or
`everything` takes it off instead; `REPO`, `BASE_URL` and `DRY_RUN` are
those of `install.sh`. It writes the `PATH` in `HKCU\Environment` through
`Microsoft.Win32.Registry`, reading it unexpanded and writing it back with
its own type (`REG_EXPAND_SZ`), then broadcasts `WM_SETTINGCHANGE` through
`SendMessageTimeout`, so Explorer gives it to the programs it starts:
`[Environment]::SetEnvironmentVariable` would write it back expanded, as
`REG_SZ`. Taking eco off also deletes the app id key `eco setup` wrote
(`HKCU\Software\Classes\AppUserModelId\This-Is-NPC.eco`, §15). Its errors end the script with status 1 when it runs as a file,
and leave the session open under `iex`. `mise run install:windows` and
`mise run uninstall:windows` are the same script: `mise run package` also
writes `dist/SHA256SUMS`, and `scripts/install-windows` runs `install.ps1`
with `BASE_URL` at `dist`'s `file:` URL; `scripts/uninstall-windows` asks,
then runs it with `ECO_UNINSTALL`.

**Updates.** eco installs its own releases with the steps of `install.sh` and
`install.ps1`, written in Rust behind two ports: the release source, GitHub,
and the installer, one adapter per platform.

- *Knowing.* The daemon asks
  `https://api.github.com/repos/This-Is-NPC/eco/releases/latest` for the tag
  and the release page only: one GET with `User-Agent: eco/<version>`, nothing
  of the user in it, and no telemetry. It asks when it starts if the last
  check is older than 24 hours, then every 24 hours while it runs, and on
  `update.check` (§10.2) or `eco update --check`. `[update] check = false`
  (§9) stops the daily check. The answer is kept in `update.json` in the state
  directory (§8), `checked_at` and `latest`, so a restart does not ask again,
  and told to the windows as `update` (§10.1), and, with `[ui] notifications`
  on and no window active, as one system notification per version (§12.2). A network that is down or an
  answer that does not parse leaves the last state and sets `error`.
- *Fetching.* `eco update`, or `update.install` from the window, downloads
  the release's `SHA256SUMS` from
  `https://github.com/This-Is-NPC/eco/releases/download/<tag>/`, takes from it
  the line of this system's asset — the `eco-<version>-…pkg.tar.zst` Arch
  package on Linux, `eco-<version>-windows-x86_64.zip` on Windows, the names
  `install.sh` and `install.ps1` match — downloads the asset and checks its
  sha256. On a mismatch nothing is installed and the update fails.
- *Installing.* On Linux, `pkexec pacman -U --noconfirm <file>`, which shows
  the desktop's password prompt; without `pkexec` and with a terminal
  attached, `sudo pacman -U`. On Windows, `%LOCALAPPDATA%\Programs\eco` is
  replaced with the zip's `eco` folder, as `install.ps1` does; the running
  `eco.exe` cannot be overwritten, so every file of the folder is renamed
  aside first, `<file>.<id>.eco-old`, and deleted at once when nothing holds
  it, else when the daemon next starts (`installer::tidy`). A file another
  program has open for a moment (a window rereading a language pack it
  watches, a virus scan) is tried again for up to 5 s; one still held then
  fails the install, and what was moved aside is put back. The Start-menu
  shortcut and the `PATH` entry are left as they are. The package is
  downloaded into a folder of its own under the temporary directory, deleted
  when the install ends.
- *Restarting.* Then the daemon restarts on the new binary, as `eco restart`
  does, and a window that was open reopens. The restart never cuts a
  recording: while a session is live it waits (`update_progress` stage
  `waiting`, "restarts when the session ends") and runs once the last live
  session, recording or paused, ends; the daemon looks after every command it
  reads. On Linux the restart runs in a transient unit, `systemd-run --user
  --collect --quiet <eco> restart`, where `<eco>` is the running executable's
  path without the ` (deleted)` Linux adds once pacman replaced it.

**Releases.** Commits follow Conventional Commits, and release-please reads
them (`release-please-config.json`, `.release-please-manifest.json`). Every
pull request merged into `master` runs `.github/workflows/release.yml`, which
opens or updates a release pull request: it moves the version in `Cargo.toml`,
`Cargo.lock` and the `PKGBUILD`, and writes `CHANGELOG.md`. Merging that pull
request tags `v<version>`, and the same workflow builds from the tag, in
three jobs after release-please:

- `package-linux` builds the Arch package with `scripts/package` in an Arch
  container and attaches it to the release.
- `package-windows` runs on GitHub's `windows-2022` image, for the Visual
  Studio 2022 that `scripts/window-build`'s CMake generator names
  (`windows-latest` carries Visual Studio 2026). `jdx/mise-action` installs
  only the tools of `mise.toml` the zip needs (Rust, Python, CMake, uv, aqt),
  without a cache; `mise run package` installs Qt (`qt:install`), builds
  `eco-window` and the zip, and the job attaches
  `eco-<version>-windows-x86_64.zip`.
- `checksums` downloads the release's `eco-*` assets, writes one
  `SHA256SUMS` over all of them with `sha256sum` (`<sha256>  <file>`, the
  format `install.sh` checks) and attaches it. It runs once the Linux package
  is attached even if the Windows one failed, so `install.sh` keeps working.

Every action is pinned by commit sha. On Windows the tools mise installs are
pinned, the compiler and Windows SDK are the image's. The Linux job's
container is `archlinux:base-devel` pinned
by its multi-arch index digest, so the base image cannot change under a tag;
the job's `pacman -Syu` still installs the Rust toolchain, CMake, Ninja, Qt
and the system packages current on the day of the build, so the toolchain
itself is not pinned. `eco-window` builds with warnings as errors, so a newer
compiler or Qt that warns fails the build until the code is fixed. To
move the image, read the `docker-content-digest` header of the registry's
`library/archlinux/manifests/base-devel` and replace the digest and the date
in the workflow. The workflow runs no test; the gate
is local (see [Tests and the gate](#tests-and-the-gate)).

**From a checkout.** `mise run install` (`scripts/install`) builds the release binary and installs it
under a prefix (`~/.local` unless `PREFIX`): the binary, the window program in
`lib/eco` (built first by `window:build`), its own copy of the overlay, the
Hyprland rules, the launcher, icon, shell completions and user service,
with the models and the agent skill prepared first (`eco setup --harnesses
agents,claude-code`). It is idempotent: each install replaces the last, and a
daemon that was running is restarted on the new binary. Sessions, config and
models stay. `mise run uninstall` removes what install put under the prefix and
leaves sessions, config, models and the agent skill.

- **User service:** `packaging/eco.service` runs `eco daemon --headless`
  (`Restart=on-failure`). `eco start` starts it through `systemctl --user`
  (`src/adapters/service_systemd.rs`) when nothing answers on the socket. It
  sets glibc's allocator for a long-running daemon: `MALLOC_ARENA_MAX=2`, so
  threads share two arenas instead of one each, and
  `MALLOC_TRIM_THRESHOLD_=131072`, so memory freed after a peak (a long
  session read back, speakers clustered) goes back to the system instead of
  staying resident. Environment, not `malloc_trim` called from the daemon,
  which would take `unsafe` code. A daemon run some other way, and the daemon
  on Windows, keep their allocator's defaults.
- **Hyprland:** `packaging/hypr/eco.lua`, loaded from
  `~/.config/hypr/bindings.lua` by one line `eco setup` adds
  (`src/adapters/desktop_hyprland.rs`):
  the line ends in `-- eco setup`, names the `share/eco/hypr/eco.lua` beside the
  running binary, and runs `dofile` only when `io.open` finds that file, so a
  removed package leaves no error. Setup rewrites only its own line, adds none
  when another line already names an `eco.lua`, and creates no `bindings.lua`.
  A change keeps `bindings.lua.bak.<unix seconds>` first, then replaces the
  file (through a symlink, its target) atomically in its own mode, every other
  byte kept; a file it cannot read or write is a warning and exit status 1
  after the rest of setup ran.
  The file binds `SUPER+ALT+<n>` to the actions,
  `SUPER+ALT+N` to a new session, `SUPER+ALT+P` to pause/resume, `SUPER+ALT+H`
  to SESSIONS, `SUPER+ALT+C` to the config window and `SUPER+ALT+E` to bring eco
  to the front; those that open a view also give eco the keyboard. Each runs an
  `eco window` command (§11), never a second daemon: those that open a view
  send `window.call` (§10.2), and the daemon focuses the window. It holds the window rules,
  matching the class `eco`: the overlay (`float`, `pin`, `persistent_size`,
  opaque) and the config window (`float`, `pin`, centered, opaque). The config window opens
  above the overlay; it is not its child, so a shortcut that focuses the
  overlay also raises it above the settings, and a dialog it opens shows.
  Without the rules, Hyprland tiles both.
- **Screen sharing:** off by default, eco's windows show in captures.
  `[ui] hide_from_share` (Settings › General) makes the daemon set
  Hyprland's `no_screen_share` on the windows of every `eco-window` it started
  (the overlay and its config window, found by pid) through the Lua dispatcher
  (`hyprctl dispatch`, in `src/adapters/window_hyprland.rs`): when a window
  process opens, when the saved setting changes and, while it is on, when
  Hyprland's event socket reports an `eco` window opening. The window itself
  never calls Hyprland. Hyprland cannot tell a screen share from
  a screenshot, so they show black in the user's screenshots and recordings too.

---

## 14. What this is, and what it is not

- It is not a bot that joins the meeting, and it does not record raw audio:
  audio is never written to disk, imports are decoded through a pipe, and voices
  are kept only as embeddings.
- It is released for Linux (Omarchy, Hyprland, PipeWire) and as a zip for
  Windows, where it builds and passes its tests and several platform seams
  have no adapter yet (§15); macOS is not supported. A Windows machine on the LAN that serves
  models over HTTP is reached through the same adapters as any provider.
- Transcripts stay local in `~/.local/share/eco/`; `--no-save` keeps a session
  in memory only. Secrets are never logged, nor transcript text at info level.
  No telemetry: the one request eco makes of its own is the daily GET for the
  latest release's tag (§13), which `[update] check = false` turns off.
  A system notification (`[ui] notifications`, §12.2) stays on the machine and
  carries no secret and no transcript text: an answer is named by its skill or
  question, never its text.

The targets the design is held to:

| Requirement | Target |
|---|---|
| End of speech → first suggestion token | **< 1.5 s** (fast model) |
| Idle CPU usage | < 10% |
| Cost per 1 h meeting (Deepgram + Gemini Flash, manual mode) | < US$0.50 |
| Privacy | No audio on disk; transcripts local in `~/.local/share/eco/` |

Cost: two continuous Deepgram streams cost 2 × 60 min × US$0.005 = US$0.60/h,
above target. The target depends on sending only speech segments (VAD) and/or
transcribing the "me" channel locally.

---

## Measured, and what it cost

The measurements live in [benchmarks.md](benchmarks.md): LLM latency, the LAN
models, the prompt cache, the Rust daemon against the Python one, diarization,
import, naming people by voice, streaming transcription, concurrent sessions
and the overlay's cost. Answer quality has its own tool, `mise run bench`
([benchmark/README.md](../benchmark/README.md)). Where a number above changed
the design it is stated beside the decision: the Deepgram and Scribe phrase
latencies (§5), the 0.70 guess threshold against the 0.52 closest wrong person
(§7.3), echo cancellation's ~3% of a core (§4), and the input trace's `Shape`
at ~12% of a core against a `Canvas` at ~77% (§12.4).

---

## Working on eco

From a clone of the repository. mise pins the Rust toolchain and owns every
command:

```bash
mise install
mise run setup              # build the daemon, download the VAD and speaker models
mise run start              # build and run this checkout with its window
mise run start -- --replay session.wav   # a 16 kHz mono WAV instead of a call
mise run start -- --headless             # no window; pair with `mise run overlay`
mise run start -- --no-save              # keep sessions in memory only
mise run overlay            # only the window; start it after the daemon
mise watch overlay -o restart   # the same, run again on every save under overlay/
mise run window:build       # eco-window, into target/window (the tasks that run it build it first)
mise run qt:install         # Windows only: the Qt eco-window builds with, into .qt
mise run check              # the gate: lint, test, cli:check, docs:check
mise run lint               # cargo fmt and clippy, every warning fatal
mise run test               # the domain tests, i18n, VAD and speaker parity
mise run coverage           # per-file test coverage with cargo-llvm-cov (not part of check)
mise run cli:gen            # docs/cli.md, from the binary's usage spec
mise run docs:check         # every relative link and picture resolves
mise run shots              # docs/img, from the real window, offscreen
mise run preview:controls   # the control lab: every kit component
mise run bench              # answer quality per model; benchmark/README.md
mise run bench:llm          # model latency on OpenRouter
mise run bench:diarization <dir>   # diarization error rate on WAV + RTTM
mise run readme:gif         # the README icon, from packaging/eco.omapixel
mise run icon               # packaging/windows/eco.ico, from packaging/eco.svg
mise run install            # the release build, under ~/.local (PREFIX moves it)
mise run package            # the Arch package of HEAD, into target/arch/pkg; on Windows the zip, into dist
mise run hooks:install      # run the gate on every push, and post local-check
mise run local-check        # gate HEAD and post local-check by hand
mise run uninstall          # asks first, then takes that install back off
mise run install:windows    # Windows only: the zip, installed by install.ps1 in %LOCALAPPDATA%\Programs\eco, on your PATH
mise run uninstall:windows  # Windows only: takes it off; asks whether to delete your data too
```

**A checkout is enough to try eco.** `mise run start` runs the daemon and its
window in the foreground, until Ctrl+C, and installs nothing. It prints the
transcript, notes and answers as they come; the user service prints none of
them, so the journal holds no conversation. `mise run
install` is §13: it puts everything under `~/.local`, and the Hyprland rules
are then loaded with
`dofile(os.getenv("HOME") .. "/.local/share/eco/hypr/eco.lua")` from
`~/.config/hypr/bindings.lua`. Installing again is the upgrade. `mise tasks ls`
lists the rest.

**Nothing checks the pictures.** `mise run shots` writes `docs/img`, and no task
compares them with what the window draws today, as `cli:check` does for
`docs/cli.md`. Run it after any change to the overlay;
[the window, screen by screen](screens.md#how-this-page-stays-true) says what
in a picture differs from a real screen.

---

## Tests and the gate

```bash
mise run check
```

That is the whole gate. It runs:

| step | what it refuses |
|---|---|
| `lint` | Rust not formatted by `cargo fmt`, any clippy warning (`-D warnings`, all targets) |
| `test` | `eco-window` that does not build without a warning (`window:build`), a failing `cargo test`: the domain tests, the i18n checks (`tests/i18n.rs`), the answer Markdown (`overlay/Eco/Core/markdown.js`) against its QML test case (`tests/overlay.rs`, run offscreen by Qt's `qmltestrunner`), eco-window's bridge offscreen (`tests/window.rs`: lines each way, on Windows the `auth` line first, the reconnect, the pid, a local path, a watched file, a clean exit) and the VAD and speaker parity tests against the Python fixtures (these need the models from `mise run setup`) |
| `cli:check` | a `docs/cli.md` that is not what the binary's usage spec generates |
| `docs:check` | a relative link or image in `README.md` or `docs/*.md` whose file, or whose heading for an `#anchor`, is missing |

**The gate guards `master`, from this machine.** Nothing on GitHub runs it: it
needs the pinned toolchain and the models. `mise run hooks:install` points git
at `.githooks`, whose `pre-push` runs `scripts/local-check --pre-push` for the
commit being pushed: a red gate refuses the push, and a green one posts the
`local-check` commit status. `master` is protected: a pull request merges only
when its head carries `local-check`, and nobody pushes to it directly or
rewrites it. An administrator can merge past the rule; that is how the release
pull request, which no hook ran on, is merged. To gate it instead, check its
branch out and run `mise run local-check`, which gates that commit and posts the
status. `ECO_SKIP_LOCAL_CHECK=1` or `git push --no-verify` skips the hook.

mise pins CMake, Ninja and watchexec too; on Linux the C++ compiler and Qt 6
are the system's, the ones Arch builds Qt with.

**On Windows** a checkout builds and tests the daemon and its window, with
the MSVC Build Tools 2022 (the C++ workload) installed; CMake finds them
through its Visual Studio generator, with no developer shell. `mise install`
also pins uv and aqt there, aqt at a commit, for no release of it reads Qt
6.11's repository layout. `mise run window:build` first runs `mise run
qt:install`, which installs Qt 6.11.2 for MSVC 2022 (`qtbase` and
`qtdeclarative`, which hold every module the overlay imports) under `.qt` in
the checkout: about 2 GB with Qt's debug libraries, in a minute on the
machine it was measured on. It survives `cargo clean`; deleting `.qt` takes it
back. `mise run package` builds the zip of §13; `mise run install:windows`
installs it with `install.ps1`, as a release is installed, and `mise run
uninstall:windows` takes it off
([how](how-to-install-and-remove.md#on-windows)); §13 says how the `PATH` is
kept.
On Windows mise runs a task's command through `cmd`, which runs no script
by its shebang, so a task that runs there names a Python script, on the
Python mise pins, in its `run_windows`: besides the tasks above, `mise run
check` and the tasks it runs, `format`, `coverage` and `cli:gen` run from
plain PowerShell. The others are bash scripts, for Linux alone; Windows ships
no bash, and the first `bash` on its `PATH` may be WSL's, as it was on the
machine eco is tested on. Recorded audio replays with
`--replay <file.wav>` instead of joining a call.
`mise run preview:controls` opens the control lab, and `mise run shots`
regenerates `docs/img` from the real overlay against an isolated daemon.

---

## Decisions

- **Stack:** Rust (daemon, rewritten from the Python v1) + QML on Qt 6
  (overlay), run by a small C++ host, so the window depends on Qt alone
  (§3).
- **Architecture:** light hexagonal (ports/adapters) in the daemon.
- **Default STT:** Deepgram. Development: whisper.cpp + Vulkan on the LAN server
  (not faster-whisper).
- **Default trigger:** manual.
- **GPU:** no dedicated GPU on the laptop; Radeon 860M iGPU.
- **Interface:** QML overlay instead of a TUI.
- **Development:** on the laptop; Windows is built and tested by hand on a
  Windows machine, which can also serve models.
- **Screen sharing:** a setting, off by default — Hyprland's `no_screen_share`
  and Windows' capture exclusion also hide eco from screenshots.
- **Sessions:** transcription and storage only inside a session (outside,
  inputs are only measured for the live trace); removed suggestions disappear
  and leave the context; an unfinished session is marked paused and shown as
  interrupted, never reopened by itself.
- **Answers:** user-defined actions (prompt + format + optional model) instead
  of fixed fast/deep modes.
- **Mic echo:** both answers are offered — dropping echoed lines (on by
  default) and PipeWire echo cancellation (off by default); headphones are
  still recommended.

## Open questions

- Validate whether OpenRouter accepts audio input for Gemini (decides whether a
  `gemini` backend exists).
- Cost strategy for the "me" channel: VAD + Deepgram vs. local STT.
- An automatic trigger: question heuristic on the "them" channel (sentence ends
  with `?` or patterns such as "o que você acha", "pode explicar", "how would
  you"), followed by end of speech (VAD), with debounce. Not built.
- A running recap: when a segment leaves the window, the default model updates a
  recap asynchronously, off the action's critical path. Not built.

---

## 15. Platform seams

A *platform seam* is a port in `src/ports.rs`, the window host's bridge, or a
function, with a Linux and a Windows adapter. AGENTS.md's rule — add a port
only when two real adapters exist or a test needs the seam — makes an
explicit exception for these seams. Each seam's adapter is chosen with
`#[cfg]` in one place: the composition in `src/session.rs`, or the module
that owns the seam; nothing else names a platform.

eco builds and passes its tests on Windows. The table says where each seam's
adapter lives on each platform today.

| Seam | Linux | Windows | File(s) |
|---|---|---|---|
| Paths | the home and the XDG base directories (`HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_RUNTIME_DIR`, else `/run/user/<uid>`), and Hyprland's event socket under the runtime directory (`HYPRLAND_INSTANCE_SIGNATURE`) | the user profile (`USERPROFILE`), the config under `%APPDATA%\eco`, the data and the windows' token under `%LOCALAPPDATA%\eco`, each else under the profile's `AppData`, and the pipe `\\.\pipe\eco-<USERNAME>` | `src/paths.rs` |
| Private files | files `0600` and directories `0700` | plain files and directories: under the user profile they inherit an access list that admits only the user, the system and administrators | `src/config.rs` |
| The daemon socket | a Unix domain socket at `$XDG_RUNTIME_DIR/eco.sock`, `0600`; a socket file a crashed daemon left is replaced | a named pipe that refuses clients on other machines; its first instance is created as the first, so a second daemon is refused while one runs, and the pipe goes with its last handle; a new instance waits for the next client before each accepted one is handed out, and a client finding every instance taken tries again for two seconds. The pipe keeps Windows' default access list, which lets any local account open it (setting its own takes a call tokio offers only as `unsafe`), so a client is admitted only once its first line is `auth <token>`, the token in `%LOCALAPPDATA%\eco\eco.token`; anything else is disconnected without a byte (§10) | `src/adapters/local_socket.rs`; the admission and the protocol (`src/adapters/control_socket.rs`), the CLI client (`src/cli.rs`) and the service commands (`src/lifecycle.rs`) reach it only through it |
| Audio devices and capture | the `AudioDevices` port, adapter `PipeWire`: devices from `pw-dump`, an `AudioSource` per device through `pw-record`, echo cancellation by `libpipewire-module-echo-cancel` loaded through `pw-cli` | adapter `Wasapi`, on WASAPI through the `cpal` crate: the devices are the active endpoints, each with its endpoint id (`{0.0.1.00000000}.{…}`) and friendly name, a capture endpoint as `input` and a render endpoint as `output`, after the same two defaults; `@default-input` and `@default-output` follow the system default (the daemon names them `Default microphone` and `Default output`, in English as its messages are; the overlay names them by id in the interface language), and when it changes the stream opens again on the new one. An output is captured in loopback, beside a silent stream of eco's own on that output, because WASAPI hands a loopback nothing while its output plays nothing. Capture runs on cpal's thread in the device's own format (often 48 kHz float stereo); `src/adapters/audio_convert.rs`, platform-neutral and tested on both, averages the channels, resamples to 16 kHz with a Blackman-windowed sinc (rational polyphase, cutoff at 90% of the lower Nyquist) and cuts 512-sample frames across chunks. Frames reach the session through a queue of 256 (about 8 s) that drops new audio rather than make the device's thread wait; a device that cannot be opened or goes away is reported as `capture.down` and opened again, while the other inputs keep capturing (§4). Echo cancellation is not available: `cancel_echo` fails with `echo_cancel.unavailable`, and the "cancelled" microphone is the default microphone as it is. Measured on the Windows machine over ssh: the endpoints listed, one second of the default microphone gave 30 frames and of the default output 31 silent frames, and a 440 Hz tone at 0.2 played on it came back with a peak of 6554 | `src/session.rs` composes it; `src/adapters/audio_pipewire.rs`, `src/adapters/pipewire_devices.rs`, `src/adapters/echo_cancel.rs`, `src/adapters/devices.rs` (both), `src/adapters/audio_wasapi.rs`, `src/adapters/audio_convert.rs` |
| Service lifecycle and desktop setup | `ServiceManager`: the systemd user service (`systemctl --user`), chosen in `src/main.rs`; `DesktopIntegration`: the line loaded into the Hyprland Lua config (`~/.config/hypr/bindings.lua`), chosen in `src/setup.rs` | `ServiceManager`, adapter `BackgroundProcess`: `eco start` has PowerShell's `Start-Process -WindowStyle Hidden` run `eco detach`, which runs `eco daemon --headless` from the same executable as a process of the user's own and returns. A process Rust's standard library starts inherits every inheritable handle of its parent, and a shell or ssh reading `eco start`'s output waits until the last holder of that pipe exits: started directly, or through `cmd /c start`, the daemon held it and `eco start 2>&1 \| Out-String` never returned; through `Start-Process`, which inherits nothing, it returns once PowerShell does (measured over ssh: 12 s against 0.2 s with a 12-second program). The daemon runs with `CREATE_NO_WINDOW` (a console of its own that is never shown, which the programs it runs, a hook's `cmd` say, share instead of each opening one; `DETACHED_PROCESS` would leave it none, and Windows ignores `CREATE_NO_WINDOW` beside it), out of reach of a Ctrl-C in the terminal that ran `eco start`; its output goes to `%LOCALAPPDATA%\eco\daemon.log`, emptied at each start, and `eco start` waits for the pipe as on Linux; stop, status and restart go through the pipe. `DesktopIntegration`, adapter `OldLoginScript`: eco starts only when the user opens it, so `eco setup` removes the `eco.cmd` an earlier `eco setup` wrote into the user's Startup folder, says so, and says nothing when there is none. Adapter `StartMenu`: `eco setup` saves the shortcut `eco.lnk` in the user's Start menu through PowerShell and the shell's `WScript.Shell` COM object, the values handed in variables; it runs `conhost.exe --headless "<eco.exe>" start`, a console host without a window, with the icon of `eco-window.exe`, and a rerun leaves a shortcut with the same fields alone. Adapter `ToastName`: `eco setup` registers eco's app id for the notifications (below). `setup::desktop()` lists the three; Linux lists Hyprland alone | `src/adapters/service_systemd.rs`, `src/adapters/desktop_hyprland.rs`, `src/adapters/service_windows.rs`, `src/adapters/desktop_windows.rs` |
| Stopping the daemon and its windows | the daemon stops on Ctrl-C or `SIGTERM`; a window process is sent `SIGTERM` | the daemon stops on Ctrl-C, Ctrl-Break, its console closing or Windows shutting down; a window process is terminated | `src/session.rs`, `src/adapters/overlay.rs` |
| The running executable | `eco status --expect-current-exe` compares the daemon's `/proc/<pid>/exe` with this executable by device and inode | it compares the `exe` the daemon's greeting names with this executable's path; Windows lets no one overwrite a running executable | `src/lifecycle.rs` |
| Window control | the `WindowControl` port, adapter `HyprlandWindows`: giving a window process's overlay (its window titled `eco`, never the settings window) the keyboard and raising it above the settings, so a dialog the shortcut opens shows and setting `no_screen_share` on its windows through `hyprctl dispatch`, by pid, again on Hyprland's `openwindow` events while they are hidden; composed in `src/session.rs`, used by `src/adapters/overlay.rs`, which launches `eco-window`. The window rules in `packaging/hypr/eco.lua` centering the config window stay compositor config | adapter `HostWindows`: the windows control themselves. Focusing sends `window_focus` with the pid and hiding sends `window_share` with the pids (§10.1), to every client; the window of that process answers through its host, `Host.raise` on its eco window, or `Host.hideFromCapture`, and the last of each is told again to a client that connects, as a window does just after it opens. Windows gives the foreground only to a process that has it or had the last input, and neither the daemon nor its windows, in the background, do: `raise()` and `requestActivate()` alone left the window behind another program, its taskbar button flashing (seen in a desktop test). So the host's `Host.raise` first attaches its thread to the input of the foreground window's thread (`AttachThreadInput`), brings the window to the top and calls `SetForegroundWindow`, then detaches, after checking that thread answers (`SendMessageTimeout`), as an attached hung thread would hang the window: the workaround Qt applies under its private `AlwaysActivateWindow` option. Chosen over a synthetic Alt press before `SetForegroundWindow`, which reaches the program in front (an Alt press and release opens its menu bar), and over the daemon passing the right on with `AllowSetForegroundWindow` from the hotkey's thread, which has no safe binding in Rust and would not cover `eco window` from a terminal. Leaving out of capture needs Windows 10 2004 or later, and leaves the windows out of the user's own screenshots and recordings too. Qt calls a window active that Windows left behind another program, and kept it so: the daemon sent no system notification while the user was in another program (seen in a desktop test). So `Host.foreground` says whether the system's foreground window is one of the process's, followed through `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, and a window is active for `window.active` only while it is; on Linux it is always true and Qt's word holds | `src/ports.rs`, `src/adapters/window_hyprland.rs`, `src/adapters/window_host.rs`, `src/adapters/overlay.rs`, `window/host.h`, `packaging/hypr/eco.lua` |
| The window host's bridge | `eco-window`'s `EcoHost` module, built with the system's compiler and Qt by Ninja: `Host` gives the QML its pid, its environment and local paths, and copies to the Wayland clipboard through `wl-copy`; `LineSocket` is a `QLocalSocket` to the daemon socket the daemon names in `ECO_SOCKET`; `TextFile` reads and watches files, such as the Omarchy theme under `XDG_STATE_HOME` that `overlay/Eco/Core/Theme.qml` follows | the same module, built with MSVC through CMake's Visual Studio generator against the Qt `scripts/qt-install` puts under `.qt`, with Qt's libraries copied beside `eco-window.exe` by `windeployqt`; `src/adapters/overlay.rs` runs `<prefix>\lib\eco\eco-window.exe` beside an installed `eco`, else the checkout's `target\window\eco-window.exe`. `Host.copy` puts the text on the clipboard through Qt and flushes it to Windows (`OleFlushClipboard`, from `ole32`), `Host.hideFromCapture` sets the windows' display affinity, and `LineSocket` connects to the named pipe and sends `auth <token>` first. `tests/window.rs` and `tests/overlay.rs` run there too, the second with the `qmltestrunner` under `.qt`. There is no Omarchy theme, so the window keeps its own colours | `window/main.cpp`, `window/host.h`, `window/line_socket.h`, `window/text_file.h`, `scripts/window-build`, `scripts/qt-install` |
| The tray icon | `Qt.labs.platform`'s `SystemTrayIcon`, which Qt exports as a StatusNotifierItem with a D-Bus menu (qt6-base carries Qt D-Bus); the bar's tray shows it | the same type, a notification area icon whose menu is a Qt Widgets menu; `windeployqt` ships Qt Widgets and the `Qt.labs.platform` plugin with `eco-window.exe` | `overlay/tray.qml`, `src/adapters/overlay.rs` |
| System notifications | the `Notifier` port, adapter `notify-send`: `notify-send` with the app name `eco`, the installed eco icon and the urgency (`normal`, or `critical` for an error); a missing `notify-send` is written to the daemon's log | adapter `Toasts`: a Windows toast notification through the WinRT `ToastNotificationManager` of the `windows` crate's safe projection, `long` for an error, under eco's app id `This-Is-NPC.eco`. eco is not a packaged app, so the app id is registered for the user in `HKCU\Software\Classes\AppUserModelId\This-Is-NPC.eco`, `DisplayName` `eco` and `IconUri` the shipped `share\eco\eco.ico`, through the `windows-registry` crate's safe interface: Windows then labels the toast "eco" with eco's icon, with no Start-menu shortcut. `eco setup` writes the key (adapter `ToastName`), and the notifier writes it again before each toast, writing nothing when it holds these values, so a daemon whose setup never ran or whose key was deleted still shows "eco"; a build that is not installed ships no icon and leaves a registered one alone. A key it cannot write sends the toast under Windows PowerShell's app id, which Windows always has, labelled "Windows PowerShell", and says why in the daemon's log. `install.ps1` deletes the key when it takes eco off. Both chosen with `#[cfg]` in `src/session.rs`. Only with `[ui] notifications` on and no eco window active (§12.2) | `src/ports.rs`, `src/domain/notifications.rs`, `src/session.rs`, `src/adapters/notify_linux.rs`, `src/adapters/notify_windows.rs` |
| Shortcuts | a global Hyprland bind that runs an `eco window` command, which sends one line to the socket and returns once the daemon has read it: `window.call` for those that open a view, the daemon giving that window the keyboard | the daemon registers `[shortcuts]` (§9) itself, `Win+Alt+A`, `Q`, `C`, `N`, `P`, `H` and `E` by default, the keys of `eco.lua` with the Windows key for SUPER, none of them one Microsoft documents for Windows (`Win+Alt` with a digit opens a taskbar jump list, and was refused on a desktop; the Game Bar holds `G`, `M`, `R` and `T`; Omarchy binds `SUPER+ALT+1` to `5` to a group's windows), through the `windows-hotkeys` crate: `RegisterHotKey` on a hidden message-only window of a thread of its own, which waits for `WM_HOTKEY` in the crate's loop. A pressed key hands the daemon's loop the very line `eco window` builds (`cli::Window::line`), so it is routed as the command would be. A key the system refuses, another program holding it, is written to the daemon's log and greets every client as an `error` with the code `shortcut.unavailable`, which names the config's path, to be created if missing, as the settings window has no place for shortcuts; the others still work. Chosen over `global-hotkey`, which registers the same way but leaves the message loop to the application, and that loop is `unsafe` code eco forbids, and over `livesplit-hotkey`, which installs a low-level keyboard hook: it sees every key pressed in every program, passes the shortcut on to the program in front as well (Win+Alt+1 would also open the first taskbar program's jump list) and cannot tell a key another program holds (read in the crates' sources). The keys are read when the daemon starts. Over ssh the system refuses every key: that session has no interactive window station (`RegisterHotKey` fails with error 1459, `ERROR_REQUIRES_INTERACTIVE_WINDOWSTATION`, measured), so each is reported; on a desktop, `Win+Alt+C`, `N`, `H` and `E` registered and worked when pressed (seen in a desktop test). `eco window` works as on Linux, knowing the daemon has read its line from the `synced` answer to `sync` (§10) | `packaging/hypr/eco.lua`, `src/cli.rs`, `src/shortcuts.rs`, `src/adapters/shortcuts_windows.rs` |
| The hook shell | `shell()` runs each hook with `sh -c` | `shell()` runs each hook with `cmd /S /C "<command>"`, the command handed over as written, for cmd does not read its line by the quoting rules other programs use; a hook reads `%ECO_SESSION%` and `%ECO_TITLE%`. cmd is on every Windows | `src/adapters/hook_shell.rs` |
| The omapass keyring | `api_key_omapass` reads a key through the omapass CLI | omapass does not exist: it is never installed, and a key from it fails | `src/adapters/omapass.rs` |
| Installing a release | `pkexec pacman -U --noconfirm` on the Arch package, else `sudo pacman -U` from a terminal; Hyprland's binds are what `eco doctor` checks for shortcuts | `%LOCALAPPDATA%\Programs\eco` replaced in place, the running `eco.exe` renamed aside and deleted at the next start; the registered keys are what `eco doctor` checks | `src/ports.rs` (`Releases`, `Installer`), `src/installer.rs`, `src/adapters/releases_github.rs`, `src/adapters/installer_linux.rs`, `src/adapters/installer_windows.rs`, `src/session/update.rs`, `src/doctor.rs` (§13) |
| The Windows Credential Manager | none: `api_key_credential` and `eco key set`/`remove` fail, naming omapass and the environment; the `keys` event's `credentials` is `null` | `api_key_credential` reads the generic credential `eco:<name>`; `eco key set`/`remove` write and delete it; the `keys` command lists them by name (§9.1) | `src/adapters/credentials.rs` |
