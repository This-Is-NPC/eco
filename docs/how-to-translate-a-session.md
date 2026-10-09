# How to translate a session

**The question:** the meeting is in English and I want Portuguese under every
line as it is said — or a stored session in Spanish read back in English. How?

This page is enough on its own. It ends with one session translating, and the
others left alone.

---

## Before you start

- **The daemon is running.** `eco status` says
  `eco: daemon running (pid …, version …)`.
- **A chat model is registered.** Translations are written by an LLM, through
  the same OpenAI-compatible adapter as answers. [How to register
  models](how-to-register-models.md).
- `90a8bbae6b73` is the session's id throughout this page; yours comes from
  `eco sessions`. Its lines are in English.

**Translation is turned on per session, and only there.** No setting translates
every session; a new session starts with it off. That is on purpose: each
translation is a request that costs money, and most sessions do not need one.

---

## 1. Turn it on

```bash
eco translate 90a8bbae6b73 --lang pt
```

```json
{"ok":true,"data":{"session":"90a8bbae6b73","translating":"pt"}}
```

It answers at once. What the session already holds is translated in the
background; from now on each line, once saved, and each answer, once finished,
is translated as it comes. Up to three translations run at a time, apart from
transcription and answers, so a live session does not wait on them.

The code is one of the languages eco translates into: `pt`, `en`, `es`, `fr`,
`de`, `it`, `nl`, `pl`, `ru`, `uk`, `tr`, `ar`, `hi`, `ja`, `ko`, `zh`, `sv` —
other than the session's own. The window offers the same ones.

## 2. Read it back

```bash
eco show 90a8bbae6b73
```

Each line and answer that has its translation carries it:

```json
{"type":"transcript","session":"90a8bbae6b73","who":"Speaker 1","name":"Ana Ribeiro","text":"Let's start with the onboarding plan for Acme.","at":1791499971.3514547,"latency_ms":0,"translation":"Vamos começar com o plano de integração da Acme."}
{"type":"transcript","session":"90a8bbae6b73","who":"Speaker 2","name":"Tom Becker","text":"The VPN access is still pending on their side.","at":1791499975.8514547,"latency_ms":0,"translation":"O acesso à VPN ainda está pendente do lado deles."}
```

and `session.translating` is `"pt"`. A line with no `translation` yet is one
still on its way.

**`text` is what was said; `translation` is a model's.** Quote the first.

## 3. Change the language, or stop

```bash
eco translate 90a8bbae6b73 --lang es    # another language from now on
eco translate 90a8bbae6b73 --off        # stop
```

```json
{"ok":true,"data":{"session":"90a8bbae6b73","translating":null}}
```

Translations are kept in the session's log (`translated` records), one per
line and language. Going back to a language translated before shows its
translations again at once, without asking for them twice. Correcting or
removing a line drops its translation.

---

## In the window

Open the session's details (`⋯`) and pick a language on its **TRANSLATION**
line, which reads `TRANSLATION: OFF` until you do:

![The details of "Acme onboarding kickoff" with its TRANSLATION menu open: OFF marked, then PT · português, ES · español de España, FR · français, DE · Deutsch, IT · italiano, NL · Nederlands, PL · polski, RU · русский, UK · українська, TR · Türkçe, and more below the window's edge.](img/24-session-translation.png)

The menu leaves out the session's own language: this one is spoken in English.

The translations show below each line and answer, dimmer and in italics:

![A stored meeting, "Revisión de precios", with Spanish lines from Sofia Alvarez on the left and from you on the right, each with its English translation in dim italics beneath it; at the bottom an ASK card, "¿Qué falta decidir?", with its answer in Spanish and the English beneath.](img/25-session-translated.png)

An answer's card has a translate button of its own, for one answer in a session
that does not translate: it goes into the session's translation language when
there is one, else into the interface's.

The model that writes them is chosen in **Settings › Translation**:

![Settings on the Translation tab: "A session translates its lines and answers only once it is turned on in that session, on the TRANSLATION line of its details. Here you pick the model that writes the translations." and a DEFAULT MODEL dropdown on gemini-flash.](img/47-settings-translation.png)

---

## Which model translates

The first that applies ([design.md §6](design.md#6-answers)):

1. the session kind's own translation model — in **Settings › Sessions**, or
   `translates = ["idea"]` on a chat model in `config.toml`;
2. the default translation model, `[translation] model`;
3. the assistant's model, `[llm] model`.

Each translation is one request with only the text to translate — no
transcript, no context — so it stays fast and cheap. **Translations never reach
the context of later answers:** a question asked in a translating session is
answered from what was said, not from the translation.

---

## What it costs

Every translation is a `spent` record with what the provider reported, and it
counts in the session's LLM part. This session's four translated lines and one
answer came to:

```bash
eco sessions --search "Acme kickoff"
```

```json
"cost":{"llm_usd":0.0002215,"llm_unknown":false,"transcription_usd":0.0,"transcription_unknown":false,"total_usd":0.0002215,"transcribed_s":{}}
```

The session's cost screen shows the translations as their own part.
[How to read what a session cost](how-to-read-what-a-session-cost.md).

---

## What can go wrong

**The model fails.** The line stays without a translation and the window says
`Translation failed:` with the provider's reason. Nothing falls back to another
model.

**A code eco does not translate into, or the session's own, is refused.** On a
session in Portuguese:

```bash
eco translate a110f46d050d --lang zz
eco translate a110f46d050d --lang pt
```

```json
{"ok":false,"code":"translation.unknown","message":"eco does not translate into \"zz\""}
{"ok":false,"code":"translation.own","message":"the session is already in \"pt\""}
```

The translation stays as it was.

---

## Next

- [How to read what a session cost](how-to-read-what-a-session-cost.md) — where
  the translations show up.
- [How to register models](how-to-register-models.md) — a cheaper model for
  translation alone.
- [Every screen the window draws](screens.md).
- [The command line](cli.md) — `translate`.
