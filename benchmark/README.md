# Answer benchmark

Measures how well each model does eco's skills — answering in an interview,
summarizing a meeting, following up a class, turning an idea session into
actions, writing a client e-mail — and eco's translation, on synthetic sessions, and keeps every run
for comparison. Nothing here comes from a real person: the user is the
fictional Sadao Maia, a demon lord who wants to be promoted, and his cast
(`synthetic-data/persona.md`).

## Run it

```bash
cp benchmark/.env.example benchmark/.env   # or export the variables yourself
mise run bench                              # every model in bench.toml
mise run bench -- --models gpt-6-luna,grok-4.20 --label two  # only these
```

Then open `benchmark/bench.html` in a browser. It reads `runs.js`, so it works
from disk, offline; `runs.js` is generated and kept out of git, so after a
clone `mise run bench:index` writes it. Pick a run in the first dropdown; pick another in the
second to see each number's change against it.

`bench.toml` sets everything: the models under test, the graders, the rules
and the skills (prompt and format of each), how many runs and which
scenarios. Keys come from
the environment or `benchmark/.env`, by the variable each entry names
(`OPENROUTER_API_KEY` by default). The skill runs in a real eco daemon,
built from this checkout and started headless with its own runtime, data and
config directories, so your sessions and settings are never touched. It needs
eco's VAD model (`mise run setup`).

Every answer of a run — each scenario, model and run on its own copy of the
session — is asked for at once, and eco streams `concurrency` of them side by
side (8 by default), as it does for several sessions; translations run three
at a time. A run takes about as long as its slowest answers, not their sum.
Latency is measured from when eco sends the request, so waiting for a turn
does not count; `concurrency = 1` asks one at a time, as a single live
session would, for when the provider itself slows under load.

Grading runs the same way: every grader call that does not wait for another —
each answer's points, claims and score, each validation, each pair in each
order — goes at once, `graders` of them side by side (32 by default); a
rate-limited call backs off and is asked again.

After editing an answer key or the graders, `mise run bench:grade
benchmark/bench-<…>` grades a run's answers again without asking the models
again; `mise run bench:index` rebuilds `runs.js` from the run folders without
calling any model.

## What a run writes

`bench-<date>-<label>/` holds `run.json` (when, which models, scenarios and
runs), `config.toml` (the bench.toml it ran with),
`answers.jsonl` (each answer with its latency, tokens, cost and grades),
`judgments.jsonl` (every grader reply) and `summary.json`. Grader replies are
cached in `.cache/`, so grading the same answer again costs nothing.

## How answers are graded

Each scenario (`synthetic-data/scenarios/*.toml`) is a session and an answer
key written by hand: a reference answer, required points (`must`), forbidden
points (`must_not`) and a deliberately bad answer. The key decides what a good
answer is; graders only read it. Graders never see which model wrote an
answer: they get the text alone, and pairs come as A and B.

1. **Gates, no model.** The answer is in the expected language (a stopword
   count), within the scenario's word range, has every section the scenario
   requires, and has no number of two digits or more, or percentage, that the
   sources — the conversation, the notes and, in interviews, the persona — do
   not contain; a translation must also keep every number of its lines.
2. **The key, point by point.** A point with a `regex` that matches is present
   without asking anyone. The rest go to the **decider**
   (`openai/gpt-6-luna-decisions`, OpenRouter's `/api/alpha/decisions`), which
   gives the probability that the answer states each point. At 0.7 or more the
   point is present; at 0.3 or less, absent. In between, the **examiner**
   decides it, quoting the answer (a quote not in the answer counts as
   absent), and the **adversary** tries to prove it wrong, quoting too. A
   point they dispute counts as absent and is marked. *Coverage* is the share
   of required points present; an answer *passes* when every required point is
   there, no forbidden one is, and every gate holds.
3. **Facts.** The examiner lists the checkable facts the answer states —
   names, numbers, dates, results, who took what, decisions — and labels each
   supported, mixed (two cases or people joined), contradicted or unsupported,
   quoting the sources for a supported one (verified like any quote). The
   adversary challenges every label and must quote the sources to win. A
   disputed fact counts neither way. *Fidelity* is the share of the others
   that are supported.
4. **Score.** The decider rates each answer 1 to 5 on a rubric — ready to use
   as is down to wrong — with the task, the key, the reference and the
   sources; the score is the expected value over its levels.
5. **Pairs.** For each scenario and run, the decider sees two answers without
   the models' names and gives the probability that each is better. It leans
   towards whichever answer comes first, so every pair is judged in both
   orders and the two chances are averaged; an answer wins at 0.6 or more,
   otherwise the pair is a tie. *Wins* count a tie as half.

The graders are checked every run: on each scenario the whole point check must
find every point of the reference answer and fail the bad answer on the
points it names (`bad_fails`); below 90% the report marks them not trusted.
The report also says how many points each step decided, how often the
adversary agreed, and in how many pairs the pick followed the order.

Pass, coverage, fidelity, score and wins carry 95% intervals from resampling
the scenarios; when two models' intervals overlap, the difference may be
noise. There is no single score: latency (p50 and p95 of the first word and
the whole answer), cost per answer and the share of the prompt served from
cache sit beside quality, because the right trade depends on the use.

## Add a scenario

Copy a file in `synthetic-data/scenarios/`. A scenario sets:

- `skill` (a `[skills.<name>]` of bench.toml; `responder` by default) and
  `kind`, the session kind (`interview` by default; only interviews get the
  persona as context);
- `language` of the session, and `answer_language` when the answer is
  written in another one (a class in English followed up in Portuguese);
- `transcript`: `[who, text]` lines, where `who` is `me`, `them` (named by
  `interviewer`) or any label — name labels in `names`, or leave them as
  `Speaker 1` to test that nobody gets a name; `notes` the user typed;
- `task`, what the skill was asked, for the report and the graders;
- `min_words`, `max_words` and `sections` (regexes the answer must match);
- for translation, `skill = "translation"` and `translate_to`: each model
  translates the sessions of its own kind (`translate-<model>`), the bench
  turns translation on for a fresh copy of the session, and the answer is
  every line translated, in order; its
  first-word time is the median time per line, and `keep_numbers` (on by
  default) fails it for a number it dropped;
- `reference`, `bad`, `bad_fails`, and the `[[must]]` and `[[must_not]]`
  points.

Keep facts to the sources, write the reference answer you would want, list
the points as short checkable statements, and add a `regex` only when a match
can only mean the point is there. Name in `bad_fails` the points your bad
answer gets wrong; the graders' check uses them. Put the trap a scenario tests —
a decision reversed, a task only suggested, an idea dropped — in a
`must_not`.
