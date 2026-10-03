# AC266 local library qualification

Base: `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. This slice adds two qualification tests to the already reviewed Mods implementation; no production source changed. Both new tests passed against the existing implementation. There was no intended failing behavior reproduced and no RED claim.

`outstanding_preview_survives_crash_before_confirmation_with_original_bytes` saves an unconfirmed preview, deletes its original source, kills/restarts the isolated daemon and confirms the same preview. It asserts exact saved content and identity, result remaining NULL, continued confirmation refusal, original fingerprint/bytes after install, staging cleanup, no binding activation, and idempotent confirmation history after another restart.

`real_ctl_preview_install_update_and_remove_round_trip_with_confirmation` invokes the actual `overseerd ctl` executable for preview/install/update/remove/list. It checks unconfirmed mutations are refused, confirmed and duplicate operations return truthful results, different bytes under version 1 have distinct fingerprints, socket and CLI views agree, and installation does not enable bindings. CLI exit status alone is insufficient: protocol errors are inspected explicitly.

## Executed evidence

All successful commands used `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-verify-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20`, outside the restricted sandbox, with isolated fixture daemons and disabled paid harness paths.

| Command suffix after cargo test -p overseerd | Result | Exact log |
|---|---|---|
| `--test mods outstanding_preview_survives_crash_before_confirmation_with_original_bytes -- --nocapture` | 1 passed, 0 failed | outstanding-preview-green.log |
| `--test mods real_ctl_preview_install_update_and_remove_round_trip_with_confirmation -- --nocapture` | 1 passed, 0 failed | ctl-roundtrip-green.log |
| `--test mods -- --nocapture` | 15 passed, 0 failed (11 library cases + 4 shared helper units) | library-green.log |
| `--bin overseerd mods::library::tests -- --nocapture` | 2 passed, 0 failed (migration and reviewed bundle bytes) | migration-green.log |

`restricted-setup-failure.log` preserves the first restricted attempt: compilation succeeded, but fixture daemon startup failed before the new assertions, nice/setpriority was denied and cleanup could not list processes. It is an execution-boundary failure, not evidence of a product regression. The approved rerun reached all assertions and passed. No process remained under the qualification worktree or target after the successful runs.

No UI, provider turns, owner configuration access or production daemon access occurred. The coordinated full suite at the base is separate and still running; these results do not qualify native delivery, management surfaces, prose quality, every Mods criterion, or a full post-change integration run. Criteria/ledger updates remain coordinator-owned on main.
