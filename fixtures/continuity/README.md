# Continuity fixtures (synthetic)

Transcripts replayed through `fixtures/fake-harness/replay.js` for the Continuity tests (Gate L).
They are **synthetic**: shaped like Codex 0.155's `exec --json` events, with error texts of the kinds
the harnesses print. They never count as live evidence.

| File | What it is |
| --- | --- |
| `codex-network-error.jsonl` | A turn that fails because the provider's host cannot be found (a connection error) |
| `codex-outage.jsonl` | A turn that fails because the provider answers 503 |
| `codex-rate-limit.jsonl` | A turn that fails on 429 (an account state, never a connection state) |
| `codex-usage-limit.jsonl` | A turn that fails on the usage limit (an account state) |
