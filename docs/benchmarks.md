# Benchmarks

Answer quality — which model answers a skill best, faithfully and cheaply —
has its own tool: `mise run bench`, described in
[`benchmark/README.md`](../benchmark/README.md), with every run charted in
`benchmark/bench.html`. The measurements below are latency and audio, and
one comparison of benchmark runs.

## M0 — LLM latency (2026-10-03)

`mise run bench:llm`: one PT-BR "suggest a reply" action over a three-line
interview transcript, 3 runs per model on OpenRouter, medians. Measured from
a home connection.

| Model | Time to first token | Total |
|---|---|---|
| anthropic/claude-haiku-4.5 | 920 ms | 2981 ms |
| google/gemini-3.5-flash-lite | 1216 ms | 1494 ms |
| openai/gpt-5.4-mini | 2040 ms | 3965 ms |
| google/gemini-3.8-flash (`minimal` reasoning) | 2119 ms | 3012 ms |
| anthropic/claude-sonnet-5.5 | 5657 ms | 6151 ms |

The < 1.5 s target is end of speech → first token, so it includes the STT:
Deepgram (~0.3 s) + Haiku fits; the LAN whisper.cpp server (~1 s) fits no
model. The fast models invented facts absent from the context (RLS, JWT);
Sonnet 5.5 flagged what to confirm instead.

## LAN models (2026-10-03)

Replay of a 22 s PT-BR interview question (`--replay`), RX 9060 XT:

| Stage | Latency |
|---|---|
| whisper.cpp large-v3-turbo, per segment | ~990 ms |
| Qwen3.5-9B Q8 (`enable_thinking: false`), `probe` action | 356 ms first token, 7.9 s total |
| Qwen3.5-9B Q8, `ask` action | 271 ms first token, 3.4 s total |

## Prompt cache (2026-10-04)

Two consecutive requests built by `conversation()` against the LAN
`llama-server` (Qwen3.5-9B Q8): 60 transcript lines, an action, then one more
line and a free question.

| Request | Prompt tokens | From cache | Time |
|---|---|---|---|
| 1st | 1795 | 0 | 3.7 s |
| 2nd | 1919 | 1791 (93%) | 1.5 s |

The second request repeats the first byte for byte before its new part, so
the server reuses it; a sliding window would have changed the start and
reused nothing.

## Rust daemon vs Python (2026-10-04)

Release build, `eco daemon --headless --no-save`, idle (no meeting) monitoring
the two default inputs through `pw-record` + Silero VAD; CPU averaged over
20–30 s. The `pw-record` children (~0.3% CPU, ~17 MB together) are the same
in both and not counted.

| | Python | Rust, ONNX Runtime | Rust, tract + rustls |
|---|---|---|---|
| RSS | 102 MB | 54 MB | 27 MB |
| CPU while monitoring | 3% | 1.3–1.6% | 1.3–1.4% |
| Start-up | 230 ms import | 6 ms to the socket | 6 ms to the socket |

With ONNX Runtime the first VAD session cost ~26 MB (half code, half heap);
disabling its arena, memory pattern or graph optimisation changed nothing.
tract runs the If-less export of the same Silero v6.2.3 within 1e-4 of the
Python probabilities
for ~4 MB, and rustls on ring replaces the 4.7 MB of OpenSSL pages. What is
left is mostly code: ~14 MB of the binary is resident. Fat LTO took it to
~1 MB less at 4 minutes per release build, thin LTO measured no gain, so
neither is on.

## Diarization (2026-10-05)

`mise run bench:diarization <dir>`: eco's diarization over 16 kHz mono WAVs
with a reference RTTM (and UEM). Silero VAD and the segmenter cut speech
regions as in a session; WeSpeaker CAM++ (Apache-2.0, through tract) embeds
1.5 s windows every 0.75 s; average-linkage clustering on cosine distance
stops at 0.65, and groups under 2% of the windows join the nearest larger
one. DER follows pyansession.metrics: overlapped speech scored, collar split
half before and half after each reference boundary. On the four dev
recordings below the harness agrees with pyansession.metrics 4.1 to 0.1 point
for every recording.

Data: AMI Mix-Headset (CC BY 4.0), RTTMs and UEMs of
`pyansession/AMI-diarization-setup` (`only_words`). The threshold and the
model were chosen on four dev meetings (IS1008a, ES2011a, TS3004a, IB4001);
the test meetings were run once with them.

| Dev, collar 0 | Threshold 0.45 | 0.55 | 0.65 | 0.75 | 0.85 |
|---|---|---|---|---|---|
| CAM++ (512-d), DER / confusion | 35.1 / 11.9 | 28.0 / 4.7 | **26.5 / 3.3** | 31.7 / 8.5 | 38.5 / 15.3 |
| ResNet34-LM (256-d), DER / confusion | — | 31.1 / 7.9 | 27.6 / 4.4 | 29.4 / 6.2 | 34.2 / 11.0 |

ResNet34-LM is also half as fast (25–30× against 53–60× real time), so
CAM++ is the model `eco setup` downloads.

Test set, 16 meetings, 9 h, CAM++ at 0.65, against the published
pyansession/speaker-diarization-3.1 run on the same set (same "full" setup:
no collar, overlap scored):

| | Missed | False alarm | Confusion | DER |
|---|---|---|---|---|
| eco, collar 0 | 21.7% | 2.5% | 2.6% | 26.7% |
| pyansession 3.1, collar 0 | 9.5% | 3.6% | 5.7% | 18.8% |
| eco, collar 0.5 s (±0.25 s) | 17.7% | 0.6% | 1.1% | 19.4% |

eco confuses speakers less than pyansession; its gap is missed speech:
overlapping speakers (one voice per moment in eco) and the edges the VAD
trims. Speakers found matched the reference on 15 of 16 meetings (TS3003a:
2 of 4). The 9 h took 11 min on 12 threads (~49× real time) with a 370 MB
peak, the whole WAV held in memory by the harness.

The parity test (`speaker_tract`) checks the Rust fbank and the tract
embeddings against kaldi-native-fbank and ONNX Runtime on three 2 s clips of
ES2004a (AMI, CC BY 4.0) in `tests/fixtures/speaker-clips.s16`: cosine above
0.999.

### Import with diarization (2026-10-05)

ES2004a (17.5 min) imported through a release daemon with a local STT that
answers at once (two timed phrases per segment), so the time is eco's own:

| | Without the speaker model | With it |
|---|---|---|
| Import time | 2.7 s | 16.7 s |
| Daemon RSS at rest → peak → after | 27 → 45 → 37 MB | 27 → 68 → 39 MB |
| `eco diarize` child peak | — | 259 MB, freed when it exits |

328 lines, 4 speakers found for 4 in the reference; mapping each line to
the reference speaker over its start, about 91% of lines carry the right
one. Embedding in the daemon itself left it at 267 MB after the import
(allocator and thread pool keep what they took), hence the child process.
The extra 22 MB of daemon peak are speech regions queued for the child
while this instant STT runs ahead; with a real STT (whisper.cpp ~1 s per
segment) the STT is the slower side.

### Naming people by voice (2026-10-05)

`eco bench people <dir>`: the AMI series ES2004, IS1009, TS3003 and EN2002
each hold the same four people in meetings a–d. Every speaker eco finds in
an `a` meeting is enrolled as the reference speaker it overlaps most (14
people: two speakers were merged in TS3003a); the 47 speakers found in the
b–d meetings are then ranked against all 14.

| Threshold | Named without asking | Wrong |
|---|---|---|
| 0.30 | 45 | 4 |
| 0.50 | 42 | 1 |
| 0.55–0.85 | 41 | 0 |
| 0.90 | 34 | 0 |

The highest wrong match scored 0.523 and the right ones 0.862–0.974. eco
guesses who a speaker is from 0.70, halfway, for the user to confirm or
clear, and suggests people from 0.40. The six left unnamed all scored below
0.53.

End to end through the daemon: ES2004a imported, its four speakers named by
hand after the reference; ES2004b imported next had all four named without
asking, and 670 of its 713 lines (94%) carry the right person.

## Streaming transcription (2026-10-05)

The first 2 minutes of AMI ES2004a (English, four people in a room) replayed
in real time (`--replay`) into a session, `language = "en"`; latency is from the
end of a phrase's last word in the audio to its line, which includes the
provider's wait for silence. Release build, home connection.

| Provider | Lines | p50 | p90 | Worst |
|---|---|---|---|---|
| Deepgram nova-3 (endpointing 300 ms, utterance end 1 s) | 11 | 1.58 s | 2.83 s | 4.2 s |
| ElevenLabs Scribe v2 Realtime (server default VAD silence) | 8 | 1.88 s | 4.95 s | 4.9 s |
| ElevenLabs Scribe v2 Realtime (VAD silence 0.6 s) | 14 | 1.05 s | 1.45 s | 4.3 s |

Most of Deepgram's phrases end by the 1 s `UtteranceEnd` (its minimum): the
room noise rarely gives 300 ms of silence. Scribe keeps 0.6 s.

Importing the same 2 minutes (diarized too):

| Provider | How | Time |
|---|---|---|
| Deepgram | streamed | 97 s — it transcribes as fast as the audio plays |
| Deepgram | each segment over HTTPS (`utterances=true`), one at a time | 24 s |
| Deepgram | the same, four segments in flight | 7.1 s |
| Scribe | streamed frame by frame | refused (`queue_overflow`) |
| Scribe | streamed, waiting frames joined up to 1 s | 9.5 s, then 2 s of quiet to close |

Not measured yet: PT-BR speech, and the whisper.cpp LAN server on the same
clip (it was off).

## Concurrent sessions and overlay cost (2026-10-08)

Release build on the Omarchy laptop (12 cores), one replayed input (a 4-minute
Deepgram TTS clip, paced in real time), isolated runtime directories. CPU is
percent of one core over 30 s; memory is PSS, which splits shared libraries
between processes.

| Scenario | Daemon CPU | Daemon PSS | Overlay CPU (each) | Overlay PSS (each) |
|---|---|---|---|---|
| No session (inputs measured) | 1.5% | 34 MB | 15.6% | 182 MB |
| One session | 9.9% | 197 MB | 18.1% | 195 MB |
| Two sessions alike, one daemon, two overlays | 9.8% | 168 MB | ~19% | ~195 MB |
| Two sessions on two models, one daemon | 10.1% | 189 MB | ~18.5% | ~189 MB |
| Two daemons, one session each | 9.6% + 9.4% | 178 + 177 MB | ~18% | ~195 MB |

Sessions in one daemon share the capture, the VAD and the diarizer (a child
process per input: 7.8% CPU and 178 MB RSS of the daemon's total); a second
transcription model costs only its connection. Two daemons pay for all of it
twice. Each overlay was its own Quickshell process, as the window ran then,
and shared nothing but the Qt libraries.

Most of the overlay's CPU was its input traces: each sample laid the whole
curve out again (480 points, two strokes). Laying out only the newest piece,
and nothing while no frame is drawn, keeps the same look and frame rate:

| Overlay, one input | Before | After |
|---|---|---|
| Start screen, shown | 15.7% | 6.6% |
| Start screen, on a hidden workspace | 9.7% | 1.5% |
| Recording a session | 18.1% | 9.6% |

### Memory (2026-10-08)

An empty Quickshell window already takes 116 MB PSS here (87 MB with
software rendering, so about 25 MB is the GPU driver); the overlay adds about
65 MB, spread over first-use costs — text at a few sizes (~11 MB), Shapes
(~6 MB), the Quickshell modules its singletons load (~18 MB) — with no single
view or dialog above noise. The overlay's floor is Qt's.

### The window on plain Qt 6 (2026-10-09)

The same overlay, run by `eco-window` (§3 of [design.md](design.md#3-architecture))
and, for comparison, by Quickshell 0.3.1 from the commit before the change.
Release build of `eco-window`, Qt 6.11.2, on Hyprland on the Omarchy laptop;
one window on the start screen against an isolated daemon replaying a WAV, no
session. Time is from starting the process to Hyprland listing its window as
mapped (`hyprctl clients`, polled every 10 ms); memory is PSS six seconds later.
Four runs each; the first `eco-window` run had an empty QML disk cache.

| Window | Time to a mapped window | PSS |
|---|---|---|
| `eco-window` | 315 ms with an empty cache; 175–241 ms after | 120–124 MB |
| Quickshell | 297–314 ms | 196–200 MB |

`eco-window` is 77 KB, built in about 8 s.

The diarizer child held 159 MB RSS for a 29 MB model: glibc kept the tensors
each clip's length sized differently. A fixed `MALLOC_TRIM_THRESHOLD_` (1 MB)
on the child returns them: 65–70 MB, for about 1% of a core. With one session
recording, the daemon went from 197 to 90 MB PSS.

## Answer benchmark with the themed cast (2026-10-09)

The benchmark's persona became Sadao Maia and his cast, with every scenario
keeping its skill, kind, languages and trap. `mise run bench -- --label
sadao`, same `bench.toml` (six models, reasoning off, `runs = 2`, the same
decider and judges), compared with `bench-2026-10-08-0125-no-thinking`, the
last run on the old persona. Old → new per model; score is the decider's
1–5 rubric, unsupported counts the claims graded unsupported, mixed or
contradicted, cost is per 1000 answers.

| Model | Score | Pass | Forbidden hits | Unsupported claims | Invented numbers | First word p50 / p95 | Cost |
|---|---|---|---|---|---|---|---|
| gpt-6-luna | 4.32 → 4.34 | 95% → 95% | 0 → 0 | 13 → 11 | 0 → 0 | 1210 / 1730 → 1096 / 1600 ms | $0.16 → $0.10 |
| haiku-5.5 | 3.92 → 4.03 | 82% → 79% | 1 → 2 | 41 → 30 | 0 → 0 | 1219 / 1456 → 1302 / 1463 ms | $0.33 → $0.36 |
| deepseek-v4.1-flash | 3.79 → 3.80 | 84% → 76% | 0 → 0 | 39 → 34 | 0 → 0 | 660 / 1460 → 824 / 1510 ms | $0.49 → $0.21 |
| mimo-v2.6-pro | 3.42 → 3.74 | 63% → 61% | 2 → 0 | 48 → 62 | 0 → 2 | 2619 / 4289 → 1888 / 7505 ms | $0.35 → $0.54 |
| mimo-v2.6-flash | 3.65 → 3.59 | 87% → 76% | 1 → 0 | 47 → 50 | 0 → 0 | 2096 / 12869 → 2384 / 5253 ms | $0.24 → $0.23 |
| grok-4.20 | 3.39 → 3.29 | 76% → 87% | 0 → 1 | 65 → 58 | 3 → 1 | 890 / 1144 → 2170 / 4574 ms | $1.57 → $2.17 |

Noise, measured two ways: the two repeats inside one run differ by up to
0.33 in a model's score and 16 points in its pass rate; a first themed run,
identical on 17 of the 19 scenarios, differs from the kept one by up to 0.17
and 16 points. Against that:

- No model's score or pass rate moved beyond noise, and the order holds:
  gpt-6-luna leads on every quality measure, haiku-5.5 second.
  mimo-v2.6-pro's +0.32 equals its old repeat spread.
- grok-4.20's first word went from 890 ms to 2170 ms (3434 ms in the first
  themed run) for prompts of the same size: the provider, not the scenarios.
  Cost per answer follows prompt-cache hits: deepseek and gpt-6-luna halved
  between the two themed runs.
- By scenario, only the legacy-stack trap moved beyond its repeat spread
  (0.08–0.19): DB2 on a mainframe scores 3.65 (4.00 in the first themed run)
  where Oracle scored 4.34, with the same length and unsupported claims;
  every model still says it never used DB2. The long meeting still fails
  everywhere (0% pass).

The first themed run also caught two lines testing more than before: "the
night branches" in the English-to-Portuguese meeting, left in English by
half the models, and "Boa noite", a greeting or a farewell in Japanese. Both
went back to their old wording before the kept run. The graders' check found
168 of 171 points (169 before). A run costs about $0.84: $0.14 of answers
and $0.70 of grading ($0.76 before).

