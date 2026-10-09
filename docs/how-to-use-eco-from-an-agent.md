# How to use eco from an agent

**The question:** I want to ask a coding agent "what did Acme say about the
deadline last week?" and have it read my sessions — or have it tag them, name
the speakers, import a recording. How does an agent reach eco?

This page is enough on its own. The short answer: **through the `eco` command
line and the `eco` skill that teaches it, and nothing else.** There is no MCP
server.

---

## Before you start

- **eco is installed and the daemon is running.** `eco status` says
  `eco: daemon running (pid …, version …)`. An agent can read sessions only
  through the running daemon.
- **The agent can run shell commands** and reach your user's
  `$XDG_RUNTIME_DIR/eco.sock`. A sandbox that hides that socket gets
  `daemon.access_denied`.
- `90a8bbae6b73` is a session's id throughout this page; the agent finds its
  own with `eco sessions`.

---

## 1. Give the agent the skill

The skill is `skills/eco/SKILL.md`, embedded in the `eco` binary. `eco setup`
publishes it where agent harnesses look for skills:

```bash
eco setup --harnesses agents,claude-code
```

```
eco: /home/you/.local/share/eco/models/silero_vad.onnx is up to date
eco: /home/you/.local/share/eco/models/wespeaker_campplus.onnx is up to date
eco: skill /home/you/.agents/skills/eco/SKILL.md installed
eco: skill /home/you/.claude/skills/eco/SKILL.md installed
```

| harness | where the skill goes |
|---|---|
| `agents` | `~/.agents/skills/eco/SKILL.md` |
| `claude-code` | `~/.claude/skills/eco/SKILL.md` |

[Installing eco](how-to-install-and-remove.md#2-download-its-models) does this
for both. Afterwards **every `eco setup` keeps the
copies current**, with or without `--harnesses`: a copy that is the same says
`is up to date`, an older one says `updated`. A copy is eco's when its front
matter says `owner: eco`; a file of somebody else's at that path is never
overwritten, and setup says so and stops.

The skill tells the agent what a session is, every command, what each prints,
what each error code means, and the rules below. A change to the CLI changes the
skill in the same commit, so the two never disagree.

## 2. What the agent runs

Every command prints one JSON object on one line — `{"ok":true,"data":…}`, or
`{"ok":false,"code":…,"message":…}` with exit code 1 — so an agent checks `ok`
and never parses prose. `eco export` is the one exception: it prints the WebVTT
itself.

**Find the session** by what was said in it:

```bash
eco sessions --search "deadline"
```

or by title, kind, tag or person ([how to find a
session](how-to-find-a-session.md)). When several match, the skill tells the
agent to ask which one.

**Read it:**

```bash
eco show 90a8bbae6b73
```

`{session, timeline, speakers}`: every line with who said it (`who` the label,
`name` who they are) and when, every answer, every note.

**Ask it something.** The answer comes from the model the session answers
with, from its transcript and context, and stays in the session where you see it:

```bash
eco ask 90a8bbae6b73 when is the new deadline
```

```json
{"ok":true,"data":{"session":"90a8bbae6b73","id":"9b87d8e0","action":"chat","prompt":"when is the new deadline","model":"openai/gpt-6-luna","text":"The new deadline is the 21st.","ttft_ms":1380,"total_ms":1570}}
```

`data.text` is Markdown. `eco action <id> <name>` runs one of your skills
instead — `minutes`, say — and waits the same way.

**Leave it a fact** that later answers use:

```bash
eco note 90a8bbae6b73 Acme IT contact is on leave until Monday
```

```json
{"ok":true,"data":{"session":"90a8bbae6b73","id":"ffc56cfb","text":"Acme IT contact is on leave until Monday","at":1791500042.7807467}}
```

The rest — `translate`, `context`, `rename`, `tag`, `speaker`, `assign`,
`participant`, `people`, `line`, `import`, `send`, `delete` — is in [the command
line](cli.md) and in the skill, and the how-to pages show each one with its
output.

## 3. When something is wrong

```json
{"ok":false,"code":"daemon.offline","message":"eco is not running; start it with `eco start` (or `eco start --headless`)"}
```

| code | what the skill tells the agent to do |
|---|---|
| `daemon.offline` | tell you; run `eco start --headless` only if you agree. Never start a second daemon. |
| `daemon.access_denied` | explain it cannot reach the socket from where it runs, and ask for a less restricted environment |
| `daemon.unavailable` | report the connection error, without claiming eco stopped |
| `argument.invalid` | an argument held a line break (a line feed or a carriage return); nothing was sent |
| `action.unknown` | the skill name is not one of your `[[actions]]` |
| `suggestion.removed` | a newer request on the same session replaced this one while it waited |

```json
{"ok":false,"code":"action.unknown","message":"unknown action \"nosuchaction\""}
```

Each session streams one answer at a time: a second request on the same session
waits for the first and sees its answer. Different sessions answer at once, so
an agent may ask several sessions in parallel.

---

## The rules the agent is given

They are in the skill, and they are the point of it:

- **Sessions are your private conversations.** Quote only what the task needs;
  never send their content anywhere you did not ask for.
- **Commands that change things run only when you asked for that**, not to
  explore: `ask`, `action`, `rename`, `tag` (except `tag list`), `import`,
  `speaker`, `assign`, `delete`, `participant` and `people`. `delete` and
  `people forget` cannot be undone.
- **Never guess a name you did not give.** eco's voice guesses are confirmed or
  cleared only when you say so.
- **Live recording is yours.** The CLI cannot start, pause or end a live
  session.
- **Quote the original `text`.** A `translation` is a model's.

---

## Sending an answer elsewhere

A skill of yours may have a hook: a command eco runs with the session's id and
title in `ECO_SESSION` and `ECO_TITLE`, for another system to read the session
with `eco show "$ECO_SESSION"`. When it does not run on its own, an agent sends
an answer to it when you ask:

```bash
eco send 90a8bbae6b73 9b87d8e0
```

It waits until the hook exits and returns `{session, id, sent}`; it fails with
`hook.none` when the answer's skill has no hook, `answer.not_found` when there
is no finished answer with that id, and `hook.failed` with what the command
wrote to stderr.

---

## What this does not do

- **No MCP server, no HTTP API.** The CLI over the socket is the whole
  interface for agents.
- **No hooks, context files or models of its own.** The CLI does not change the
  settings. A program that sends `config.set` on the socket itself and adds or
  changes a hook, adds a context file, or adds a model or changes its address or
  key source, saves nothing: the change waits in the
  eco window until you approve or reject it, and the program gets
  `config.pending` ([screens §12](screens.md#12-settings)). Until you choose,
  any other `config.set` it sends gets `config.busy` and is not saved.
- **No access without the daemon.** The logs in `~/.local/share/eco/sessions/`
  are plain JSON Lines and readable, but the skill sends agents through the CLI,
  which reads them as they read now — corrections, removals and names applied.

---

## Next

- [How to find a session](how-to-find-a-session.md) — the search an agent uses
  most.
- [How to name the speakers](how-to-name-the-speakers.md) — `speaker`,
  `assign`, `people`.
- [The command line](cli.md) — every command and flag.
- [How it is built](design.md) — §10 the socket, §11 the command line.
