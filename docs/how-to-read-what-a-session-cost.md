# How to read what a session cost

**The question:** an hour of meeting, a dozen answers and a translation running
under it — what did that cost, and which part of it?

This page is enough on its own. It says where the numbers come from, which ones
are reported and which estimated, and why a part can be unknown.

---

## Before you start

- **The daemon is running.** `eco status` says
  `eco: daemon running (pid …, version …)`.
- **For transcription costs from Deepgram, its key can read usage.** The key in
  `DEEPGRAM_API_KEY` (or in omapass) needs the `usage:read` scope: Deepgram
  tells a request's cost only to a key that has it. Without it eco still
  transcribes, and the transcription part is unknown, with that reason, unless
  the model has a fallback price per minute.
- `dea44e92fd25` is the session's id throughout this page; yours comes from
  `eco sessions`.

**A cost is shown in two places, and only there:** the session's cost screen in
the window, opened from inside the session, and `eco sessions` in the CLI. The
sessions list, the session header and the answer cards never show one.

---

## 1. Read it from the CLI

```bash
eco sessions --search "Cost check" | jq -c '.data[0] | {id,title,cost}'
```

```json
{"id":"dea44e92fd25","title":"Cost check","cost":{"llm_usd":0.0004614000000000001,"llm_unknown":false,"transcription_usd":0.004983900763193766,"transcription_unknown":false,"total_usd":0.005445300763193766,"transcribed_s":{"deepgram":282.4358010292053}}}
```

| field | what it is |
|---|---|
| `llm_usd` | answers, reviews and translations, as their provider reported them |
| `transcription_usd` | transcription, as the provider reported it, or at the price you gave |
| `total_usd` | the two added up |
| `transcribed_s` | seconds of audio each transcription model heard — once per input, as every input streams on its own |
| `llm_unknown`, `transcription_unknown` | `true` when part of that side is **left out** of the total |

All amounts are US dollars. **An `_unknown` that is `true` means the number
beside it is a floor, not the cost.** Here both are `false`: under half a cent
for a five-minute meeting with four answers, translated into English.

## 2. Where each number comes from

**Answers, reviews and translations: what OpenRouter reports.** Each request
asks the provider for its usage, and each answer, reviewer pass and translation
appends a `spent` record to the session's log with the cost the provider sent
back. eco does not keep a price list for chat models. A provider that reports no
cost — OpenAI's own API, which sends only token counts, or a LAN
`llama-server` — leaves that charge unknown, and there is no price per token to
fall back on: route answer models through OpenRouter when the cost matters. A
removed answer stays in the cost: it was paid.

**Transcription: what Deepgram's API reports.** A streaming connection to
Deepgram is one request, named by the `dg-request-id` header it opens with; each
segment of an imported file is one request too, named in Deepgram's reply. The
session's log keeps each request as it opens. When it closes, eco asks Deepgram
what it cost — `GET /v1/projects/{project}/requests/{request}`, the project
being the key's first — 5 seconds later, then after waits about as long as the
request has been closed, up to an hour apart, for a day, off everything else.
Until Deepgram answers, the charge is *not reported yet*; a request it has not
listed after a day is *not reported*.

The requests still waiting are in the logs, so a restart does not lose them:
the next start asks for them again, the waits going on from each request's age.
A request still open when the daemon stopped is closed at the next start, each
session's part read from how long it listened, up to its last line.

Sessions recording at once share one transcriber, so they share its requests:
each session pays its share, by how long it listened while the request was open
([design.md §7.8](design.md#78-cost)).

**A fallback price per minute, when you give one.** A transcription model may
carry `price_per_minute`, in USD. It is empty by default, and it prices only
what no reported cost covers:

- a Deepgram request whose cost is not reported (yet, or ever);
- the time of a model whose provider names no requests — ElevenLabs Scribe, a
  LAN whisper.cpp server, Groq, OpenAI.

A model whose requests were reported is costed by them alone; the price never
adds to a reported cost. A charge from the price is an *estimate*, and the cost
screen marks it `≈`.

**The price is kept with what it priced.** The session's log keeps the price
per minute its model had while it heard the audio, and when each request closed,
so a new price changes only what is heard from then on. A session recorded
before eco kept prices has none in its log and takes the price the model has
now; so does audio heard while the model had no price.

```toml
[[models]]
name = "deepgram"
type = "transcription"
base_url = "wss://api.deepgram.com/v1/listen"
model = "nova-3"
api_key_env = "DEEPGRAM_API_KEY"
price_per_minute = 0.0077                 # USD; only where the provider reports no cost
```

In the window it is the model's **Fallback price per minute, USD (optional)**
field, in **Settings › Models**, on transcription models only.

An import of 35 seconds on that model, with a key that cannot read usage, so
each of its twelve requests is priced at it:

```json
{"id":"b7259be32a56","title":"3102 billed import","duration_s":35,"cost":{"llm_usd":0.0,"llm_unknown":false,"transcription_usd":0.003638506666666667,"transcription_unknown":false,"total_usd":0.003638506666666667,"transcribed_s":{"deepgram":35.32800006866455}}}
```

The twelve segments hold 28.352 s of speech, and 28.352 s at US$0.0077 a minute
is US$0.0036: the silence between them was never sent. A WebVTT import costs
nothing: nothing transcribes it.

---

## In the window

Open the session, its details (`⋯`), and press **COST**. The masthead says
`COST · <title>`; **SESSION** or Esc goes back:

![The cost screen of "Acme onboarding kickoff": TOTAL US$ 0.27 at the top right; "LLM US$ 0.00068  answers US$ 0.00068" and "TRANSCRIPTION US$ 0.27 (35 MIN)"; then three charges — Transcription · 35 min at 14:34 on deepgram, US$ 0.27; Answer · What did we agree on the deadline? at 14:02 on google/gemini-2.5-flash, US$ 0.00027; and Answer · reply at 14:00, US$ 0.00041.](img/28-session-cost.png)

The total, then the LLM part split into answers, reviews and translations, then
transcription with its minutes. Below, every charge, newest first: what it was
— the answer's question or the action's name —, when, which model, and the
amount. A charge with no amount shows `?` and its reason; each reason the
session has is also said once in full above the list, and a note says when a
part is estimated. The screen follows a live session while it is open.

![The cost screen of "Northwind backend interview": "TOTAL ≥ US$ 0.35", since a part is unknown; "LLM US$ ? answers US$ ?" and "TRANSCRIPTION US$ 0.35 (45 MIN)"; in amber, "A provider reported no cost for some calls."; "≈ estimated at the price per minute you set for the model."; then "Transcription · 45 min, 09:45 · deepgram, ≈ US$ 0.35" and, in amber, "Answer · reply, 09:00 · google/gemini-2.5-flash, ? not reported".](img/29-session-cost-unknown.png)

There the provider reported nothing for the answer, so the total is a floor
(`≥`), and Deepgram did not say what the transcription cost, so the model's
price per minute stands in for it (`≈`).

The button is the only way in: neither the sessions list nor an answer card
shows a cost.

---

## When a part is unknown

The reasons, as the cost screen says them in full:

| reason | on the charge | in full |
|---|---|---|
| `no_scope` | `? key can't read it` | The Deepgram key lacks the usage:read scope, so Deepgram cannot tell what transcription cost. |
| `pending` | `? not reported yet` | The transcription provider has not reported its cost yet; eco keeps asking for a day after the request closes. |
| `no_price` | `? no price` | A transcription model reports no cost and has no price per minute set. |
| `unreported` | `? not reported` | A provider reported no cost for some calls. |
| `unrecorded` | `? model unknown` | The log does not say which model transcribed part of the audio. |
| `untracked` | `? not kept` | Answers from before eco kept costs have no cost. |

`no_scope` is fixed at Deepgram: give the key the `usage:read` scope, or set a
price per minute to estimate instead. `no_price` is fixed with a price per
minute. `unrecorded` and `untracked` are sessions recorded before eco kept costs;
nothing can recover them, and a price per minute does not help the audio, since
the log does not say which model heard it.

`pending` settles by itself: eco keeps asking, across restarts, and a request
Deepgram has not listed a day after it closed becomes `unreported`.

---

## Next

- [How to translate a session](how-to-translate-a-session.md) — the
  translations part.
- [How to import a recording](how-to-import-a-recording.md) — a request per
  segment.
- [How to register models](how-to-register-models.md) — where the price per
  minute is set.
- [Every screen the window draws](screens.md).
