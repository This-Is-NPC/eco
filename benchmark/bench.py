#!/usr/bin/env python3
"""eco's answer benchmark.

Runs eco's skills in an isolated eco daemon on synthetic sessions, once per
model and run, then grades every answer: gates that need no model (language,
length, sections, invented numbers); a decider that gives the probability of
each point of the scenario's answer key, a rubric score and blind pairwise
picks in both orders; and, for what has no probability, an examiner whose
quoted verdicts an adversary tries to disprove. Writes bench-<date>-<label>/
and rebuilds runs.js, which bench.html reads.

    bench.py run [--label NAME] [--models A,B]   run the benchmark bench.toml describes,
                                                 or only the models named
    bench.py grade FOLDER         grade a run's answers again, with today's keys and judges
    bench.py index                rebuild runs.js from the run folders
"""

import argparse
import hashlib
import json
import os
import random
import re
import secrets
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import unicodedata
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DATA = HERE / "synthetic-data"
CACHE = HERE / ".cache" / "judge.jsonl"

# The skill a scenario names to test eco's translation rather than an action.
TRANSLATION = "translation"

# The labels eco's session records give the two sides of a conversation.
USER, OTHERS = "Eu", "Eles"
ANSWER_TIMEOUT = 240

STOPWORDS = {
    "pt": set("de que não para com uma os no na em é eu meu minha por mais como isso foi são se ao do da dos das".split()),
    "en": set("the and to of in is that it for with my was on as not be are this an at by have".split()),
}


# Inputs.

def load_env(path):
    """Variables from a KEY=VALUE file, without replacing those already set."""
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip().strip('"').strip("'"))


def load_scenarios(selected):
    scenarios = []
    for path in sorted((DATA / "scenarios").glob("*.toml")):
        if selected and path.stem not in selected:
            continue
        scenario = defaults(tomllib.loads(path.read_text()))
        scenario["id"] = path.stem
        scenarios.append(scenario)
    if not scenarios:
        sys.exit("no scenarios to run")
    return scenarios


def defaults(scenario):
    """A scenario with what it may leave out: an interview answered by `responder`.
    A translation scenario's answer is every line translated, in order."""
    scenario.setdefault("skill", "responder")
    scenario.setdefault("kind", "meeting" if scenario["skill"] == TRANSLATION else "interview")
    if scenario["skill"] == TRANSLATION:
        scenario.setdefault("answer_language", scenario["translate_to"])
        scenario.setdefault("keep_numbers", True)
    scenario.setdefault("notes", [])
    scenario.setdefault("must_not", [])
    scenario.setdefault("names", {})
    scenario.setdefault("sections", [])
    scenario.setdefault("min_words", 0)
    if "interviewer" in scenario:
        scenario["names"].setdefault(OTHERS, scenario["interviewer"])
    return scenario


def label(who):
    """The label eco's records give a transcript line's speaker."""
    return {"me": USER, "them": OTHERS}.get(who, who)


def uses_persona(scenario):
    return scenario["kind"] == "interview"


def sources(scenario, persona):
    """Everything an answer may draw facts from: the conversation, the notes and,
    in an interview, the persona."""
    def speaker(who):
        return "Sadao (me)" if who == "me" else scenario["names"].get(label(who), label(who))
    lines = [f"{speaker(who)}: {text}" for who, text in scenario["transcript"]]
    notes = [f"My note: {note}" for note in scenario["notes"]]
    return "\n".join(([persona] if uses_persona(scenario) else []) + ["## Conversation", *lines, *notes])


def question(scenario):
    """What the skill was asked: the scenario's task, or the interviewer's last line."""
    if "task" in scenario:
        return scenario["task"]
    return next(text for who, text in reversed(scenario["transcript"]) if who == "them")


# The eco daemon under test.

def toml_string(value):
    return json.dumps(value, ensure_ascii=False)


def eco_config(config, persona_path, scenarios):
    """An eco config with every model under test as its own copy of each skill the
    scenarios use; the persona is context only for interviews."""
    models = config["models"]
    translating = any(s["skill"] == TRANSLATION for s in scenarios)
    kinds = sorted({s["kind"] for s in scenarios} | ({translator_kind(m) for m in models} if translating else set()))
    skills = sorted({s["skill"] for s in scenarios} - {TRANSLATION})
    missing = [name for name in skills if name not in config["skills"]]
    if missing:
        sys.exit(f"bench.toml has no skill {', '.join(missing)}")
    out = [
        f"kinds = [{', '.join(toml_string(k) for k in kinds)}]",
        f"rules = {toml_string(config['rules'])}",
        "",
        "[stt]",
        'model = "none"',
        'language = "pt"',
        'languages = ["auto", "pt", "en"]',
        "",
        "[llm]",
        f"model = {toml_string(models[0]['name'])}",
        f"concurrency = {int(config.get('concurrency', 8))}",
        "",
        "[[contexts]]",
        'name = "persona"',
        f"files = [{toml_string(str(persona_path))}]",
        'kinds = ["interview"]',
        "",
        "[[participants]]",
        f"name = {toml_string(USER)}",
        "user = true",
        "devices = []",
        "",
        "[[participants]]",
        f"name = {toml_string(OTHERS)}",
        "devices = []",
        "",
        "# Never reached: sessions are stored, not recorded.",
        "[[models]]",
        'name = "none"',
        'type = "transcription"',
        'base_url = "http://127.0.0.1:9/v1"',
        'model = "whisper-1"',
    ]
    for model in models:
        out += ["", "[[models]]", f"name = {toml_string(model['name'])}", 'type = "chat"',
                f"base_url = {toml_string(model['base_url'])}", f"model = {toml_string(model['model'])}",
                f"api_key_env = {toml_string(model['api_key_env'])}"]
        if "reasoning" in model:
            out.append(f"reasoning = {toml_string(model['reasoning'])}")
        # Each model translates the sessions of its own kind.
        if translating:
            out.append(f"translates = [{toml_string(translator_kind(model))}]")
    for name in skills:
        skill = config["skills"][name]
        for model in models:
            out += ["", "[[actions]]", f"name = {toml_string(action(name, model))}",
                    f"prompt = {toml_string(skill['prompt'])}", f"format = {toml_string(skill['format'])}",
                    f"model = {toml_string(model['name'])}"]
    return "\n".join(out) + "\n"


def action(skill, model):
    return f"bench-{skill}-{model['name']}"


def translator_kind(model):
    """The session kind `model` translates, so each copy of a translation
    scenario names its translator by its kind."""
    return f"translate-{model['name']}"


def session_records(scenario, kind, started):
    """A stored, ended session of `kind` holding the scenario's conversation and notes."""
    sid = secrets.token_hex(6)
    records = [
        {"type": "session", "id": sid, "title": scenario["title"], "kind": kind, "source": "live",
         "language": scenario["language"], "started_at": started},
        {"type": "state", "state": "recording", "at": started},
    ]
    records += [{"type": "speaker", "label": who, "name": name, "at": started + 1}
                for who, name in scenario["names"].items()]
    at = started + 5
    for who, text in scenario["transcript"]:
        records.append({"type": "speech", "who": label(who), "text": text, "at": at})
        at += 12
    # The notes were typed before the last line, as during a live session.
    for note in scenario["notes"]:
        records.insert(-1, {"type": "note", "id": secrets.token_hex(4), "text": note, "at": at - 13})
    records.append({"type": "state", "state": "ended", "at": at})
    return sid, records


class Daemon:
    """`eco daemon --headless` in its own runtime, data and config directories."""

    def __init__(self, eco, config_text, jobs):
        """Writes one stored session per job, so every answer starts from the same
        conversation and none waits for another in its session."""
        self.dir = Path(tempfile.mkdtemp(prefix="eco-bench-"))
        runtime, data, conf = self.dir / "run", self.dir / "data" / "eco", self.dir / "config" / "eco"
        for path in (runtime, data / "sessions", conf):
            path.mkdir(parents=True)
        runtime.chmod(0o700)
        models = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "eco" / "models"
        if not (models / "silero_vad.onnx").exists():
            sys.exit(f"no VAD model in {models}; run `mise run setup` first")
        (data / "models").symlink_to(models)
        (conf / "config.toml").write_text(config_text)
        started = time.time() - 3600
        for k, job in enumerate(jobs):
            scenario = job["scenario"]
            kind = translator_kind(job["model"]) if scenario["skill"] == TRANSLATION else scenario["kind"]
            job["sid"], records = session_records(scenario, kind, started + k * 120)
            stamp = time.strftime("%Y-%m-%d-%H%M%S", time.localtime(started + k * 120))
            path = data / "sessions" / f"{stamp}-{scenario['id']}-{k}.jsonl"
            path.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records))
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), XDG_DATA_HOME=str(self.dir / "data"),
                   XDG_CONFIG_HOME=str(self.dir / "config"))
        self.log = open(self.dir / "daemon.log", "w")
        self.process = subprocess.Popen([str(eco), "daemon", "--headless"], env=env, stdout=self.log,
                                        stderr=subprocess.STDOUT)
        socket_path = runtime / "eco.sock"
        for _ in range(100):
            if socket_path.exists():
                break
            if self.process.poll() is not None:
                sys.exit(f"eco stopped: {(self.dir / 'daemon.log').read_text()[-500:]}")
            time.sleep(0.1)
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.connect(str(socket_path))
        self.reader = self.socket.makefile(encoding="utf-8")

    def send(self, command):
        self.socket.sendall((command + "\n").encode())

    def event(self, deadline):
        self.socket.settimeout(max(0.1, deadline - time.time()))
        return json.loads(self.reader.readline())

    def collect(self, jobs):
        """Ask for every job's answer at once — eco bounds how many stream — and
        fill each job's `got` as its answer arrives: a skill's reply, or a
        translation scenario's lines translated, in order."""
        by_session = {job["sid"]: job for job in jobs}
        by_answer = {}
        for job in jobs:
            job.update(started=time.time(), text=[], lines={}, ms=[], cost=[])
            sid = job["sid"]
            if job["scenario"]["skill"] == TRANSLATION:
                language = job["scenario"]["translate_to"]
                self.send("session.translation " + json.dumps({"id": sid, "language": language}))
            else:
                name = action(job["scenario"]["skill"], job["model"])
                self.send("session.action " + json.dumps({"id": sid, "name": name}))
        waiting = len(jobs)
        deadline = time.time() + ANSWER_TIMEOUT * len(jobs)

        def done(job, got):
            nonlocal waiting
            if "got" not in job:
                job["got"] = got
                waiting -= 1
                print(f"{len(jobs) - waiting}/{len(jobs)} · {job['scenario']['id']} · {job['model']['name']}"
                      f"{'' if got['ok'] else ' · ' + got['error']}", flush=True)

        while waiting:
            try:
                event = self.event(deadline)
            except (TimeoutError, socket.timeout):
                break
            kind, params = event.get("type"), event.get("params") or {}
            job = by_answer.get(event.get("id")) or by_session.get(event.get("session") or params.get("session"))
            if kind == "suggestion_start" and job:
                by_answer[event["id"]] = job
            elif kind == "suggestion_delta" and job:
                job["text"].append(event["text"])
            elif kind == "suggestion_end" and job:
                done(job, {"ok": True, "text": "".join(job["text"]).strip(), "ttft_ms": event["ttft_ms"],
                           "total_ms": event["total_ms"], "prompt_tokens": event.get("prompt_tokens"),
                           "cached_tokens": event.get("cached_tokens"), "cost_usd": event.get("cost_usd")})
            elif kind == "translated" and job:
                job["lines"][event["at"]] = event["text"]
                job["ms"].append(event["ms"])
                job["cost"].append(event.get("cost_usd"))
                if len(job["lines"]) == len(job["scenario"]["transcript"]):
                    known = [c for c in job["cost"] if c is not None]
                    done(job, {"ok": True, "text": "\n".join(job["lines"][at] for at in sorted(job["lines"])),
                               "ttft_ms": percentile(job["ms"], 0.5),
                               "total_ms": round((time.time() - job["started"]) * 1000),
                               "cost_usd": sum(known) if known else None})
            elif kind == "error":
                job = by_answer.get(params.get("id")) or by_session.get(params.get("session") or params.get("id"))
                if job:
                    done(job, {"ok": False, "error": event.get("message", event.get("code"))})
        for job in jobs:
            if "got" not in job:
                job["got"] = {"ok": False, "error": f"no answer in {ANSWER_TIMEOUT * len(jobs)} s"}

    def stop(self):
        try:
            self.send("stop")
            self.process.wait(timeout=10)
        except Exception:
            self.process.kill()
        self.log.close()


# Gates: no model involved.

def words(text):
    return re.findall(r"\w+", re.sub(r"[*_#`]", " ", text))


def language_of(text):
    # Japanese has no spaces to count words by: its scripts tell it.
    letters = [c for c in text if c.isalpha()]
    japanese = sum("\u3040" <= c <= "\u30ff" or "\u4e00" <= c <= "\u9fff" for c in letters)
    if letters and japanese / len(letters) > 0.3:
        return "ja"
    tokens = [w.lower() for w in words(text)]
    counts = {code: sum(t in stop for t in tokens) for code, stop in STOPWORDS.items()}
    best = max(counts, key=counts.get)
    return best if counts[best] else "unknown"


def numbers(text):
    """Numbers of two digits or more, or percentages: the ones worth checking."""
    found = set()
    for match in re.finditer(r"\d+(?:[.,]\d+)?(?: ?%)?", text):
        token = match.group(0).replace(" ", "")
        digits = token.rstrip("%")
        if len(re.sub(r"\D", "", digits)) >= 2 or token.endswith("%"):
            found.add(digits.replace(",", "."))
    return found


# Month names a translation may write a dd/mm date's month as.
MONTHS = {f"{n:02d}": names for n, names in enumerate([
    ("january", "janeiro"), ("february", "fevereiro"), ("march", "março"), ("april", "abril"),
    ("may", "maio"), ("june", "junho"), ("july", "julho"), ("august", "agosto"),
    ("september", "setembro"), ("october", "outubro"), ("november", "novembro"), ("december", "dezembro"),
], start=1)}


def kept_numbers(said, answer):
    """The numbers of `said` that `answer` does not keep; a dd/mm date's month also
    counts as kept when the answer names it."""
    missing = numbers(said) - numbers(answer)
    named = {month for month, names in MONTHS.items() if any(name in answer.lower() for name in names)}
    months = {m.group(2) for m in re.finditer(r"\b(\d{1,2})/(\d{2})\b", said)}
    return sorted(missing - (months & named))


def gates(answer, scenario, source):
    language = language_of(answer)
    invented = sorted(numbers(answer) - numbers(source))
    count = len(words(answer))
    missing = [section for section in scenario["sections"] if not re.search(section, answer, re.IGNORECASE)]
    # A translation keeps every number of the lines it translates.
    said = "\n".join(text for _, text in scenario["transcript"])
    dropped = kept_numbers(said, answer) if scenario.get("keep_numbers") else []
    return {
        "language": language,
        "language_ok": language == scenario.get("answer_language", scenario["language"]),
        "words": count,
        "length_ok": scenario["min_words"] <= count <= scenario["max_words"],
        "invented_numbers": invented,
        "missing_sections": missing,
        "dropped_numbers": dropped,
    }


# Judges.

def plain(text):
    """Text compared loosely: no accents, case, markup or extra spaces."""
    text = unicodedata.normalize("NFKD", text)
    text = "".join(c for c in text if not unicodedata.combining(c)).lower()
    text = re.sub(r"[*_`#>\"“”‘’'«»]", " ", text)
    return re.sub(r"\s+", " ", text).strip()


def quoted(quote, text):
    """Whether `quote` appears in `text`, allowing for an ellipsis between parts."""
    parts = [p for p in (plain(part) for part in re.split(r"\.\.\.|…", quote)) if len(p) >= 3]
    haystack = plain(text)
    return bool(parts) and all(part in haystack for part in parts)


class Judges:
    """Calls to the decider (typed answers with probabilities) and to the examiner
    and adversary (chat with JSON replies), cached across runs so grading again
    costs nothing; a failed call is asked again next time, never kept."""

    def __init__(self):
        self.lock = threading.Lock()
        self.calls = []
        self.cache = {}
        if CACHE.exists():
            for line in CACHE.read_text().splitlines():
                entry = json.loads(line)
                self.cache[entry["key"]] = entry["reply"]

    def cached(self, judge, payload, fetch, purpose):
        key = hashlib.sha256(json.dumps([judge["model"], payload]).encode()).hexdigest()
        with self.lock:
            reply = self.cache.get(key)
        if reply is None:
            reply = fetch()
            with self.lock:
                if "error" not in reply["json"]:
                    self.cache[key] = reply
                    CACHE.parent.mkdir(exist_ok=True)
                    with CACHE.open("a") as file:
                        file.write(json.dumps({"key": key, "reply": reply}, ensure_ascii=False) + "\n")
        with self.lock:
            self.calls.append({"judge": judge["name"], "purpose": purpose, "reply": reply["json"],
                               "cost": reply.get("cost")})
        return reply["json"]

    def ask(self, judge, system, user, purpose):
        """A chat judge's JSON reply."""
        body = {"model": judge["model"], "temperature": 0, "response_format": {"type": "json_object"},
                "usage": {"include": True},
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]}

        def parse(reply):
            content = reply["choices"][0]["message"]["content"]
            if not content:
                raise ValueError("the judge sent no content")
            content = re.sub(r"^```(?:json)?|```$", "", content.strip()).strip()
            return json.loads(content), (reply.get("usage") or {}).get("cost")

        return self.cached(judge, [system, user], lambda: post(judge, "/chat/completions", body, parse), purpose)

    def decide(self, decider, state, questions, purpose):
        """The decider's typed answers to `questions` about `state`, by name."""
        body = {"model": decider["model"], "state": state, "questions": questions}

        def parse(reply):
            return reply["answers"], (reply.get("usage") or {}).get("cost")

        return self.cached(decider, [state, questions], lambda: post(decider, "/decisions", body, parse), purpose)


def post(judge, path, body, parse):
    """POST `body` to the judge's endpoint, retried; `parse` turns the reply into
    (json, cost). A call that keeps failing comes back as {"error": …}."""
    key = os.environ[judge["api_key_env"]]
    request = urllib.request.Request(judge["base_url"].rstrip("/") + path, data=json.dumps(body).encode(),
                                     headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    failure = None
    # Many calls run at once: a rate limit backs off longer each time.
    for attempt in range(5):
        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                found, cost = parse(json.load(response))
            return {"json": found, "cost": cost}
        except (urllib.error.URLError, ValueError, KeyError, TimeoutError) as error:
            failure = error
            time.sleep(2 ** (attempt + 1))
    return {"json": {"error": str(failure)}, "cost": 0}


# Probabilities the decider must reach for a point to count as present or absent;
# in between, the examiner and the adversary decide it.
SURE_YES, SURE_NO = 0.7, 0.3
# How sure, averaged over both orders, the decider must be that one answer is
# better for it to win the pair; otherwise the pair is a tie.
PAIR_MARGIN = 0.6

RUBRIC = [
    "1: does not do what was asked, or is mostly wrong",
    "2: does part of it, with errors that must be fixed before use",
    "3: does it, but needs edits before use",
    "4: does it well; only minor edits",
    "5: ready to use as is: complete, faithful to the sources and clear",
]

POINTS = """You check an answer against a list of points. For each point, decide only
whether the answer states it, word for word or in a clear paraphrase. The
SOURCES are the session the answer was written from: read them only to
understand points that refer to them, such as a task given to the wrong person
or a correction nobody made. Do not judge quality, tone or length. For a point
that is present, quote the passage of the answer that states it, copied
exactly. Reply with JSON:
{"points": [{"id": "<point id>", "present": true or false, "quote": "<exact passage, or empty>"}]}"""

CLAIMS = """You list the checkable facts an answer states about the SOURCES: names,
companies, projects, numbers, dates, results, who did or took what, what was
decided, promised or corrected, and, when the sources include a résumé, the
candidate's own past and skills. Leave out everything that is not such a fact:
reasons and motives, opinions, plans, advice, general knowledge, the answer's
own suggestions, and wording a reader would take for granted (a pay quoted as
a monthly figure, a notice period implied by leaving a job). Label each fact
against the SOURCES:
- "supported": the sources state it, for the same person and the same case
- "mixed": it joins facts of different cases or people, e.g. one company's result told as another's, or a task given to the wrong person
- "contradicted": the sources say otherwise, including a decision told as final that was later changed
- "unsupported": the sources do not say it
For a supported claim, quote the SOURCES passage that states it, copied exactly.
Reply with JSON: {"claims": [{"claim": "...", "label": "...", "source": "<exact quote, or empty>"}]}"""

CHALLENGE_POINTS = """Another judge decided whether an ANSWER states each of a list of points; its
verdicts are below. Your job is to find its mistakes. For each verdict, read the
ANSWER again (and the SOURCES, when the point refers to them) and decide
whether the verdict is right. Do not defer to the other judge: disagree whenever
the text does not back the verdict. When you disagree, quote the passage that
proves it wrong, copied exactly from the ANSWER or the SOURCES; a disagreement
you cannot quote does not count. Reply with JSON:
{"verdicts": [{"id": "<verdict id>", "agree": true or false, "quote": "<exact passage, or empty>", "reason": "<one sentence>"}]}"""

CHALLENGE_CLAIMS = """Another judge listed facts an ANSWER states and labelled each against the
SOURCES: supported (the sources state it, same person and case), mixed (facts
of different cases or people joined), contradicted (the sources say otherwise)
or unsupported (the sources do not say it). Your job is to find wrong labels.
A fact being written in the ANSWER says nothing about its label: only the
SOURCES decide it. Do not defer to the other judge. When you disagree, quote
the SOURCES passage that proves the label wrong, copied exactly; a
disagreement that does not quote the SOURCES does not count. Reply with JSON:
{"verdicts": [{"id": "<verdict id>", "agree": true or false, "quote": "<exact passage, or empty>", "reason": "<one sentence>"}]}"""


def point_list(scenario):
    return [dict(p, kind="must") for p in scenario["must"]] + [dict(p, kind="must_not") for p in scenario["must_not"]]


def key_text(scenario):
    must = "\n".join(f"- must: {p['text']}" for p in scenario["must"])
    must_not = "\n".join(f"- must not: {p['text']}" for p in scenario["must_not"])
    return f"{must}\n{must_not}"


def challenge(judges, adversary, prompt, source, answer, verdicts, evidence, purpose):
    """The adversary's take on each verdict ({id: text}): agree, or a reason not to
    that quotes `evidence` — the texts its quote must come from."""
    listing = "\n".join(f"- {vid}: {text}" for vid, text in verdicts.items())
    reply = judges.ask(adversary, prompt, f"SOURCES:\n{source}\n\nANSWER:\n{answer}\n\nVERDICTS:\n{listing}", purpose)
    found = {v.get("id"): v for v in reply.get("verdicts", []) if isinstance(v, dict)}
    out = {}
    for vid in verdicts:
        take = found.get(vid, {})
        quote = take.get("quote") or ""
        # A challenge only counts when its quote is really in the texts it may cite.
        upheld = take.get("agree") is False and any(quoted(quote, text) for text in evidence)
        out[vid] = {"agree": not upheld, "quote": quote, "reason": take.get("reason", "")}
    return out


def check_points(judges, config, scenario, source, answer, purpose):
    """Each point of the scenario's key, present or not, and how that was decided: a
    matching regex; the decider, when it is sure; otherwise the examiner, with a
    quote from the answer, checked by the adversary. A point they dispute is
    absent and marked."""
    decider, (examiner, adversary) = config["decider"], config["judges"]
    results, asked = {}, []
    for point in point_list(scenario):
        regex = point.get("regex")
        match = re.search(regex, answer, re.IGNORECASE) if regex else None
        if match:
            results[point["id"]] = {"present": True, "by": "regex", "quote": match.group(0)}
        else:
            asked.append(point)
    if not asked:
        return results
    questions = {f"p{i}": {"type": "noul", "instructions":
                           f"Does the ANSWER state this, word for word or in a clear paraphrase: {p['text']}? "
                           "Read the SOURCES only to understand the point."}
                 for i, p in enumerate(asked)}
    decided = judges.decide(decider, f"SOURCES:\n{source}\n\nANSWER:\n{answer}", questions, f"decide:{purpose}")
    unsure = []
    for i, point in enumerate(asked):
        probability = ((decided.get(f"p{i}") or {}) if "error" not in decided else {}).get("noul")
        if probability is not None and probability >= SURE_YES:
            results[point["id"]] = {"present": True, "by": "decider", "probability": probability}
        elif probability is not None and probability <= SURE_NO:
            results[point["id"]] = {"present": False, "by": "decider", "probability": probability}
        else:
            unsure.append((point, probability))
    if not unsure:
        return results
    listing = "\n".join(f"- {p['id']}: {p['text']}" for p, _ in unsure)
    reply = judges.ask(examiner, POINTS, f"SOURCES:\n{source}\n\nANSWER:\n{answer}\n\nPOINTS:\n{listing}", f"examine:{purpose}")
    verdicts = {v.get("id"): v for v in reply.get("points", []) if isinstance(v, dict)}
    examined = {}
    for point, probability in unsure:
        verdict = verdicts.get(point["id"], {})
        quote = verdict.get("quote") or ""
        # A presence the examiner cannot quote from the answer does not count.
        present = bool(verdict.get("present")) and quoted(quote, answer)
        examined[point["id"]] = {"present": present, "probability": probability, "quote": quote}
    takes = challenge(judges, adversary, CHALLENGE_POINTS, source, answer,
                      {pid: f"{'present' if v['present'] else 'absent'}"
                            f"{' — quoted: ' + v['quote'] if v['present'] else ''} — point: {point['text']}"
                       for (point, _), (pid, v) in zip(unsure, examined.items())},
                      (answer, source), f"challenge-points:{purpose}")
    for pid, verdict in examined.items():
        take = takes[pid]
        if take["agree"]:
            results[pid] = dict(verdict, by="examined")
        else:
            results[pid] = dict(verdict, present=False, by="disputed", challenge=take)
    return results


def check_claims(judges, config, answer, source, purpose):
    """The answer's factual claims, labelled by the examiner with a quoted source and
    checked by the adversary; a claim they dispute is marked so, and fidelity
    leaves it out."""
    examiner, adversary = config["judges"]
    reply = judges.ask(examiner, CLAIMS, f"SOURCES:\n{source}\n\nANSWER:\n{answer}", f"claims:{purpose}")
    claims = []
    for claim in reply.get("claims", []):
        if not isinstance(claim, dict):
            continue
        label = claim.get("label", "unsupported")
        # A support the examiner cannot quote from the sources does not count.
        if label == "supported" and not quoted(claim.get("source") or "", source):
            label = "unverified"
        claims.append({"claim": claim.get("claim", ""), "label": label, "source": claim.get("source", "")})
    if claims:
        takes = challenge(judges, adversary, CHALLENGE_CLAIMS, source, answer,
                          {f"c{i}": f"{c['label']} — claim: {c['claim']}" for i, c in enumerate(claims)},
                          (source,), f"challenge-claims:{purpose}")
        for i, claim in enumerate(claims):
            if not takes[f"c{i}"]["agree"]:
                claim["challenge"] = takes[f"c{i}"]
                claim["label"] = "disputed"
    return claims


def rate(judges, decider, scenario, source, answer, purpose):
    """The decider's rubric score, 1 to 5, as an expected value over its levels."""
    state = (f"SOURCES:\n{source}\n\nTASK:\n{question(scenario)}\n\nKEY:\n{key_text(scenario)}\n\n"
             f"REFERENCE (one good answer):\n{scenario['reference'].strip()}\n\nANSWER:\n{answer}")
    questions = {"score": {"type": "score", "criteria": RUBRIC, "instructions":
                           "How well does the ANSWER do the TASK? Judge it by the KEY first: what it must and "
                           "must not say, and how faithful it is to the SOURCES; then by how clear it is."}}
    reply = judges.decide(decider, state, questions, f"score:{purpose}")
    score = (reply.get("score") or {}).get("score") if "error" not in reply else None
    return None if score is None else score + 1


def compare(judges, decider, scenario, source, first, second, purpose):
    state = (f"SOURCES:\n{source}\n\nTASK:\n{question(scenario)}\n\nKEY:\n{key_text(scenario)}\n\n"
             f"REFERENCE (one good answer):\n{scenario['reference'].strip()}\n\n"
             f"ANSWER A:\n{first}\n\nANSWER B:\n{second}")
    questions = {"pick": {"type": "choice", "instructions":
                          "Which answer better meets the KEY: covers the required points, avoids the forbidden "
                          "ones and stays faithful to the SOURCES? Length and style count only after that.",
                          "criteria": {"A": "answer A is better", "B": "answer B is better",
                                       "tie": "neither is better"}}}
    reply = judges.decide(decider, state, questions, purpose)
    odds = (reply.get("pick") or {}).get("probabilities") if "error" not in reply else None
    if not odds:
        return None
    # The chance that answer A is better, a tie counting half.
    return odds.get("A", 0) + 0.5 * odds.get("tie", 0)


# Grading.

def passed(points, scenario):
    must = [p["id"] for p in scenario["must"]]
    must_not = [p["id"] for p in scenario["must_not"]]
    return all(points[i]["present"] for i in must), [i for i in must_not if points[i]["present"]]


def validate(pool, judges, config, scenarios, source):
    """Start the whole point check on each scenario's reference (it must find every
    required point and no forbidden one) and bad answer (it must fail the points
    the scenario names); returns what waits for the checks and tallies them."""
    started = []
    for scenario in scenarios:
        for which, text in (("reference", scenario["reference"]), ("bad", scenario["bad"])):
            started.append((scenario, which, pool.submit(check_points, judges, config, scenario,
                                                         source[scenario["id"]], text,
                                                         f"validate:{scenario['id']}:{which}")))

    def tally():
        checks = []
        for scenario, which, running in started:
            points = running.result()
            for pid, kind in ((p["id"], p["kind"]) for p in point_list(scenario)):
                if which == "reference":
                    expected = kind == "must"
                elif pid in scenario["bad_fails"]:
                    expected = kind == "must_not"
                else:
                    continue
                checks.append({"scenario": scenario["id"], "answer": which, "point": pid,
                               "expected": expected, "got": points[pid]["present"], "by": points[pid]["by"]})
        correct = sum(c["expected"] == c["got"] for c in checks)
        return {"correct": correct, "total": len(checks), "accuracy": correct / len(checks) if checks else 1.0,
                "misses": [c for c in checks if c["expected"] != c["got"]]}

    return tally


def bootstrap(per_scenario, rounds=2000, seed=7):
    """A 95% interval for the mean over scenarios, resampling the scenarios."""
    values = list(per_scenario)
    if not values:
        return [None, None]
    rng = random.Random(seed)
    means = sorted(statistics.fmean(rng.choices(values, k=len(values))) for _ in range(rounds))
    return [means[int(rounds * 0.025)], means[int(rounds * 0.975) - 1]]


def percentile(values, share):
    values = sorted(v for v in values if v is not None)
    if not values:
        return None
    return values[min(len(values) - 1, int(round(share * (len(values) - 1))))]


def summarize(models, scenarios, answers, wins, validation, review, judge_cost):
    per_model = {}
    for model in models:
        mine = [a for a in answers if a["model"] == model["name"]]
        done = [a for a in mine if a["ok"]]

        def by_scenario(field):
            out = []
            for scenario in scenarios:
                values = [a[field] for a in done if a["scenario"] == scenario["id"] and a.get(field) is not None]
                if values:
                    out.append(statistics.fmean(values))
            return out

        coverage, fidelity, passing = by_scenario("coverage"), by_scenario("fidelity"), by_scenario("passed")
        scores = by_scenario("score")
        record = wins.get(model["name"], {"wins": 0, "ties": 0, "losses": 0, "per_scenario": {}})
        games = record["wins"] + record["ties"] + record["losses"]
        win_by_scenario = [s["score"] / s["games"] for s in record["per_scenario"].values() if s["games"]]
        costs = [a["cost_usd"] for a in done if a.get("cost_usd") is not None]
        cached = [a["cached_tokens"] / a["prompt_tokens"] for a in done if a.get("prompt_tokens")]
        per_model[model["name"]] = {
            "model": model["model"],
            "answers": len(mine),
            "failed": len(mine) - len(done),
            "pass_rate": statistics.fmean(passing) if passing else None,
            "pass_ci": bootstrap(passing),
            "coverage": statistics.fmean(coverage) if coverage else None,
            "coverage_ci": bootstrap(coverage),
            "fidelity": statistics.fmean(fidelity) if fidelity else None,
            "fidelity_ci": bootstrap(fidelity),
            "score": statistics.fmean(scores) if scores else None,
            "score_ci": bootstrap(scores),
            "violations": sum(len(a["violations"]) for a in done),
            "disputed": sum(sum(p["by"] == "disputed" for p in a["points"].values())
                            + sum(c["label"] == "disputed" for c in a["claims"]) for a in done),
            "language_fails": sum(not a["gates"]["language_ok"] for a in done),
            "length_fails": sum(not a["gates"]["length_ok"] for a in done),
            "section_fails": sum(bool(a["gates"]["missing_sections"]) for a in done),
            "invented_numbers": sum(len(a["gates"]["invented_numbers"]) for a in done),
            "dropped_numbers": sum(len(a["gates"]["dropped_numbers"]) for a in done),
            "win_rate": (record["wins"] + 0.5 * record["ties"]) / games if games else None,
            "win_ci": bootstrap(win_by_scenario),
            "wins": record["wins"], "ties": record["ties"], "losses": record["losses"],
            "ttft_p50": percentile([a["ttft_ms"] for a in done], 0.5),
            "ttft_p95": percentile([a["ttft_ms"] for a in done], 0.95),
            "total_p50": percentile([a["total_ms"] for a in done], 0.5),
            "total_p95": percentile([a["total_ms"] for a in done], 0.95),
            "cost_per_answer": statistics.fmean(costs) if costs else None,
            "cached_share": statistics.fmean(cached) if cached else None,
        }
    return {"models": per_model, "validation": validation, "review": review, "judge_cost_usd": judge_cost}


# The run.

# What collecting an answer records; grading adds the rest.
COLLECTED = ("run", "scenario", "model", "ok", "text", "error", "ttft_ms", "total_ms", "prompt_tokens",
             "cached_tokens", "cost_usd")


def setup():
    """bench.toml, checked, with the keys it needs in the environment."""
    load_env(HERE / ".env")
    config = tomllib.loads((HERE / "bench.toml").read_text())
    if len(config["judges"]) != 2:
        sys.exit("bench.toml needs two judges: the examiner, then the adversary")
    graders = [config["decider"], *config["judges"]]
    for entry in config["models"] + graders:
        if not os.environ.get(entry["api_key_env"]):
            sys.exit(f"{entry['api_key_env']} is not set (environment or benchmark/.env)")
    return config


def run(label, only):
    """Collect every model's answers — or only those named in `only` — into a new
    run folder, then grade them."""
    config = setup()
    label = label or config.get("label", "run")
    models = config["models"]
    if only:
        names = [name.strip() for name in only.split(",") if name.strip()]
        unknown = [name for name in names if name not in {m["name"] for m in models}]
        if unknown:
            sys.exit(f"bench.toml has no model {', '.join(unknown)}")
        models = [m for m in models if m["name"] in names]
        config = dict(config, models=models)
    scenarios = load_scenarios(config.get("scenarios", []))
    persona_path = DATA / "persona.md"
    runs = int(config.get("runs", 1))

    print("building eco…", flush=True)
    subprocess.run(["cargo", "build", "--quiet"], cwd=ROOT, check=True)
    # One job per run, scenario and model, each on its own copy of the session.
    jobs = [{"run": r, "scenario": scenario, "model": model}
            for r in range(1, runs + 1) for scenario in scenarios for model in models]
    daemon = Daemon(ROOT / "target" / "debug" / "eco", eco_config(config, persona_path, scenarios), jobs)
    try:
        daemon.collect(jobs)
    finally:
        daemon.stop()
    answers = [{"run": job["run"], "scenario": job["scenario"]["id"], "model": job["model"]["name"], **job["got"]}
               for job in jobs]

    stamp = datetime.now()
    folder = HERE / f"bench-{stamp:%Y-%m-%d-%H%M}-{re.sub(r'[^a-z0-9-]+', '-', label.lower())}"
    folder.mkdir()
    (folder / "config.toml").write_text((HERE / "bench.toml").read_text())
    meta = {"id": folder.name, "label": label, "date": stamp.isoformat(timespec="minutes"), "runs": runs,
            "model_order": [m["name"] for m in models], "scenario_ids": [s["id"] for s in scenarios]}
    (folder / "run.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    # The answers are kept before grading, so a failed grade can be run again.
    write_jsonl(folder / "answers.jsonl", answers)
    grade(folder, config)


def grade(folder, config=None):
    """Grade a run's answers with the scenarios' keys and the judges bench.toml
    names now, and write its judgments and summary."""
    config = config or setup()
    folder = Path(folder).resolve()
    meta = json.loads((folder / "run.json").read_text())
    answers = [{key: value for key, value in json.loads(line).items() if key in COLLECTED}
               for line in (folder / "answers.jsonl").read_text().splitlines()]
    scenarios = load_scenarios(meta["scenario_ids"])
    models = [{"name": name} for name in meta["model_order"]]
    runs = meta["runs"]
    persona = (DATA / "persona.md").read_text()

    judges = Judges()
    decider = config["decider"]
    source = {s["id"]: sources(s, persona) for s in scenarios}
    by_id = {s["id"]: s for s in scenarios}

    def finish(answer, points, claims, score):
        """An answer's grade, from its three checks."""
        scenario = by_id[answer["scenario"]]
        answer["gates"] = gates(answer["text"], scenario, source[scenario["id"]])
        answer["points"], answer["claims"], answer["score"] = points, claims, score
        must_ok, violations = passed(points, scenario)
        answer["violations"] = violations
        must = scenario["must"]
        answer["coverage"] = sum(points[p["id"]]["present"] for p in must) / len(must)
        # A fact the examiner and the adversary dispute counts neither way.
        labels = [c["label"] for c in claims if c["label"] != "disputed"]
        answer["fidelity"] = labels.count("supported") / len(labels) if labels else 1.0
        g = answer["gates"]
        answer["passed"] = float(must_ok and not violations and g["language_ok"] and g["length_ok"]
                                 and not g["invented_numbers"] and not g["missing_sections"]
                                 and not g["dropped_numbers"])
        return answer

    # Blind pairwise comparisons in both orders. They read the answers, not their
    # grades, so they run beside everything else.
    pairs = []
    for scenario in scenarios:
        for r in range(1, runs + 1):
            got = {a["model"]: a for a in answers if a["scenario"] == scenario["id"] and a["run"] == r and a["ok"]}
            names = list(got)
            for i in range(len(names)):
                for j in range(i + 1, len(names)):
                    pairs.append((scenario, r, names[i], names[j], got))

    def order(scenario, r, first, second, got):
        tag = f"pair:{scenario['id']}:{r}:{first}>{second}"
        return compare(judges, decider, scenario, source[scenario["id"]], got[first]["text"], got[second]["text"], tag)

    # Every grader call that does not wait for another runs at once, up to `graders`:
    # each answer's points, claims and score, each validation, each pair's two orders.
    print("grading…", flush=True)
    with ThreadPoolExecutor(int(config.get("graders", 32))) as pool:
        checks = []
        for answer in answers:
            if not answer["ok"]:
                checks.append(None)
                continue
            scenario = by_id[answer["scenario"]]
            text, src = answer["text"], source[scenario["id"]]
            tag = f"{answer['scenario']}:{answer['model']}:{answer['run']}"
            checks.append((pool.submit(check_points, judges, config, scenario, src, text, tag),
                           pool.submit(check_claims, judges, config, text, src, tag),
                           pool.submit(rate, judges, decider, scenario, src, text, tag)))
        validating = validate(pool, judges, config, scenarios, source)
        orders = [(pool.submit(order, scenario, r, a, b, got), pool.submit(order, scenario, r, b, a, got))
                  for scenario, r, a, b, got in pairs]
        answers = [answer if check is None else finish(answer, *(f.result() for f in check))
                   for answer, check in zip(answers, checks)]
        validation = validating()
        matches = []
        for (scenario, r, a, b, _), (first, second) in zip(pairs, orders):
            first, second = first.result(), second.result()
            if first is None or second is None:
                matches.append({"scenario": scenario["id"], "run": r, "a": a, "b": b, "first": first,
                                "second": second, "chance": None, "flipped": False, "winner": "tie"})
                continue
            # The decider leans towards whichever answer comes first, so each order's
            # chance that a given answer is better is averaged, which cancels the
            # lean; only a clear average makes a winner.
            chance = (first + 1 - second) / 2
            winner = a if chance > PAIR_MARGIN else b if chance < 1 - PAIR_MARGIN else "tie"
            # The order alone changed which answer the decider preferred.
            flipped = (first > 0.5) == (second > 0.5)
            matches.append({"scenario": scenario["id"], "run": r, "a": a, "b": b, "first": first,
                            "second": second, "chance": chance, "flipped": flipped, "winner": winner})
    wins = {m["name"]: {"wins": 0, "ties": 0, "losses": 0, "per_scenario": {}} for m in models}
    for match in matches:
        for name, other in ((match["a"], match["b"]), (match["b"], match["a"])):
            record = wins[name]
            scene = record["per_scenario"].setdefault(match["scenario"], {"score": 0.0, "games": 0})
            scene["games"] += 1
            if match["winner"] == name:
                record["wins"] += 1
                scene["score"] += 1
            elif match["winner"] == other:
                record["losses"] += 1
            else:
                record["ties"] += 1
                scene["score"] += 0.5

    # How the adversary took the examiner's verdicts, and how sure the decider was.
    done = [a for a in answers if a["ok"]]
    points = [p for a in done for p in a["points"].values()]
    claims = [c for a in done for c in a["claims"]]
    reviewed = [p for p in points if p["by"] in ("examined", "disputed")] + claims
    disputed = sum(p["by"] == "disputed" for p in points) + sum(c["label"] == "disputed" for c in claims)
    review = {"points": len(points), "by_regex": sum(p["by"] == "regex" for p in points),
              "by_decider": sum(p["by"] == "decider" for p in points),
              "examined": sum(p["by"] in ("examined", "disputed") for p in points), "claims": len(claims),
              "reviewed": len(reviewed), "disputed": disputed,
              "agreement": 1 - disputed / len(reviewed) if reviewed else None,
              "flipped_pairs": sum(m["flipped"] for m in matches), "pairs": len(matches)}

    models = [{"name": m["name"], "model": model_id(folder, m["name"])} for m in models]
    # What grading this run costs, whether the replies were paid now or cached before.
    cost = sum(call["cost"] or 0 for call in judges.calls)
    summary = summarize(models, scenarios, answers, wins, validation, review, cost)
    summary.update({
        "id": meta["id"], "label": meta["label"], "date": meta["date"], "runs": runs,
        "graded": datetime.now().isoformat(timespec="minutes"),
        "decider": decider["name"], "judges": [j["name"] for j in config["judges"]],
        "model_order": meta["model_order"],
        "scenarios": [{"id": s["id"], "title": s["title"], "language": s["language"], "skill": s["skill"],
                       "kind": s["kind"], "lines": len(s["transcript"]), "question": question(s),
                       "reference": s["reference"].strip(),
                       "points": [{"id": p["id"], "kind": p["kind"], "text": p["text"]} for p in point_list(s)]}
                      for s in scenarios],
    })
    write_jsonl(folder / "answers.jsonl", answers)
    write_jsonl(folder / "judgments.jsonl", judges.calls + [dict(m, purpose="pair") for m in matches])
    (folder / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    index()
    print(f"wrote {folder.relative_to(ROOT)}; open benchmark/bench.html")
    for name, m in summary["models"].items():
        print(f"  {name:24} pass {fmt(m['pass_rate'])}  coverage {fmt(m['coverage'])}  "
              f"fidelity {fmt(m['fidelity'])}  wins {fmt(m['win_rate'])}  ttft p50 {m['ttft_p50']} ms")


def model_id(folder, name):
    """The provider's id of model `name`, as the run's config named it."""
    config = tomllib.loads((folder / "config.toml").read_text())
    return next(m["model"] for m in config["models"] if m["name"] == name)


def fmt(value):
    return "—" if value is None else f"{value:.0%}"


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows))


def index():
    """runs.js: every run's summary and answers, for bench.html to read from disk."""
    runs = []
    for folder in sorted(HERE.glob("bench-*")):
        summary_path = folder / "summary.json"
        if not summary_path.exists():
            continue
        summary = json.loads(summary_path.read_text())
        summary["answers"] = [json.loads(line) for line in (folder / "answers.jsonl").read_text().splitlines()]
        runs.append(summary)
    (HERE / "runs.js").write_text("window.BENCH_RUNS = " + json.dumps(runs, ensure_ascii=False) + ";\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    run_parser = sub.add_parser("run", help="run the benchmark bench.toml describes")
    run_parser.add_argument("--label", default=os.environ.get("usage_label") or None)
    run_parser.add_argument("--models", default=os.environ.get("usage_models") or None,
                            help="only these models of bench.toml, by name, comma-separated")
    grade_parser = sub.add_parser("grade", help="grade a run's answers again, with today's keys and judges")
    grade_parser.add_argument("folder", nargs="?", default=os.environ.get("usage_folder"))
    sub.add_parser("index", help="rebuild runs.js from the run folders")
    args = parser.parse_args()
    if args.command == "run":
        run(args.label, args.models)
    elif args.command == "grade":
        if not args.folder:
            parser.error("grade needs a run folder")
        grade(args.folder)
    else:
        index()


if __name__ == "__main__":
    main()
