---
name: eco
description: Read, ask, organize and import the user's voice sessions — meetings, conversations, sessions, spoken ideas — and name the people who speak in them, kept by eco on Omarchy, through the eco CLI.
metadata:
  owner: eco
---

# eco

eco records what the user says and hears as **sessions**: each has an `id`, a
`title`, a `kind` (`meeting`, `conversation`, `session`, `idea`, or the user's
own), a `source` (`live` or `import`), a `language`, a `state` (`recording`,
`paused`, `importing`, `ended`, or `interrupted`: left open by a daemon that
stopped, and recorded by no one) and a timeline of transcribed speech and AI
answers. Use the `eco` CLI to work with them; there is no other interface for
agents.

## Results

Every command prints one JSON object: `{"ok": true, "data": …}`, or
`{"ok": false, "code": "…", "message": "…"}` with exit status 1. Check `ok`.
Codes: `argument.invalid`, `daemon.offline`, `daemon.access_denied`, `daemon.unavailable`,
`session.not_found`, `session.invalid`, `action.unknown`,
`completion.failed`, `suggestion.removed`, `import.busy`, `import.failed`,
`person.not_found`, `person.invalid`, `person.exists`, `people.failed`,
`line.not_found`.

The commands talk to the running eco daemon. On `daemon.offline`, tell the
user; run `eco start --headless` only if they agree. On
`daemon.access_denied`, explain that this agent cannot access the socket and
ask for a less restricted environment. Do not start another daemon. On
`daemon.unavailable`, report the connection error without claiming eco stopped.
On `argument.invalid`, an argument held a control character such as a line
break and nothing was sent; pass ids and names on one line.
`eco status` checks whether the daemon is running. `eco stop` and `eco restart`
control it; use them only when the user asks.

## Read

```bash
eco sessions                 # newest first: id, title, kind, source, language, state,
eco sessions --kind meeting  #   started_at, duration_s, speech, suggestions, tags, cost
eco sessions --person <id>   # sessions linked to one person
eco sessions --tag "Acme"    # sessions with that tag, any case
eco sessions --search "prazo do contrato"  # sessions whose title, tags, lines, notes or answers hold it
eco show <id>             # {session, timeline, speakers}; session has path and bytes
eco translate <id> --lang en  # translate its lines and answers from now on (--off stops)
eco export <id> > a.vtt   # the transcript as WebVTT on stdout (not JSON)
```

A session's `cost` is in USD, as the providers reported it: `llm_usd`,
`transcription_usd` (Deepgram reports each request's cost a minute or so after
it closes; a model's optional `price_per_minute` only stands in where a provider
reports none), and `total_usd`; `llm_unknown` or `transcription_unknown` says a
part is left out (a provider reported no cost, or not yet, and no price stands
in, or an old log did not say).

Commands take a session's `id`. To find a session the user names by title, list
`eco sessions` and match it; when several match, ask which one. To find one by
what was said in it, use `--search`: it ignores case and accents and matches
the text as one phrase.

In a timeline, `{"type": "transcript", "who", "name", "text", "at"}` is a line
of speech — `who` is the speaker's label, `name` what they go by — and
`{"type": "suggestion", "action", "prompt", "text"}` an answer (`action` is
`chat` for a free question). `session.speakers` lists the names in the order they
first speak. Read the transcript yourself for
exact quotes and for anything the user wants done outside eco.

When a session translates (`session.translating` is its language code), lines
and answers carry a `translation` in that language. `eco translate` turns it
on, changes the language or stops it (`--off`), and returns
`{session, translating}`; a language eco does not translate into fails with
`translation.unknown`, the session's own with `translation.own`. What the
session already holds is translated in the background, and `eco show` brings
the translations once they arrive. Quote
the original `text`; the translation is a model's.

## Ask

```bash
eco ask <id> what did we decide about pricing?
eco action <id> <name>    # an action configured by the user, e.g. minutes
eco note <id> I migrated a WPF app to MVVM   # a fact later answers use, not a question
```

`ask` and `action` wait for the whole answer; `data.text` is Markdown. `note`
returns at once with `{session, id, text, at}`: the note stays in the session,
where the user sees it, and every later answer reads it. The answer is kept
in the session, where the user sees it. Actions are the user's `[[actions]]` in
`~/.config/eco/config.toml`; an unknown name fails with `action.unknown`. Each
session streams one answer at a time: a request made meanwhile on the same
session waits for it and sees its answer; a newer request replaces the one
waiting (`suggestion.removed`). Different sessions answer at once, so asking
about several sessions in parallel is fine.

An action may have a hook: a command of the user's that eco runs with the
session's id and title, for another system to read the session. It may run on
its own after each answer; otherwise send an answer to it when the user asks:

```bash
eco send <id> <answer>    # the answer's id, from `eco show`; waits until the hook exits
```

It returns `{session, id, sent}`. It fails with `hook.none` when the answer's
action has no hook, `answer.not_found` when there is no finished answer with
that id, and `hook.failed` with what the command wrote to stderr.

Answers use the user's global context and the context slots the session has on:
named groups of files (`[[contexts]]` in the config, e.g. a résumé for
interviews). A session has the slots of its kind on until someone chooses.

```bash
eco context <id>                           # {session, contexts (on), available}
eco context <id> --add entrevista          # turn a slot on (repeatable)
eco context <id> --remove projetos         # turn a slot off (repeatable)
```

Change the slots only when the user asks; an unknown name fails with
`context.unknown`.

## Organize and import

```bash
eco rename <id> --title "Acme kickoff" --kind meeting
eco import ~/recordings/talk.mp4 --kind other --title "Rust talk"
eco delete <id>  # permanently removes an interrupted or ended session and its voice links
eco tag list                     # [{tag, sessions}]: every tag and how many sessions carry it
eco tag add <id> "Acme"          # {session, tags}; several tags per session
eco tag remove <id> "Acme"
eco tag rename "Acme" "Acme Corp"  # in every session; into an existing tag joins the two
eco tag delete "Acme"            # off every session
```

Tags group sessions apart from kinds and people. They are one whatever their
case and keep the casing first written: `tag add <id> acme` gives `Acme` when
another session has it. An empty tag is `tag.invalid`; renaming or deleting a
tag no session has is `tag.not_found`.

`kind` must be one of the configured kinds. `import` takes a WebVTT transcript
(`.vtt` from Teams or Zoom: speakers and times kept, nothing transcribed) or
any audio or video ffmpeg reads, optionally `--language pt|en|…`, `--participant <name>` (who
the file is heard as; the user by default) and `--date YYYY-MM-DDTHH:MM[:SS]` (when
it was recorded, local time; by default the file's recording date, else when it
last changed — the session's `started_at`), and waits until it is
transcribed (`--no-wait` returns once it starts). One import runs at a time.

## Lines

A line of speech is named by who said it and when: the `who` (label) and `at`
of its `transcript` entry in `eco show`.

```bash
eco line <id> <who> <at> --text "…"   # its corrected text, everywhere the session is read
eco line <id> <who> <at> --remove     # it leaves the session, its context and its export
```

Prints `{session, who, at, text}` or `{session, who, at, removed}`. Correct or
remove a line only when the user asks.

## Speakers and people

Imported recordings, and the others' audio of live sessions once capture
stops, are split by voice: speakers are labelled `Speaker 1`, `Speaker 2`…
(one voice keeps the label it was heard as) and `eco show` lists them in
`speakers`: `{"label", "name", "person", "voice", "suggestions", "guess"}`.
`voice` says eco kept the speaker's voice (not WebVTT imports, the user's own
input, or speakers whose person was forgotten); `person` is the id of who they
are, if known; `suggestions` are people whose voice is close
(`{"person", "name", "score"}`). eco never names a speaker on its own: `guess`
is the person their voice is very close to, waiting for the user. Confirm it
with `--person`, or clear it with `--clear`; only do either when the user
says so.

```bash
eco speaker <id> "Speaker 2" --name "Ana"      # who they are (known name, or a new person)
eco speaker <id> "Speaker 2" --person <person> # a known person, by id
eco speaker <id> "Speaker 2" --clear           # no one: back to the label, or clear the guess
eco assign <id> line "Speaker 2" <at> --person <person> # just one line
eco assign <id> all --name "Ana"                # all spoken lines in the session
eco people                                     # [{id, name, voices, sessions}]
eco people add "Sadao Maia"                    # a new person, before any session names them
eco people rename <person> "Ana Paula"         # renamed in every session
eco people merge <into> <from>                 # the same person: voices and sessions join
eco people forget <person>                     # deletes them and their voices; an id no longer kept leaves the sessions naming it
eco people adopt                               # link named speakers in older live sessions
eco participant <id> --name "Ana"               # add a person to a session, without identifying a speaker
eco participant <id> --person <person>          # add a known person
eco participant <id> --remove <person>          # remove a person added to the session
```

`--name` makes the speaker the person of that name when eco knows one and
creates a person otherwise, including for WebVTT imports. `assign line` changes
only the identified line; `assign all` changes every speaker in the session.
`speaker` prints `{session, label, name}`; the `people`
commands print everyone after the change. Naming a speaker with a voice teaches eco that person's
voice for later sessions. Prefer a `suggestions` entry when the user confirms
it; never guess a name the user did not give.
`people add` refuses a name someone already has, in any case, with
`person.exists` (its `params` hold that person's `id` and `name`).
`people adopt` appends person links only for already named live speakers. It
does not change WebVTT or audio import names, and can be run again safely.

## Rules

- Sessions are the user's private conversations. Quote only what the task
  needs; never send their content anywhere the user did not ask for.
- `ask`, `action`, `rename`, `tag` (except `tag list`), `import`, `speaker`, `assign`, `delete`, `participant` and `people` change the
  user's sessions and people: run them when the user asked for that, not to
  explore. `delete` and `people forget` cannot be undone.
- Live recording is the user's: the CLI cannot start, pause or end it. Several
  sessions may be `recording` at once (a call and a class, say); each holds only
  what was said while it recorded.
