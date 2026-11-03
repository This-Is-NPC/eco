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

eco now clusters online, as it hears each region (design.md §4). Its
numbers come first; the tables after them are from the average-linkage
clustering eco ran when the recording ended, kept to compare.

### Online clustering (2026-10-14)

The same four dev meetings, collar 0, CAM++, merge distance 0.3, min share
2% (`mise run bench:diarization`, release build). Missed speech 19.5% and
false alarm 3.7% are the same for every threshold: only who is who changes.
Speakers found are per meeting, against 4 in each.

| New-speaker threshold | 0.3 | 0.4 | 0.5 | 0.6 | **0.7** | 0.8 |
|---|---|---|---|---|---|---|
| Confusion % | 6.0 | 5.3 | 6.7 | 5.9 | **5.7** | 11.2 |
| DER % | 29.2 | 28.5 | 29.9 | 29.1 | **28.9** | 34.4 |
| Speakers found (IS1008a, ES2011a, TS3004a, IB4001) | 27, 31, 31, 32 | 17, 29, 32, 26 | 13, 15, 22, 28 | 8, 8, 11, 12 | **4, 4, 8, 7** | 4, 3, 4, 4 |

A merge distance of 0.4 gave the same table within 0.1 point; 0.5 joined two
of ES2011a's speakers (confusion 34%). eco starts a speaker at 0.7: the
fewest extra speakers short of joining real ones, at 2.4 points of DER and
2.4 of confusion more than the clustering at the end (26.5 / 3.3 below).
TS3004a still finds 8 speakers and IB4001 7. 51–58× real time; peak RSS
319 MB is the harness, which holds the whole WAV.

### Average linkage at the end (2026-10-05)

`mise run bench:diarization <dir>`: eco's diarization over 16 kHz mono WAVs
with a reference RTTM (and UEM). Silero VAD and the segmenter cut speech
regions as in a session; WeSpeaker CAM++ (Apache-2.0, through tract) embeds
1.5 s windows every 0.75 s; average-linkage clustering on cosine distance
stops at 0.65, and groups under 2% of the windows join the nearest larger
one. DER follows pyannote.metrics: overlapped speech scored, collar split
half before and half after each reference boundary. On the four dev
recordings below the harness agrees with pyannote.metrics 4.1 to 0.1 point
for every recording.

Data: AMI Mix-Headset (CC BY 4.0), RTTMs and UEMs of
`pyannote/AMI-diarization-setup` (`only_words`). The threshold and the
model were chosen on four dev meetings (IS1008a, ES2011a, TS3004a, IB4001);
the test meetings were run once with them.

| Dev, collar 0 | Threshold 0.45 | 0.55 | 0.65 | 0.75 | 0.85 |
|---|---|---|---|---|---|
| CAM++ (512-d), DER / confusion | 35.1 / 11.9 | 28.0 / 4.7 | **26.5 / 3.3** | 31.7 / 8.5 | 38.5 / 15.3 |
| ResNet34-LM (256-d), DER / confusion | — | 31.1 / 7.9 | 27.6 / 4.4 | 29.4 / 6.2 | 34.2 / 11.0 |

ResNet34-LM is also half as fast (25–30× against 53–60× real time), so
CAM++ is the model `eco setup` downloads.

Test set, 16 meetings, 9 h, CAM++ at 0.65, against the published
pyannote/speaker-diarization-3.1 run on the same set (same "full" setup:
no collar, overlap scored):

| | Missed | False alarm | Confusion | DER |
|---|---|---|---|---|
| eco, collar 0 | 21.7% | 2.5% | 2.6% | 26.7% |
| pyannote 3.1, collar 0 | 9.5% | 3.6% | 5.7% | 18.8% |
| eco, collar 0.5 s (±0.25 s) | 17.7% | 0.6% | 1.1% | 19.4% |

eco confuses speakers less than pyannote; its gap is missed speech:
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

## Baseline and diagnosis (2026-11-03)

The online clustering as it stands (commit `df2cab8`), measured before
anything changes, so a later change is judged against these numbers.
`mise run bench:diarization`, release build, CAM++ (`wespeaker_campplus.onnx`,
sha256 `b50810498b5bcf5773d086f6993d344476bd0c88b566a41e8d801aaf8461efad`),
collar 0, threshold 0.7, merge 0.3, min share 2%, settle 120 windows.
The dev set is IS1008a, ES2011a, TS3004a and IB4001. The held-out set is
IS1009b, ES2004b, TS3003b and EN2002b, picked once from the test meetings
(`heldout.txt`, not used to choose anything). Data: AMI Mix-Headset
(CC BY 4.0).

How each column is read:

- **purity**: of the time found speakers share with the reference, the part
  in each one's main reference speaker. **cover**: the same from the reference
  side. Both count only speech the reference and eco both hold, so missed
  speech and false alarm do not move them.
- **<5%**: found speakers holding under 5% of the found speech. A real but
  quiet speaker counts too.
- **churn %**: speech eco told as one speaker and later moved to another (the
  speakers joined), over all speech told. **stayed**: seconds of recording, on
  average, until the move.
- **offline**: the average-linkage clustering eco ran when the recording ended
  (threshold 0.65, min share 2%), over the same windows.

| Dev | Speakers | Found | Missed | F. alarm | Confusion | DER | Purity | Cover | <5% | Churn % | Stayed s | Offline found / DER / purity |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| IS1008a | 4 | 4 | 7.7 | 3.9 | 1.9 | 13.5 | 94.1 | 94.1 | 0 | 1.6 | 279 | 4 / 12.7 / 94.9 |
| ES2011a | 4 | 4 | 25.6 | 1.6 | 4.1 | 31.3 | 80.5 | 82.0 | 1 | 1.3 | 176 | 4 / 31.5 / 80.4 |
| TS3004a | 4 | 8 | 21.2 | 4.7 | 12.7 | 38.5 | 72.6 | 82.3 | 5 | 3.3 | 265 | 4 / 29.8 / 81.7 |
| IB4001 | 4 | 7 | 20.8 | 4.1 | 4.0 | 29.0 | 80.0 | 79.6 | 4 | 4.1 | 106 | 4 / 28.2 / 80.3 |
| **All** | | | 19.5 | 3.7 | 5.7 | 28.9 | 81.1 | 83.5 | 10 | 2.8 | 175 | 26.5 / 83.4 |

| Held-out | Speakers | Found | Missed | F. alarm | Confusion | DER | Purity | Cover | <5% | Churn % | Stayed s | Offline found / DER / purity |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| IS1009b | 4 | 8 | 12.2 | 1.8 | 1.8 | 15.9 | 87.9 | 87.6 | 4 | 1.4 | 112 | 4 / 15.6 / 87.9 |
| ES2004b | 4 | 5 | 17.6 | 1.8 | 1.5 | 20.8 | 88.0 | 88.0 | 1 | 1.1 | 124 | 4 / 20.9 / 87.9 |
| TS3003b | 4 | 5 | 16.5 | 3.1 | 1.9 | 21.4 | 92.9 | 92.9 | 1 | 0.7 | 123 | 4 / 20.8 / 93.7 |
| EN2002b | 4 | 8 | 28.5 | 2.0 | 7.8 | 38.3 | 70.5 | 66.9 | 3 | 1.2 | 114 | 4 / 34.6 / 70.8 |
| **All** | | | 18.6 | 2.1 | 3.2 | 24.0 | 84.7 | 83.7 | 9 | 1.1 | 118 | 22.9 / 84.9 |

Online against offline: 2.4 points of DER on the dev set (28.9 against 26.5)
and 1.1 on the held-out set (24.0 against 22.9); offline always finds the 4
speakers. The whole gap is confusion, on three meetings: TS3004a (8.7
points), EN2002b (3.7) and IB4001 (0.8). Elsewhere online and offline agree
within 0.8.

### Found speakers over time

Of the 19 found speakers under 5% of the speech (10 dev, 9 held-out), 18 lived
1 to 8 seconds, and 17 were born in the last 2 minutes of the meeting. Born
in the last minutes, they cannot settle: the fold into a larger speaker waits
for 120 windows (about 90 s of speech) after the birth. Examples, in seconds
(speech, born, meeting end): TS3004a five extras of 1 to 2 s born from 1293
to 1309 (end 1329); IS1009b 5, 1, 1, 1 s born at 1927, 2011, 2024, 2027 (end
2028); EN2002b 8, 1, 1 s born at 1730, 1767, 1771 (end 1786). The other two:
a real fourth speaker of IB4001 (FIO093, 65 s, 80% pure) that the share
threshold counts as small, and one extra of ES2011a born 123 s before the
end. The extras cost little: a few seconds
of speech each. What they cost is the speaker count (4 reference speakers,
5 to 8 found in 6 of 8 meetings).

The bigger error is long-lived: TS3004a's first speaker holds 249 s of which
48% is MTD015UID and the rest others; EN2002b's second speaker holds 110 s of
which 31% is MEE073 and 69% others. Purity is 70% to
80% on TS3004a, IB4001 and EN2002b (ES2011a 80.5%) against 88% to 94% on the
other four. The offline
result separates most of them (TS3004a purity 81.7%).

### Margin between the two nearest voices

Per window, cosine similarity to the nearest final voice minus the second
nearest. Share of windows under 0.05 / 0.1 / 0.2, and the median:

| Meeting | under 0.05 | under 0.1 | under 0.2 | median |
|---|---|---|---|---|
| IS1008a | 0.4% | 0.9% | 1.6% | 0.628 |
| ES2011a | 3.9% | 6.7% | 18.1% | 0.337 |
| TS3004a | 2.9% | 5.7% | 13.2% | 0.435 |
| IB4001 | 2.9% | 6.0% | 12.0% | 0.423 |
| IS1009b | 2.4% | 4.1% | 8.5% | 0.482 |
| ES2004b | 0.9% | 1.8% | 4.7% | 0.493 |
| TS3003b | 0.8% | 2.1% | 6.8% | 0.431 |
| EN2002b | 4.2% | 9.6% | 21.9% | 0.358 |

At most 10% of the windows are within 0.1 of a second voice. The margin is
measured against the voices at the end, not the ones the online path had when
it decided; how many wrong windows are among the ambiguous ones was not
measured.

### Churn

The online path never reassigns a window; the only change after a speaker is
told is a join. On the dev set 102 s of 3622 s (2.8%) moved to another speaker
after being told, and they stayed with the old one for 175 s on average and up
to 594 s. On the held-out set 74 s of 6663 s (1.1%), 118 s on average, up to
225 s. A name or an answer built on such speech meanwhile has the wrong
speaker.

### Existing constants

Merge 0.3 / 0.4, min share 0.01 / 0.02 / 0.05 and settle 60 / 120, from the
same embeddings (offline unchanged, 26.5 dev and 22.9 held-out):

| Merge | Min share | Settle | Dev DER / conf. | purity / cover | <5% | churn % | Held-out DER / conf. | purity / cover | <5% | churn % |
|---|---|---|---|---|---|---|---|---|---|---|
| 0.3 | 0.02 | 120 | 28.9 / 5.7 | 81.1 / 83.5 | 10 | 2.8 | 24.0 / 3.2 | 84.7 / 83.7 | 9 | 1.1 |
| 0.3 | 0.02 | 60 | 28.9 / 5.7 | 81.0 / 83.5 | 7 | 2.7 | 24.0 / 3.2 | 84.7 / 83.7 | 8 | 1.2 |
| 0.3 | 0.01 | 120 | 29.8 / 6.6 | 81.1 / 82.4 | 11 | 1.3 | 24.0 / 3.2 | 84.7 / 83.7 | 9 | 1.1 |
| 0.3 | 0.01 | 60 | 29.7 / 6.5 | 81.1 / 82.5 | 8 | 1.4 | 24.0 / 3.2 | 84.7 / 83.7 | 8 | 1.2 |
| 0.3 | 0.05 | 120 | 32.2 / 9.0 | 77.4 / 85.4 | 8 | 5.1 | 24.9 / 4.1 | 83.7 / 82.8 | 9 | 1.9 |
| 0.3 | 0.05 | 60 | 35.6 / 12.4 | 73.7 / 86.4 | 6 | 5.0 | 27.7 / 6.9 | 80.6 / 83.9 | 8 | 4.1 |
| 0.4 | 0.02 | 120 | 28.9 / 5.7 | 81.1 / 83.5 | 10 | 2.8 | 23.9 / 3.1 | 84.7 / 83.8 | 8 | 1.2 |
| 0.4 | 0.02 | 60 | 28.9 / 5.7 | 81.0 / 83.5 | 7 | 2.7 | 23.9 / 3.1 | 84.7 / 83.8 | 7 | 1.3 |
| 0.4 | 0.01 | 120 | 29.8 / 6.6 | 81.1 / 82.4 | 11 | 1.3 | 23.9 / 3.1 | 84.7 / 83.8 | 8 | 1.2 |
| 0.4 | 0.01 | 60 | 29.7 / 6.5 | 81.1 / 82.5 | 8 | 1.4 | 23.9 / 3.1 | 84.7 / 83.8 | 7 | 1.3 |
| 0.4 | 0.05 | 120 | 32.2 / 9.0 | 77.4 / 85.4 | 8 | 5.1 | 24.8 / 4.0 | 83.7 / 82.9 | 8 | 2.0 |
| 0.4 | 0.05 | 60 | 35.6 / 12.4 | 73.7 / 86.4 | 6 | 5.0 | 27.6 / 6.8 | 80.6 / 83.9 | 7 | 4.0 |

No combination beats the default by more than 0.1 point of DER. Merge 0.4
changes nothing on the dev set and 0.1 point on the held-out set. A share of
5% folds real speakers (confusion up 1 to 7 points); settle 60 removes one to
three extras but not the confusion. The constants are at their best already,
so the remaining gap is not theirs.

### What points where

- Short-lived extras are real but cheap in DER; they are born too late to
  settle. A fold of unsettled small speakers when the recording ends, or a
  speaker that is not told as new until it has been heard a few windows,
  removes the speaker-count error.
- The DER gap to offline is the long-lived mixing in three meetings, not the
  extras and not the low-margin windows: under 10% of windows are
  ambiguous. What offline has that online lacks is the chance to revisit
  early windows with the later voices, so re-clustering the settled speakers
  (a second pass over the windows kept in memory) is the lever for it.
- The fixture `tests/fixtures/ami-TS3004a-tail.emb` holds the CAM++
  embeddings of the last 433 windows of TS3004a with the reference turns of
  that stretch (`.rttm`), AMI (CC BY 4.0). It reproduces the 8 speakers, the
  5 late extras and 64% purity, and is read by `bench::embeddings` tests.

## Speaker embedding choice (2026-11-03)

Does a stronger embedding beat the current CAM++ on the same clustering?
`mise run bench:diarization` on the four dev meetings, release build, collar
0, merge 0.3, min share 2%, settle 120, thresholds 0.5 to 0.8. All models
take the 80-dim Kaldi fbank with the clip mean removed that `TractEmbedder`
feeds, and all load in tract unchanged. Models come from the sherpa-onnx
`speaker-recongition-models` release (sha256 from its `checksum.txt`, checked). Licenses are those of the WeSpeaker and 3D-Speaker repositories; the weights carry no separate notice that was read.

| Model | License | Size | Speed (x real time) |
|---|---|---|---|
| CAM++ (WeSpeaker, current) | Apache-2.0 | 29.3 MB | 18-44 |
| CAM++ LM (WeSpeaker) | Apache-2.0 | 29.3 MB | 23-35 |
| CAM++ en VoxCeleb (3D-Speaker) | Apache-2.0 | 29.6 MB | 19-35 |
| ERes2Net en VoxCeleb (3D-Speaker) | Apache-2.0 | 26.5 MB | 7-9 |
| ResNet293 LM (WeSpeaker) | Apache-2.0 | 114.3 MB | 2 (stopped after IS1008a) |

ERes2NetV2 is published only for Mandarin (`zh-cn`) as ONNX, so it was not
tried; the English ERes2Net is its predecessor. The machine ran other builds
at the same time, so speed varies between runs of the same model (CAM++ ran
18x to 44x across the four meetings); read the ratio between models, not the
figure. Over the whole grid ERes2Net took 621 s against 161 s for CAM++.

Dev set totals (all four meetings; missed 19.5 and false alarm 3.7 are the
same for every model). <5%: found speakers under 5% of the speech.

| Model | Threshold | Confusion | DER | Purity | Cover | <5% | Churn % |
|---|---|---|---|---|---|---|---|
| CAM++ | 0.5 | 6.7 | 29.9 | 81.1 | 82.4 | 64 | 12.5 |
| CAM++ | 0.6 | 5.9 | 29.1 | 81.1 | 83.2 | 26 | 6.4 |
| **CAM++** | **0.7** | **5.7** | **28.9** | **81.1** | **83.5** | **10** | **2.8** |
| CAM++ | 0.8 | 11.2 | 34.4 | 75.0 | 85.5 | 3 | 0.9 |
| CAM++ LM | 0.5 | 7.4 | 30.6 | 80.7 | 81.7 | 62 | 12.4 |
| CAM++ LM | 0.6 | 6.3 | 29.5 | 80.7 | 82.8 | 29 | 6.2 |
| CAM++ LM | 0.7 | 6.1 | 29.3 | 80.7 | 83.0 | 11 | 2.3 |
| CAM++ LM | 0.8 | 8.2 | 31.4 | 78.2 | 83.9 | 5 | 1.1 |
| CAM++ 3D | 0.5 | 4.8 | 28.0 | 83.3 | 82.0 | 72 | 15.3 |
| CAM++ 3D | 0.6 | 6.0 | 29.2 | 81.3 | 83.1 | 33 | 7.3 |
| CAM++ 3D | 0.7 | 6.0 | 29.2 | 81.2 | 83.0 | 13 | 2.9 |
| CAM++ 3D | 0.8 | 7.5 | 30.7 | 78.9 | 84.7 | 2 | 1.1 |
| ERes2Net | 0.5 | 5.2 | 28.4 | 83.3 | 81.6 | 75 | 16.6 |
| ERes2Net | 0.6 | 7.3 | 30.5 | 81.2 | 81.7 | 39 | 8.4 |
| ERes2Net | 0.7 | 6.6 | 29.8 | 81.1 | 82.2 | 18 | 3.2 |
| ERes2Net | 0.8 | 8.0 | 31.2 | 78.6 | 84.1 | 7 | 1.6 |

Result: no change of model. No candidate beats CAM++ at its best threshold
(0.7, DER 28.9) with purity and cover kept. The nearest are the 3D-Speaker
CAM++ and ERes2Net at 0.5 (DER 28.0 and 28.4, confusion 4.8 and 5.2), but
at 0.5 they leave 72 and 75 short-lived speakers, churn above 15%, and
cover 82.0 and 81.6 against 83.5. Their gain is the lower threshold
splitting speakers more, which the clustering has to undo. At 0.7 neither
is better than CAM++ (DER 29.2 and 29.8). ERes2Net also runs at about a
quarter of CAM++'s speed (7x to 9x real time, under the 25x floor), and
ResNet293 at 2x. The held-out set was not run: the dev set decided.

## Re-clustering (2026-11-03)

The speech a session heard, clustered again as a whole every 400 windows
(about five minutes of speech) and when the recording ends
(`Speakers::recluster`, see [design.md §4](design.md#4-audio)). Same setup as
the [baseline](#baseline-and-diagnosis-2026-11-03): `mise run
bench:diarization`, release build, CAM++, collar 0, threshold 0.7, merge 0.3,
min share 2%, settle 120 windows, and now re-cluster distance 0.5 every 400
windows. The numbers are the replay of the same child logic without the
thread: the result is applied before the next region, so the live child, which
applies it between two regions, can differ by what it hears meanwhile.

The re-cluster distance is lower than the offline 0.65 because it runs on the
mean of a run of windows, whose unit vector is closer to its neighbours than a
single window is. It was chosen on the dev set only, over 0.4 to 0.75 and 200
to 800 windows. Totals for the four dev meetings:

| Distance | Every | Confusion | DER | Purity | Cover | Churn % |
|---|---|---|---|---|---|---|
| 0.40 | 200 / 400 / 800 | 3.4 / 3.7 / 3.5 | 26.6 / 26.9 / 26.7 | 83.3 / 83.0 / 83.2 | 83.6 / 83.3 / 83.6 | 7.6 / 10.3 / 5.7 |
| 0.45 | 200 / 400 / 800 | 3.4 / 3.7 / 3.4 | 26.6 / 26.9 / 26.6 | 83.3 / 83.0 / 83.2 | 83.6 / 83.3 / 83.5 | 7.1 / 9.9 / 5.7 |
| **0.50** | 200 / **400** / 800 | 3.4 / **3.4** / 3.5 | 26.6 / **26.6** / 26.7 | 83.3 / **83.3** / 83.2 | 83.7 / **83.6** / 83.6 | 4.3 / **4.2** / 5.7 |
| 0.55 | 200 / 400 / 800 | 3.4 / 3.4 / 3.5 | 26.6 / 26.6 / 26.7 | 83.3 / 83.3 / 83.2 | 83.7 / 83.6 / 83.5 | 4.7 / 4.3 / 6.4 |
| 0.60 | 200 / 400 / 800 | 5.2 / 5.2 / 5.2 | 28.4 | 81.3 | 84.0 | 6.2 / 4.0 / 3.7 |
| 0.65 | 400 | 8.6 | 31.8 | 77.6 | 85.0 | 8.3 |
| 0.75 | 400 | 14.1 | 37.3 | 71.7 | 88.4 | 11.9 |

From 0.4 to 0.55 the end of the recording decides: the figures hardly move,
and the choice is the lowest churn (0.5, 400). From 0.6 the groups start to
join two people.

| Dev | Speakers | Found | Missed | F. alarm | Confusion | DER | Purity | Cover | <5% | Churn % | Stayed s | Offline found / DER / purity |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| IS1008a | 4 | 4 | 7.7 | 3.9 | 1.1 | 12.7 | 94.9 | 94.9 | 0 | 1.6 | 446 | 4 / 12.7 / 94.9 |
| ES2011a | 4 | 3 | 25.6 | 1.6 | 4.1 | 31.3 | 80.5 | 82.1 | 0 | 2.2 | 313 | 4 / 31.5 / 80.4 |
| TS3004a | 4 | 4 | 21.2 | 4.7 | 4.1 | 29.9 | 81.6 | 81.6 | 0 | 6.6 | 409 | 4 / 29.8 / 81.7 |
| IB4001 | 4 | 4 | 20.8 | 4.1 | 3.6 | 28.6 | 80.0 | 80.0 | 0 | 5.2 | 182 | 4 / 28.2 / 80.3 |
| **All** | | | 19.5 | 3.7 | 3.4 | 26.6 | 83.3 | 83.6 | 0 | 4.2 | 299 | 26.5 / 83.4 |

| Held-out | Speakers | Found | Missed | F. alarm | Confusion | DER | Purity | Cover | <5% | Churn % | Stayed s | Offline found / DER / purity |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| IS1009b | 4 | 4 | 12.2 | 1.8 | 1.5 | 15.6 | 87.9 | 87.9 | 0 | 1.7 | 117 | 4 / 15.6 / 87.9 |
| ES2004b | 4 | 4 | 17.6 | 1.8 | 1.5 | 20.8 | 88.0 | 88.0 | 0 | 1.2 | 187 | 4 / 20.9 / 87.9 |
| TS3003b | 4 | 4 | 16.5 | 3.1 | 1.6 | 21.1 | 93.3 | 93.3 | 0 | 3.0 | 185 | 4 / 20.8 / 93.7 |
| EN2002b | 4 | 5 | 28.5 | 2.0 | 4.3 | 34.8 | 70.6 | 70.6 | 1 | 7.1 | 365 | 4 / 34.6 / 70.8 |
| **All** | | | 18.6 | 2.1 | 2.2 | 23.0 | 84.8 | 84.8 | 1 | 3.0 | 265 | 22.9 / 84.9 |

Against the baseline and the bars (the held-out set was run once, after the
choice):

| | Baseline | Now | Bar | |
|---|---|---|---|---|
| Dev DER | 28.9 | 26.6 | at most 29.2 | met |
| Dev confusion | 5.7 | 3.4 | not above 5.7; within 0.5 of offline (3.3) | met |
| Dev purity / cover | 81.1 / 83.5 | 83.3 / 83.6 | not below 80.1 / 82.5 | met |
| Dev found | 4, 4, 8, 7 | 4, 3, 4, 4 | TS3004a and IB4001 at most 5 | met |
| Held-out DER | 24.0 | 23.0 | at most 24.3 | met |
| Held-out confusion | 3.2 | 2.2 | not above 3.2 | met |
| Held-out purity / cover | 84.7 / 83.7 | 84.8 / 84.8 | not below 83.7 / 82.7 | met |
| Held-out found | 8, 5, 5, 8 | 4, 4, 4, 5 | | |

Speakers born in the last minutes are gone: 10 dev and 9 held-out found
speakers under 5% became 0 and 1. The fourth speaker of IB4001 (about 65 s)
stays. ES2011a finds 3 of its 4 speakers, where the baseline and the offline
clustering found 4. The missing one is FEE042, who speaks 24.6 s of the
meeting's 938 s (2.6%). It is not the 2% fold: with the minimum share at 1% the
result is the same, and at 0 the meeting splits into 82 speakers. It is the
linkage: FEE042's stretches sit within 0.5 of another group on average, and
only a distance of 0.25 separates them, at which confusion goes from 4.1 to
12.8 and DER from 31.3 to 40.0. Offline linkage over single windows finds a
fourth group but scores the same DER (31.5), so nothing measurable is lost; no
guard on the fold fixes it, and none was added. The
`Speakers::recluster` tests pin both sides of the fold on synthetic data: 20
windows of 720 (2.8%) stay, 5 of 725 (0.7%) fold. The recorded tail of TS3004a (`tests/fixtures`), which the
online path split into 8 speakers, comes out as the 4 who spoke.

Churn rose from 2.8% to 4.2% on the dev set and from 1.1% to 3.0% on the
held-out set: what is told live is corrected later. The longest a corrected
stretch stayed wrong was 1,214 s on the dev set.

### Cost over 3 hours

`mise run bench:diarization <dir> --list <dev> --stack 3` repeats the four dev
meetings to 3.1 h of audio (1,814 regions, 9,827 windows) and hears them
without, then with, re-clustering as the child does: each re-cluster on its own
thread, applied between two regions. This is the algorithm in the bench
process, not the `eco diarize` child; memory is the peak of the process with
the figure reset before each pass, so the difference is what the stretches and
the distances add. The machine has 12 cores.

| | Budget | Measured |
|---|---|---|
| Slowest re-cluster, 2,575 stretches | 2 s | 0.25 s |
| Peak memory added | 50 MB | 12 MB (265 MB without, 277 MB with) |
| Longest step of the hearing thread (a region, or applying a result) | | 20 ms, against a region every 6.1 s |
| Regions waiting | under 32 | none: a re-cluster never blocks `hear` |

The real child, `eco diarize` from the release build, fed 3.00 h of the same
meetings through its stdin as 10 s regions (all of it as speech, so more
windows than the VAD would keep), its peak RSS read from `/proc` while it ran,
including the clustering at the end of the input: 216 MB, against 191 MB for
the same run on the commit before re-clustering (58eeb19), 25 MB more.

Before the distances were computed on all cores the slowest re-cluster took
2.31 s over 3,476 stretches (4.3 h); on one core the 3 h figure would be about 1.3 s,
scaled by the square of the stretches and not measured. A fourth hour holds about 3,500
stretches, which the first figure already covers.

## Test set with re-clustering (2026-11-03)

The 16 test meetings of the [2026-10-05 table](#average-linkage-at-the-end-2026-10-05)
(9.0 h), run once on the final settings: `mise run bench:diarization <dir>
--list test.txt`, release build, CAM++, collar 0, threshold 0.7, merge 0.3, min
share 2%, settle 120 windows, re-cluster distance 0.5 every 400 windows (see
[Re-clustering](#re-clustering-2026-11-03)). IS1009b, ES2004b, TS3003b and
EN2002b are the held-out four of that section; their rows are the same. Offline
is the average-linkage clustering at the end (0.65) over the same windows.

| Test | Speakers | Found | Missed | F. alarm | Confusion | DER | Purity | Cover | <5% | Churn % | Stayed s | Offline found / DER |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| IS1009a | 4 | 4 | 16.5 | 5.5 | 3.7 | 25.6 | 82.7 | 83.5 | 0 | 6.6 | 262 | 4 / 25.3 |
| IS1009b | 4 | 4 | 12.2 | 1.8 | 1.5 | 15.6 | 87.9 | 87.9 | 0 | 1.7 | 117 | 4 / 15.6 |
| IS1009c | 4 | 4 | 7.5 | 2.9 | 1.6 | 12.0 | 93.1 | 93.1 | 0 | 2.8 | 186 | 4 / 12.1 |
| IS1009d | 4 | 4 | 14.1 | 4.0 | 3.7 | 21.8 | 84.9 | 84.9 | 0 | 5.6 | 229 | 4 / 21.3 |
| ES2004a | 4 | 4 | 24.6 | 2.0 | 2.5 | 29.1 | 81.3 | 81.3 | 0 | 4.7 | 214 | 4 / 29.0 |
| ES2004b | 4 | 4 | 17.6 | 1.8 | 1.5 | 20.8 | 88.0 | 88.0 | 0 | 1.2 | 187 | 4 / 20.9 |
| ES2004c | 4 | 4 | 17.4 | 2.0 | 1.4 | 20.9 | 87.6 | 87.6 | 0 | 1.4 | 355 | 4 / 21.0 |
| ES2004d | 4 | 4 | 22.0 | 3.2 | 2.5 | 27.7 | 82.1 | 82.1 | 0 | 1.9 | 258 | 4 / 27.8 |
| TS3003a | 4 | 3 | 24.9 | 2.4 | 3.7 | 31.1 | 90.2 | 94.1 | 1 | 4.7 | 375 | 2 / 31.1 |
| TS3003b | 4 | 4 | 16.5 | 3.1 | 1.6 | 21.1 | 93.3 | 93.3 | 0 | 3.0 | 185 | 4 / 20.8 |
| TS3003c | 4 | 4 | 18.9 | 3.0 | 1.7 | 23.5 | 92.5 | 92.5 | 0 | 1.1 | 251 | 4 / 23.5 |
| TS3003d | 4 | 4 | 23.1 | 3.9 | 3.2 | 30.3 | 84.4 | 84.4 | 0 | 2.0 | 296 | 4 / 30.2 |
| EN2002a | 4 | 4 | 29.3 | 2.1 | 4.3 | 35.7 | 69.4 | 69.4 | 0 | 5.5 | 191 | 4 / 35.9 |
| EN2002b | 4 | 5 | 28.5 | 2.0 | 4.3 | 34.8 | 70.6 | 70.6 | 1 | 7.1 | 365 | 4 / 34.6 |
| EN2002c | 3 | 3 | 27.9 | 1.3 | 2.2 | 31.4 | 74.5 | 74.5 | 0 | 4.0 | 132 | 3 / 31.2 |
| EN2002d | 4 | 4 | 32.4 | 1.6 | 3.4 | 37.4 | 67.7 | 67.7 | 0 | 12.7 | 729 | 4 / 37.3 |
| **All** | | | 21.7 | 2.5 | 2.6 | 26.8 | 81.7 | 81.9 | 2 | 4.0 | 354 | 26.7 |

Against the 2026-10-05 run and the published pyannote 3.1 run on the same
set (collar 0, overlap scored):

| | Missed | False alarm | Confusion | DER |
|---|---|---|---|---|
| eco, 2026-10-05 (average linkage at the end) | 21.7% | 2.5% | 2.6% | 26.7% |
| eco, online with re-clustering | 21.7% | 2.5% | 2.6% | 26.8% |
| pyannote 3.1 | 9.5% | 3.6% | 5.7% | 18.8% |

The live path now scores as the clustering at the end did: 0.1 point of DER
apart, purity 81.7 and cover 81.9 against 81.8 and 81.9 for the offline
clustering on the same windows. Speakers found match the reference on 14 of 16
meetings: TS3003a finds 3 of 4 (the offline clustering 2), one of them under
5% of the speech, and EN2002b finds 5 for 4, the fifth holding no window at the
end (under 5%). Speech told live and
moved later (churn) is 4.0% (986 s of 24,805 s), 354 s on average until the
move, the longest 2,215 s (EN2002d, 12.7%). The 9 h took 19 min on 12 threads
(28x real time, 21x to 41x per meeting) with a 367 MB peak, the whole WAV held
in memory by the harness; the run of 2026-10-05, offline only, took 11 min.
The collar 0.5 s row was not measured again.

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


## CLI test coverage (2026-10-09)

Measured with `mise run coverage` (cargo-llvm-cov 0.9.1, every unit and
integration test) on the files behind the `eco` command line. Before is the
tree that introduced the task; after is the same tree with the CLI tests.

| File | What it holds | Lines before | Lines after |
|---|---|---|---|
| `src/cli.rs` | the client: each command's socket line, the event that settles it, its failure | 74.74% | 99.83% (3 of 1754 missed) |
| `src/lifecycle.rs` | `eco start`, `stop`, `restart`, `status` and their exit codes | 69.35% | 99.80% (1 of 500) |
| `src/main.rs` | parsing every subcommand and option, and what it runs | 16.67% | 89.22% (69 of 640) |
| `src/setup.rs` | `eco setup`: model downloads and their checksums | 27.50% | 90.62% (12 of 128) |

No test was written for these lines, read in the code and left on purpose:

- `run()` in `src/lib.rs`: the call each command makes
  and the `process::exit` of its code. Each branch only calls a function
  that has its own tests; a test here would restate the match.
- `set_up` in `src/lib.rs` and `run()` and `desktop()` in `src/setup.rs`:
  the production model URLs, the user's real paths and the Hyprland
  adapter put together. A test would reach the network or the user's
  files; what they call (`fetch`, `skill_targets`, the adapter) is tested.
- `local_date` in `src/cli.rs`, a local time skipped by a clock change: it
  needs the process time zone set, which needs `unsafe`, and the crate
  forbids it.
- The rest are lines of the tests themselves: `panic!` arms that run only
  when a test fails, and the end of fake daemons that serve until the test
  drops them.

## Streaming connection coverage (2026-10-09)

Measured with `mise run coverage` on `src/adapters/websocket.rs`, before and
after the tests of a stalled connection.

| File | What it holds | Lines before | Lines after |
|---|---|---|---|
| `src/adapters/websocket.rs` | a streaming transcription over a WebSocket: audio out, pings, phrases back, a stall ended | 80.81% (19 of 99 missed) | 90.91% (21 of 231) |

No test was written for these lines, read in the code:

- `tls()`: the platform's certificate store, reached only by a `wss://` URL,
  which a local test server does not serve.
- The request headers, the billed request's id and the `Partial` and
  `Nothing` readings: they predate this change and have no test yet.
- The rest are lines of the tests themselves: `panic!` arms that run only
  when a test fails, and the end of a fake provider that waits until the
  test drops it.

## Daemon test coverage (2026-10-10)

Measured with `mise run coverage` (cargo-llvm-cov 0.9.1, unit and integration
tests) after the daemon's tests were written; line numbers read from its
lcov report. `src/` without `src/bench/`: **98.58% of lines** (316 of 22,265
without a test). Before: 81.12% for the whole crate. Files not named below
are at 100%, or miss a line the report counts without naming it (an
artefact of counting each compiled copy of generic code).

Coverage is measured on Linux, so the Windows adapters' lines
(`#[cfg(windows)]`) are not counted; their tests run on the Windows machine.
The rows for `src/session.rs` and `src/paths.rs` were measured again when
those adapters came in.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/session.rs` | 96.10% | `run()` (1949–2042 at measurement): builds the real VAD model, the platform's audio, window control and shortcuts (PipeWire, Hyprland, none), `eco-window` and the user's socket; the Ctrl-C arm of `stop_signal()`, which a test cannot press; `impl Overlay for overlay::Windows`: hands each call to real windows; the omapass key source and the `omapass` command's task: run the user's keyring (what they return is tested through `omapass_listed`); the closing brace of `pipeline()`, which only ends by being aborted. |
| `src/lib.rs` | 89.32% (80 of 749) | the body of `run()` (each command's call, `process::exit`), `open_window()` (the notifier and the real service) and `set_up`'s I/O; `panic!` arms of tests, which run only when a test fails. |
| `src/main.rs`, `src/bin/eco-launch.rs` | 0% (5 of 5 each) | the tokio runtime and the one call each makes, to `eco::run()` and `eco::open_window()`; a test would restate them. |
| `src/paths.rs` | 88.56% | the public wrappers that read the real environment and home (151–212); the layout behind them is tested with a fixed environment. |
| `src/adapters/audio_pipewire.rs` | 79.27% | the trait methods that start the real `pw-record` and `pw-dump` (22–41); the arguments they build are tested. |
| `src/import.rs` | 97.09% | the Silero and speaker-model closures in `run` and `diarizing()`, which start the real `eco diarize` child from the current executable. |
| `src/setup.rs` | 90.62% | `run()` over the production model URLs and the user's paths; `desktop()`, one composition line. `fetch` is at 100%. |
| `src/adapters/omapass.rs` | 95.18% | `program()`, `installed()`, `accounts()`, `secret()`: they read the real PATH and home and would run the user's keyring; `find`, `accounts_of` and `secret_of` are tested. |
| `src/adapters/diarizer_process.rs`, `service_systemd.rs`, `echo_cancel.rs`, `pipewire_devices.rs`, `window_hyprland.rs`, `overlay.rs` | 95.4–98.5% | the one-line entry points into the real `eco diarize`, `systemctl`, `pw-cli`, `pw-dump`, `hyprctl` and `eco-window`, and `Windows::new`, which runs the real `eco-window`; branches only timing reaches (a child already reaped, no event follower). |
| `src/domain/assistant.rs` | 99.62% | states no command sequence reaches (2390, 2393, 2848, 2855, 2924–2925). |
| `src/domain/session.rs` | 98.21% | an `unreachable!` (527). |
| `src/adapters/control_socket.rs`, `local_socket.rs`, `pipe.rs`, `speaker_tract.rs`, `config.rs` | 97–99.7% | a closed listener's accept loop, a pipe read error and tract's input-fact error, which no input provokes; waits inside test helpers; `Address::user()`, the user's own socket and token, which a test never touches. |
| `src/cli.rs`, `src/lifecycle.rs` | 99.6–99.8% | as in the CLI section above. |

The tests found and fixed, each in its own commit: sessions with one title
started in the same second sharing a log file; a failed delete reported as
`people.failed`; line commands and an unknown person's colour refused twice;
a garbled streaming event followed by lines that depended on how bytes
arrived; import progress past the end of the file; a window that connected
during an outage not told the source was down.

## Update and doctor coverage (2026-10-14)

Measured with `mise run coverage` (cargo-llvm-cov 0.9.1, unit and integration
tests) on the tree that added `eco update`, `eco version`, `eco doctor` and the
window's UPDATES group; line numbers read from `cargo llvm-cov report
--show-missing-lines`. Before is the commit the change started from. `src/`
without `src/bench/`, the whole crate: 96.91% of lines before, 96.93% after.
The Windows installer (`src/adapters/installer_windows.rs`) is
`#[cfg(windows)]`: it is not counted here, and its tests run on Windows.

| File | Lines before | Lines after | Left without a test, and why |
|---|---|---|---|
| `src/session/update.rs` | new | 99.61% (2 of 518) | the fake releases' `tagged`, an `unreachable!` of the tests: the daemon always names the tag it installs. |
| `src/adapters/releases_github.rs` | new | 99.23% (1 of 130) | the end of a test server's task. |
| `src/adapters/installer_linux.rs` | new | 97.06% (7 of 238) | `Pacman::new`: reads the real `PATH` and whether stdin is a terminal; `on_path` and the choice between pkexec and sudo are tested with their inputs given. |
| `src/installer.rs` | new | 94.94% (26 of 514) | `installer()` and the empty Linux `tidy()`, one composition line each; `run()` and `run_version()`, which ask the real GitHub through the user's socket (`update()` and `version()` behind them are tested with fake releases and a fake daemon); the end of a fake daemon's task. |
| `src/doctor.rs` | new | 88.92% (41 of 370) | `run()` and `Places::here()`: the user's real executable, models, config and Hyprland file (`checks()` is tested on a temporary home); in `sources()`, a key variable that is set, which a test cannot set without `unsafe`, and omapass's accounts, which run the user's keyring; a Credential Manager that fails, Windows only. |
| `src/session.rs` | 96.23% | 96.79% | as in the daemon section: `run()` now also builds the updates from GitHub, the platform's installer and the user's state file; `restart_detached()` and `installed_exe()`, which run the real `systemd-run` from the running executable (`spawn_quiet`, `undeleted` and `restart_command` are tested). The tests added for this change also drive `impl Overlay for overlay::Windows` with stand-in windows, which had none. |
| `src/main.rs` | 88.74% | 88.90% | the new arms of `main()`, one line each; as before. |
| `src/paths.rs` | 89.24% | 89.89% | as before; `update_state()` is tested. |
| `src/cli.rs`, `src/config.rs`, `src/adapters/desktop_hyprland.rs` | 99.89%, 99.79%, 98.37% | 99.90%, 99.80%, 98.47% | as before. |

## Notifications coverage (2026-10-14)

Measured with `mise run coverage` (cargo-llvm-cov 0.9.1, unit and integration
tests) on the tree that added the system notifications and the window's
toasts; line numbers read from `cargo llvm-cov report --show-missing-lines`.
Before is the commit the change started from. `src/` without `src/bench/`, the
whole crate: 96.93% of lines before, 96.95% after. The Windows adapter
(`src/adapters/notify_windows.rs`) is `#[cfg(windows)]`: it is not counted
here, and its tests run on Windows.

| File | Lines before | Lines after | Left without a test, and why |
|---|---|---|---|
| `src/domain/notifications.rs` | new | 100.00% (0 of 370) | — |
| `src/adapters/notify_linux.rs` | new | 84.48% (9 of 58) | `NotifySend::notify`, which runs the real `notify-send` on a thread of its own and would show a notification on the desktop of whoever runs the tests; the arguments it builds and what it says when the program fails or is missing (`command`, `run`) are tested. |
| `src/session.rs` | 96.79% | 96.81% | as in the update section; `notifier()`, one composition line that builds the real `NotifySend`; in `emitter()`, the terminal's line, which needs a terminal (`emitter()` is tested without one, and the terminal's printing in `src/adapters/terminal.rs`). |
| `src/adapters/overlay.rs`, `src/config.rs` | 96.25%, 99.80% | 96.36%, 99.80% | as before. |

## Long meetings (2026-10-14)

Release builds of this tree and of the commit before it, each against its own
isolated home and socket, the window offscreen and driven over its test
socket; anonymous memory is the window's or the daemon's `Pss_Anon` or
`Anonymous` after a forced `gc()`. Synthetic data: lines of random words, an
answer every 120 lines, a translation of each.

| What | Before | After |
|---|---|---|
| Window, live session fed 6000 lines: anon growth per 1000 lines | 11.0 MB (12.5 overall) | 6.6 MB (8.7 overall); feeding events for a session not on screen grows the same harness by 6.0 MB per 1000, so the timeline's own share is about 0.6 MB |
| Window, rows held after 6000 live lines | 6050 | 1000 (the live cap) |
| Window, 6000 lines put in at once (one page in `tests/window/counts.qml`) | 57 s (an earlier run) | 43–47 ms; 6050 translations after it 13–14 ms |
| Window, a stored 6000-line session opened | shown in 24.1–24.8 s, anon 131 → 210 MB, then 255 MB | shown in 0.06–0.37 s with its newest 300 entries, anon +0–4 MB |
| Window, idle at start | 54 MB | 35 MB |
| Window, settings opened then closed | made at start, so no step | +18 MB at the first opening, kept after it closes; opening it again adds 0–1 MB |
| Daemon, 300 stored sessions of 300 lines: `sessions`, `people`, `tags`, `sessions.search` | idle 168 MB anon, 278 MB after the first calls; 0.12–0.18 s each | 29 MB anon idle, 32 MB after; 0.02 s each, search 0.10–0.12 s |
| `signal` events while a session records, to a client that leaves out `signal` as the tray does | 31.2 per second (every client took every event) | 0; a plain client still gets 31.2 per second |
| Diarizer child, 3 h of synthetic speech in 4 s regions | peak 374 MB RSS, climbing from 0 to 353 MB anon in the 20 s of clustering at the end | peak 124 MB RSS, 101–103 MB anon in the first and last quarter; stopping ends it with no step; it prints 2400 lines, 42 MB in all, one per region |

The search still reads each log whose title and tags do not hold the words,
for its lines.

## Meeting performance coverage (2026-10-14)

Measured with `mise run coverage` on the tree that added timeline counts and
pages, line merge and split, the session index, event subscriptions and live
diarization; line numbers read from `cargo llvm-cov report
--show-missing-lines`. Before is master at the commit the change started from.
The whole crate: 96.95% of lines before, 97.15% after. Files whose numbers
did not move are not named.

| File | Lines before | Lines after | Left without a test, and why |
|---|---|---|---|
| `src/domain/index.rs` | new | 98.51% (2 of 134) | no line is named as missed: the report lists none, and its per-line view shows none at zero. |
| `src/domain/diarization.rs` | 100.00% (0 of 421) | 99.86% (1 of 702) | as for `index.rs`: the summary counts one line that no line view shows at zero. |
| `src/adapters/session_files.rs` | 99.70% (1 of 338) | 99.67% (2 of 605) | as for `index.rs`, before and after. |
| `src/domain/assistant.rs` | 99.62% | 99.72% | states no command sequence reaches (2700, 2703, 3158, 3165, 3238–3239), and in `voices_heard` two joined speakers that hold the same label (1128): a label goes to one speaker only, so no update builds it. |
| `src/adapters/diarizer_process.rs` | 95.69% | 97.61% | `spawn()` and `serve()`, the one-line entry points into the real `eco diarize` and its stdin and stdout; a `panic!` that reports a failed test. |
| `src/adapters/control_socket.rs` | 97.53% | 97.66% | as before, and a test's wait for its two clients to connect. |
| `src/domain/session.rs`, `src/session.rs`, `src/import.rs`, `src/main.rs`, `src/cli.rs` | 98.21%, 96.81%, 97.15%, 88.90%, 99.90% | 98.24%, 96.91%, 97.59%, 89.32%, 99.91% | as before. |

## Appearance coverage (2026-10-20)

Measured on Linux with `mise run coverage -- --no-report --quiet`, followed
by `cargo llvm-cov report --summary-only --show-missing-lines` and JSON
export. The run passed 809 unit tests and 22 integration tests. The added
executable Rust lines are covered; no new uncovered lines were introduced.
The existing gaps and their reasons remain those in the daemon, update and
notifications coverage sections above.

| Changed file | Covered lines | Line coverage |
|---|---|---|
| `src/config.rs` | 1925 of 1929 | 99.79% |
| `src/domain/assistant.rs` | 12676 of 12718 | 99.67% |
| `src/session.rs` | 8972 of 9262 | 96.87% |

The new window test checks all four presets, synthetic system light/dark
preferences, live palette updates, partial theme files, custom color
retention, hexadecimal validation, reset and text contrast. The settings
keyboard test checks Ctrl+9. The bridge test reads the platform theme
properties. Native Windows theme changes still require verification on
Windows; this Linux run does not measure them.

## Windows fixes coverage (2026-10-29)

Measured on Linux with `mise run coverage` on the tree that restarts an
update into the installed eco, refuses an elevated start, treats taken
shortcuts as optional and makes the first model of each type its default.
Before is master at v0.8.0. The run passed 815 unit tests. The whole crate:
97.16% of lines before, 97.17% after (956 and 953 lines without a test).
The Windows-only lines (`src/adapters/service_windows.rs`,
`src/adapters/installer_windows.rs`, the Windows arms of `src/doctor.rs` and
`src/session.rs`) are tested by `mise run check` on Windows, which this
Linux run does not measure. Files whose numbers did not move are not named.

| File | Lines before | Lines after | Left without a test, and why |
|---|---|---|---|
| `src/session.rs` | 96.93% (145 of 4724) | 96.98% (142 of 4698) | as in the daemon and update sections, except that the running executable is now found in `src/session/update.rs`, where a plain restart's test reads it. |
| `src/session/update.rs` | 99.61% (2 of 518) | 99.63% (2 of 546) | as before. |
| `src/lifecycle.rs` | 98.75% (10 of 803) | 98.83% (10 of 855) | as before. |
| `src/config.rs` | 99.81% (2 of 1060) | 99.82% (2 of 1102) | as before. |
| `src/installer.rs` | 94.94% (26 of 514) | 94.78% (26 of 498) | as in the update section. The share falls only because its copy of the fake installer, whose lines were covered, gave way to the one in `src/session/update.rs`; no line lost its test. |
| `src/adapters/installer_linux.rs` | 97.06% (7 of 238) | 97.15% (7 of 246) | as before. |
| `src/ports.rs` | 70.00% (3 of 10) | 78.57% (3 of 14) | as before. |

## Windows installer coverage (2026-10-29)

Measured on Linux with `mise run coverage` on the tree that packages eco for
Windows as a per-machine MSI, ships the models in it, installs updates through
it and starts eco from the Start menu without a console. Before is master at
v0.9.1. The run passed 830 unit tests. The whole crate: 97.16% of lines
before and after (956 and 968 lines without a test, of 33620 and 34087). The
Windows-only code (`src/adapters/installer_windows.rs`,
`src/adapters/service_windows.rs`, the Windows arms of `src/lib.rs` and
`src/setup.rs`) is tested by `mise run check` on Windows, which this Linux run
does not measure. Files whose numbers did not move are not named.

| File | Lines before | Lines after | Left without a test, and why |
|---|---|---|---|
| `src/apply.rs` | new | 100.00% (0 of 147) | none. |
| `src/installer.rs` | 94.94% (26 of 514) | 94.86% (27 of 525) | the same lines as before, as in the update section; the line view shows no new line at zero, the summary counts one more. |
| `src/paths.rs` | 89.89% (27 of 267) | 89.83% (30 of 295) | as before, and `shipped_model`, one call that finds the models beside the running eco: its lookup order is tested through `model`. |
| `src/setup.rs` | 88.55% (15 of 131) | 90.59% (16 of 170) | as before. |
| `src/lifecycle.rs` | 98.75% (10 of 803) | 98.92% (10 of 925) | as before. |
| `src/session/update.rs` | 99.61% (2 of 518) | 99.67% (2 of 600) | as before. |
| `src/ports.rs` | 70.00% (3 of 10) | 78.57% (3 of 14) | as before. |

## Uninstall coverage (2026-10-29)

Measured on Linux with `mise run coverage` on the tree that adds `eco
uninstall`, the daemon's `uninstall` command and the window's UNINSTALL row.
The run passed 859 unit tests. The whole crate: 97.21% of lines (972 of 34780
without a test). The Windows-only code (the removal in
`src/adapters/installer_windows.rs`) is not measured by this Linux run. Its
arguments, exit codes and product filter are tested by `mise run check` on
Windows; the registry read and the hand-off of the copy of `eco.exe` are not
tested by anything, as they need Windows Installer and the shell. Master was
not measured again for this change; the lines new to the files below that have
no test are the ones named.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/apply.rs` | 99.72% (1 of 361) | the line view shows no line at zero; the summary counts one. |
| `src/installer.rs` | 94.68% (32 of 602) | `run_uninstall()`, three lines that ask the real user's socket (`uninstall()` behind it is tested with a fake daemon and installer); the rest as in the update section. |
| `src/lib.rs` | 88.66% (87 of 767) | the `ApplyUninstall` arm of `run()`, which stops the user's daemon and runs the real installer (`apply::uninstall` and `Local` are tested with fakes and a temporary directory); the rest as in the daemon section. |
| `src/session/update.rs` | 99.72% (2 of 703) | as in the update section. |
| `src/session.rs` | 96.93% (145 of 4725) | `removal_notifier()`, one composition line beside `notifier()`, which builds the notifications of the removal step (on Windows, toasts that do not register eco's name); the rest as in the daemon section. Before this change, 96.98% (142 of 4698). |
| `src/adapters/installer_linux.rs` | 98.28% (7 of 408) | `Pacman::new`, which reads the real `PATH` and terminal; the removal's arguments, the unit's arguments, the exit codes and the hand-off through a stand-in `systemd-run` are tested. |

## macOS base seams coverage (2026-10-29)

Measured on macOS 27.0.1 (Apple Silicon) with `cargo llvm-cov --summary-only
--show-missing-lines --bin eco` on the tree that builds eco there: the
daemon's unit tests, 767 passed. `tests/window.rs` and `tests/overlay.rs`
need Qt, which no task installs on macOS yet, so they did not run. The
macOS-only lines are counted here and nowhere else; a Linux run does not
compile them.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/adapters/unsupported.rs` | 96.61% (4 of 118) | `Notifier::notify` (110–112), which only writes a line to the daemon's stderr. |
| `src/paths.rs` | as in the daemon section | the public wrappers that read the real environment and home; the macOS layout behind them is tested with a fixed environment. |

## macOS audio coverage (2026-10-29)

Measured as in the section above, after the macOS audio moved onto the cpal
adapter that Windows uses: the daemon's unit tests, 771 passed, 1 ignored (the
by-hand capture test). The adapter's macOS lines are counted here; a Linux run
does not compile it.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/adapters/audio_cpal.rs` | 52.69% (167 of 353) | everything that opens a real device: `capture_cancelled` (51–53), the host's device list failing (67), the source's open and read loop (177–204: opening, a held frame let go, a refused microphone, a reopened or lost stream), `open` past the device lookup (227–303: the output that is also a microphone, the stream config, the silent stream, the sample formats) and `build` with its callbacks; the test's panic arms (369, 381); and the ignored test that prints this machine's devices and a second of each (447–496), run by hand. The device mapping, the error mapping, a missing device, the listing's defaults and when a microphone counts as refused, its frames held until then, are tested. |
| `src/adapters/unsupported.rs` | 94.81% (4 of 77) | `Notifier::notify` (74–76), which only writes a line to the daemon's stderr; the audio it no longer serves left with its tests. |

## macOS service coverage (2026-10-29)

Measured as above after the launchd agent replaced `Unsupported` as the macOS
service: the daemon's unit tests, 779 passed, 1 ignored.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/adapters/service_launchd.rs` | 88.89% (33 of 297) | `impl ServiceManager for LaunchAgent` (21–38) and `take_off` (173–175): the entry points that run the real `launchctl` on the user's real agent, log and executable; `Agent::user` (51–57), which reads this user's real paths and uid. What they build and run is tested through `start` and `take_off_agent` with a stand-in `launchctl`. The `create_dir_all` error (150), a `~/Library/LaunchAgents` that cannot be made, and a definition that cannot be deleted (196). |
| `src/adapters/daemon_log.rs` | 97.22% (1 of 36) | a log directory that cannot be made (15). |
| `src/adapters/unsupported.rs` | 93.10% (4 of 58) | `Notifier::notify` (59–61), which only writes a line to the daemon's stderr; the service it no longer serves left with its test. |

## macOS notifications coverage (2026-10-29)

Measured as above after the `osascript` notifier replaced `Unsupported` as the
macOS notifier: the daemon's unit tests, 782 passed, 1 ignored.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/adapters/notify_macos.rs` | 90.32% (3 of 31) | `Notifier::notify` (34–36), which runs the real `osascript` and shows a notification on the user's screen; the command it builds is tested. |
| `src/adapters/notify_program.rs` | 75.00% (8 of 32) | `show` (9–15), the thread that runs the program and writes why it failed to the daemon's stderr, where a test cannot read; `run`, which decides what is written, is tested. Shared with Linux, where it replaces the same lines of `notify_linux.rs`. |
| `src/adapters/unsupported.rs` | 98.18% (1 of 55) | the empty path `Installer::program` falls back to when the running executable cannot be found. |

## macOS installer coverage (2026-10-29)

Measured as above after the macOS installer replaced `Unsupported` for
`eco update`: the daemon's unit tests, 785 passed, 1 ignored.

| File | Lines | Left without a test, and why |
|---|---|---|
| `src/adapters/installer_macos.rs` | 88.02% (26 of 217) | `Folder::new` (30–36), which reads the user's real home; the old folder that cannot be moved aside (68); a home with no parent or an `~/Applications` that cannot be made (99–100); tar that cannot run or fails (120–128), which the system's `/usr/bin/tar` does not on a well-formed archive. A release that installs, an archive without eco and a folder that cannot move in are tested. |
| `src/adapters/unsupported.rs` | 100.00% | now only `eco setup`'s desktop step, tested. |


## Diarization coverage (2026-11-03)

Measured on Linux with `mise run coverage` on the tree that measures, splits
and re-clusters live diarization, against a checkout of master measured the
same way. Whole crate: 96.84% of lines (1,175 of 37,158 without a test), against
97.20% (974 of 34,781) on master, the new `src/bench/` (exempt) counting
for 363 of the lines without a test in `src/bench/diarization.rs` alone. No file of `src/`
outside `src/bench/` has a lower percentage than on master.

| File | Master | This tree | Left without a test, and why |
|---|---|---|---|
| `src/adapters/diarizer_process.rs` | 97.61% (9 of 376) | 98.31% (9 of 534) | `spawn` and `serve`, which start and serve the real `eco diarize` process on the real executable and stdin (`spawn_program` and `serve_on` are tested with a fake model and pipes); a panic arm that only runs when a test already failed. |
| `src/adapters/stt_deepgram.rs` | 99.68% (2 of 625) | 99.71% (2 of 679) | as on master. |
| `src/adapters/stt_elevenlabs.rs` | 100.00% | 100.00% | |
| `src/adapters/stt_openai.rs` | 99.04% (2 of 209) | 99.08% (2 of 217) | as on master. |
| `src/adapters/websocket.rs` | 99.16% (3 of 356) | 99.16% (3 of 357) | as on master. |
| `src/domain/assistant.rs` | 99.72% (19 of 6748) | 99.73% (19 of 6931) | as on master. |
| `src/domain/channel.rs` | 99.74% (3 of 1140) | 99.74% (3 of 1161) | as on master. |
| `src/domain/diarization.rs` | 99.86% (1 of 702) | 99.93% (1 of 1442) | the line view shows no line at zero; the summary counts one. |
| `src/domain/linkage.rs` | new | 100.00% | |
| `src/domain/session.rs` | 98.24% (46 of 2610) | 98.35% (47 of 2841) | the `unreachable!` arm of `as_record`, which the type of the records cannot reach; the summary counts one more line than the line view shows. |
| `src/import.rs` | 97.59% (23 of 956) | 97.60% (23 of 960) | as on master. |
| `src/lib.rs` | 88.66% (87 of 767) | 89.19% (87 of 805) | as in the daemon section. |
| `src/ports.rs` | 78.57% (3 of 14) | 90.00% (3 of 30) | as on master. |
| `src/session.rs` | 96.93% (145 of 4725) | 96.93% (145 of 4729) | as in the daemon section. |
