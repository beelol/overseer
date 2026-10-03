# Whistle versus Overseer Whisper: independent evaluation audit

Checked 2026-10-02. The owner asked to try Cactus Whistle and evaluate whether the existing eval agent's comparison was trustworthy. No production recognizer change was made.

The existing benchmark is useful preliminary evidence. A separate Codex reviewer independently recomputed normalized word error and exact-match counts from the raw transcripts, checked repetitions, compared the harness to the source, and inspected the Metal probe. It supports keeping Whisper small.en pending human-voice evaluation.

| Metric | Whistle | Whisper small.en |
| --- | ---: | ---: |
| Clean word error rate | 89 / 372 = 23.92% | 16 / 372 = 4.30% |
| Clean exact transcripts | 31 / 60 | 49 / 60 |
| Noisy word error rate | 79 / 171 = 46.20% | 49 / 171 = 28.65% |
| Warm recognition median | 12.93 ms | 73.82 ms |
| Warm recognition p95 | 28.48 ms | 99.40 ms |
| Peak process RSS | 86.16 MiB | 715.31 MiB |

There are 102 clip IDs and four calls per recognizer for every ID. The speech corpus contains only 20 distinct sentences spoken by three synthetic voices, plus noisy, prefix and non-speech variants. Flo contributes 61 of Whistle's 89 clean word errors. Excluding Flo still favors Whisper (11.29% versus 3.23% WER). These are not 102 independent human recordings.

## Limits of the result

- The Whisper source was pinned to `ed092328d4fd2bcaca66c355982699330ef05a5c`. It uses the old fixed vocabulary. Pending AC-229 expands the vocabulary and adds dynamic repository/agent hints, so this is not the exact pending build's baseline. Core decode settings match.
- Timings cover recognition, not microphone capture, endpoint detection, orchestration or response. Whistle's timer excludes Python conversion and JSON parsing; Whisper includes padding and cleanup. Models ran sequentially, without alternating order. The speed advantage is credible, but an exact end-to-end speedup is not proven.
- RSS is process memory, not complete GPU/system allocation. Startup has only one cache-uncontrolled observation.
- Whistle loses important words including hold, allow and negation in some examples. Whisper also misses some short Stop/Cancel clips. Word error alone does not measure whether the correct command would dispatch.
- Runtime/model/source hashes and a locked Rust harness were retained. Generated audio and model downloads were deleted; recorded PCM hashes cannot now be compared with their bytes. The generator hardcodes the former checkout and needs a path fix for reproduction.
- Telemetry behavior is not established by the archived scripts. The official Needle README describes opt-out environment flags while the Whistle announcement says the engine reads no environment variables. This is a documentation/runtime question, not evidence that recordings were uploaded. Verify the pinned runtime and run the next comparison offline.

## Next useful experiment

Use 20–30 owner-supplied human clips, including short controls, negation, repository and agent names, and ordinary room noise. Preserve hashed PCM locally; compare the current prompt to equivalent Whistle keywords; alternate model order; measure the full wrapper; score command meaning/dispatch separately from word error. The owner must supply clips or request a guided recording session. Do not start microphone capture or change the installed daemon as part of this audit.

## Evidence and reproduction

The original artifacts are local to the Mac at `/Users/bilal/.codex/visualizations/2026/10/03/01a0ffb5-2c3c-7910-a1b9-03c7c2d4e864/voice-benchmark/`: `REPORT.md`, `REPRODUCE.md`, raw JSONL for both models, metadata, manifest, Rust harness and Metal probe logs. Independent scorer and recomputed output are under `/private/tmp/overseer-eval-audit/`; these temporary paths are provenance, not portable repository evidence. Archive the source artifacts before citing them to close an AC. No AC is closed by this evaluation.

Primary references: [Cactus Whistle announcement](https://www.cactuscompute.com/blog/whistle), [Needle source and runtime documentation](https://github.com/cactus-compute/needle). Cactus's headline comparison uses Whisper base, while Overseer uses small.en.
