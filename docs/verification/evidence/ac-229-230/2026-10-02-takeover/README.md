# Voice takeover fixture check — 2026-10-02

Source subsequently committed as `ba3eb629` in [PR #52](https://github.com/beelol/overseer/pull/52). The docs-only merge is `7a7182e2`. This is partial evidence, not a full-suite or complete-criterion claim.

```sh
CARGO_TARGET_DIR=/private/tmp/overseer-closeout-pr49-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20 cargo test -p overseerd --test voice --test overseer_modes --test permission_card_answers --test voice_confirm_targets
```

The [raw run](voice-integrated.log) exited 0: mode tests 7/7, permission-card tests 11/11, voice tests 51/51, confirmation-target tests 6/6. Counts include shared helper tests. Each daemon uses its own temporary home and fixture harness; voice is simulated. No microphone or paid inference was used.

Focused red regressions first reproduced the owner-triggered Auto countdown, an unrelated permission notice becoming a spoken answer, missing task/mode/action read-backs, native No mismatch, click/native permission queue stalls, stale UI asking state, and wrong or implicit confirmation targets. Their lasting checks are in the three fixture test files and overseer_modes.rs at the linked commit.

The listener vocabulary unit tests, full workspace regression and fresh packaged UI still need final results. The full command runs in `/Users/bilal/.codex/worktrees/verify-voice/overseer`, a throwaway copy at `7a7182e2` with current main merged, using its own target and the machine-wide lock. Its coordinator log is `/private/tmp/overseer-closeout-20261002/pr52-full.log`.
