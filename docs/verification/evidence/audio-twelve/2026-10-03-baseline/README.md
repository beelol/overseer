# Twelve-line audio baseline: 2 pass, 15 feature failures

Tested source `038c2c41a70a7daedb92c9873b4b1a22bfc9ad9e` normally merges the two authored fixture branches. Production sources are unchanged from integration `79a334cd0a1b334cae80e4654ceafa244f8677e8`. The final semantic fixture revision `d36835382bf7e421c974f522549e5ea11a05ff59` pins actual start playback before releasing terminal transitions; fast permission/auth transitions do not demand obsolete queued start cues.

Root session9076 is terminal0 as an evidence-capture wrapper: **2/17 cases pass;15/17 return101 at intended feature assertions. This is a failing product baseline, not a green suite.** No setup/compiler failures, timeouts, output truncation or unexpected process signals occurred. Package-only clean5.310s, standalone build22.876s and two-target no-run23.994s succeeded. All three required executables are fresh:false with matching clone paths, dependency files and SHA256 in [receipt.json](receipt.json); exact Cargo messages are in [compiler-artifacts.json](compiler-artifacts.json). Hashes were unchanged after the cases. Owned leftovers0, other orphan leftovers0. Live processes belonging to another active test run are deliberately excluded by the scoped detector, not described as absent.

## What was reached

The pack inventory is missing its lines array, six folder cases stop at the missing `audio.source.set` method, and legacy migration lacks its source projection. Later rollback/path/preview/restart assertions in those six cases are **unreached**, not independently demonstrated failures.

The semantic fixtures capture generic `agent_needs_attention` for terminal failure, permission, sign-in, lost supervisor and plural attention. Ready Swarm start captures three constituent `agent_started` cues instead of one `swarm_initiated`. The whole-Swarm completion fixture stops at that start assertion, so its completion assertions remain unreached. Ordinary success and explicit owner Stop already pass and remain regression controls. Logs use the incumbent synthetic key sink: they do not prove selected-pack file playback, canonical spoken content, native browser routing or all AC275–288 trigger families.

## Invocation and allocation

AGENTS.md permits focused Cargo tests without the full-suite lock. Root explicitly allocated one low-priority worker on retired `/private/tmp/overseer-closeout-verify-target` after the full run's Rust stage passed1692/0; full47476 continued UI on the different integration target. No full-test lock was taken, removed or bypassed; no additional UI, real audio player, native synthesis, provider or private clip was used. Agents remained source-only. The earlier all-runtime reservation was deliberately narrowed for this bounded headless run.

Commands used CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1 and nice20: `cargo clean -p overseerd`; `cargo build -p overseerd --bin overseerd --message-format=json`; `cargo test -p overseerd --test audio_packs --test audio_semantics --no-run --message-format=json`. Each listed test ran once through its fresh executable with `--exact <name> --nocapture --test-threads=1`; inventories were checked before execution. Per-case stdout/stderr and [observed priority](priority.txt) accompany the receipt. A bounded capture controller enforced deadlines/output limits and owned-group reap; cleanup used the repository process detector with pre-run PID baseline and this run's marker.

| Target | Exact test | Result |
| --- | --- | --- |
| audio_packs | `public_inventory_is_the_exact_twelve_phrases_and_old_keys_are_not_previews` | feature FAIL |
| audio_packs | `complete_folder_selection_stays_off_private_in_place_and_survives_restart` | feature FAIL |
| audio_packs | `incomplete_unknown_and_wrong_schema_manifests_leave_the_prior_selection_unchanged` | feature FAIL |
| audio_packs | `unsafe_manifest_paths_and_non_audio_files_are_refused_without_private_path_errors` | feature FAIL |
| audio_packs | `a_stale_source_revision_cannot_replace_another_clients_selected_folder` | feature FAIL |
| audio_packs | `removing_the_selected_folder_shows_unavailable_without_switching_to_builtin` | feature FAIL |
| audio_packs | `all_twelve_explicit_previews_use_the_folder_while_audio_stays_off` | feature FAIL |
| audio_packs | `legacy_private_selection_migrates_unavailable_without_enabling_or_falling_back` | feature FAIL |
| audio_semantics | `terminal_task_failure_has_its_specific_line_without_generic_attention` | feature FAIL |
| audio_semantics | `ordinary_success_has_one_start_and_one_logical_completion` | PASS |
| audio_semantics | `legacy_permission_and_waiting_projection_announce_the_same_specific_need_once` | feature FAIL |
| audio_semantics | `expired_auth_without_a_permitted_fallback_announces_sign_in_not_failure` | feature FAIL |
| audio_semantics | `unrecoverable_live_supervisor_loss_is_unexpected_stop_not_generic_attention` | feature FAIL |
| audio_semantics | `owner_stop_is_silent_after_the_actual_start` | PASS |
| audio_semantics | `two_distinct_live_permission_needs_use_plural_instead_of_individual_lines` | feature FAIL |
| audio_semantics | `accepted_live_swarm_director_and_workers_have_one_swarm_start` | feature FAIL |
| audio_semantics | `whole_swarm_completion_replaces_director_and_worker_completions` | feature FAIL |

No acceptance criterion is closed by this baseline. The full baseline source is frozen; implementation proceeds separately with all unreached and additional Verify obligations retained.
