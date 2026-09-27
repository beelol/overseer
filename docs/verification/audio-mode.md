# Audio Mode verification (pull request #5)

Criterion: [AC-143](AC-143.md), Gate O in [the RFC](../overseer-rfc.md). Design:
[side RFC](../rfcs/audio-mode.md). Tested implementation commit: `d7be0a3`; later commits on
the branch change documents, comments and evidence only. Date: 2026-09-26, macOS 26.6.2 arm64.

## Commands and results

| Command | Result | Log |
| --- | --- | --- |
| `CARGO_BUILD_JOBS=1 cargo test -p overseerd --offline -- --test-threads=1` | 77 passed, 0 failed (16 unit, 11 audio protocol, 50 protocol) | [log](evidence/audio-mode/cargo-test-overseerd.txt) |
| `npm run check --prefix extension` | pass | [log](evidence/audio-mode/extension-check.txt) |
| `git diff --check origin/main HEAD` | pass | same log |
| `python3 docs/verification/evidence/audio-mode/check-pack.py --approved <folder>` | pass, 46 checks | [log](evidence/audio-mode/pack-check.txt) |
| `node docs/verification/evidence/audio-mode/live-playback.js` | 11 of 11 | [log](evidence/audio-mode/live-playback.txt) |
| `node test/ui/scenario-audio.js` (packaged VSIX) | 10 of 10 | [folder](evidence/ui/audio/) |

GitHub runs no checks on this repository, so these local runs are the record.

## The Reactor pack

The twelve MP3s in `daemon/assets/reactor` are the owner-approved files, byte for byte.

| Cue | Selected version | Seconds | Bytes | SHA-256 (first 16) | Same as approved |
| --- | --- | --- | --- | --- | --- |
| `agent_queued` | Current · Command | 0.350 | 2,684 | `37fe68af5b1f3782` | yes |
| `agent_started` | Current · Code | 0.365 | 2,828 | `7f8416a67056f790` | yes |
| `agent_resumed` | Current · Code | 0.365 | 2,828 | `d76daca3fa30650f` | yes |
| `agent_progress` | Current · Code | 0.365 | 2,828 | `246dfd0720847562` | yes |
| `agent_complete` | Current · Code | 0.365 | 2,828 | `167d213ae6b976d9` | yes |
| `agent_stopped` | Split Code | 0.333 | 2,540 | `f3fabcc836a4b470` | yes |
| `agent_needs_attention` | Split Code | 0.333 | 2,540 | `d35944ae8f64d8fb` | yes |
| `agent_failed` | Split Code | 0.333 | 2,540 | `b3df4298da0afed2` | yes |
| `agent_unblocked` | Tight Code | 0.276 | 2,252 | `7b75354a71059ff7` | yes |
| `review_ready` | Split Code | 0.333 | 2,540 | `f5e5036c4fdcde89` | yes |
| `delivery_ready` | Split Code | 0.333 | 2,540 | `75a6e03d29ccf388` | yes |
| `verification_passed` | Split Code | 0.333 | 2,540 | `648b499f4fd40723` | yes |
| 12 cues | | longest 0.365 | 31,488 | | 12 of 12 |

The full hashes are in the [pack's README](../../daemon/assets/reactor/README.md) and
`manifest.json`. Seconds are the length of the decoded audio. The approved folder's own manifest
states the same sizes, lengths and hashes. Git tracks these 12 audio files and no other: no
WAV, no Commander recording, no game sample, no generated voice file.

The approved folder was read through the owner's own copy of it (`cp -RL`), because macOS
denies the agent's app access to `~/Downloads`.

## Requirement by requirement

| Requirement | Evidence | State |
| --- | --- | --- |
| Off until explicitly enabled; the setting survives a restart | `audio_mode_is_off_until_enabled_and_survives_restart`; live: still on after a daemon kill, nothing replayed; VS Code: *Reactor signals · Off* on a new install | Verified |
| Off mode starts no player and writes no cache | `disabled_audio_never_materializes_a_player_asset`; live: no player process, no `audio` folder | Verified |
| Start and completion play once with VS Code closed | `live_root_events_play_once_without_or_with_multiple_ui_clients` (no client attached); VS Code scenario after quitting VS Code | Verified |
| Several UI clients never repeat a cue | same test with two clients attached: one attention cue | Verified |
| Children, tools and progress stay silent | `child_completion_stays_silent_until_root_finishes`: three cues for the whole run; `core_transitions_are_broad_and_attention_is_deduped`: later turns, `permission` events and `running` make none | Verified with fixtures |
| A permission request and its waiting status play once | `live_permission_and_waiting_status_share_one_attention_cue` | Verified with fixture |
| A failed login needs the user once | `authentication_failure_makes_one_attention_cue` | Verified with fixture |
| Simultaneous needs play one cue; the UI shows the count | `simultaneous_permissions_make_one_cue_and_two_visible_needs`, `simultaneous_attention_is_coalesced_even_when_starts_interleave`; VS Code: one cue, 2 on *Needs you*, the Overseer icon and the status bar | Verified |
| Queues are bounded and attention is not displaced | `attention_can_queue_when_routine_cues_fill_their_lane`; live: 5 of 40 burst requests accepted, 35 refused | Verified |
| Attention history is bounded | `attention_history_stays_bounded_during_long_daemon_uptime` (1,100 failed runs, at most 1,024 kept) | Verified |
| One player at a time; memory level | live: never more than one player process; resident size 8,640 KB before the burst, 8,656 KB after | Verified live |
| Reactor bundles the approved pack | `manifest_describes_the_bundled_bytes`, `pack_has_twelve_short_original_cues`, pack check with `--approved`; live: `afplay` plays from an owner-only cache identical to the bundled files | Verified |
| A cached cue from another pack is replaced | `a_cached_cue_from_another_pack_is_replaced` | Verified |
| System voice uses an installed voice on the Mac | `installed_system_voice_can_be_selected`; live: `say -v Daniel "Agent started."`, an unknown voice refused | Verified live |
| Commander is played in place and never copied | `system_and_private_commander_tracks_are_selectable_without_bundling_voice_files`; live: `afplay` on the private path, no WAV under the daemon's folder or in the repository, private files unchanged | Verified live with generated beeps |
| Missing or failed playback fails quietly | `missing_local_cache_does_not_interrupt_agents`; live: Commander folder removed, the agent completes, the failure is logged | Verified |
| VS Code changes settings and asks for previews; only the daemon plays | VS Code scenario: turn on, choose a track and preview each reach the daemon; the extension has no playback code | Verified |
| Main's Gate K menus are kept | VS Code scenario: title bar unchanged, *Audio Mode and Reactor Cues…* in the overflow menu; `extension/package.json` differs from main by two lines | Verified |
| Other platforms report unavailable | `a_platform_without_players_reports_unavailable_and_stays_silent` (a test switch takes the players away on macOS) | Verified on macOS only |

## The owner's ledger

The seven checks the owner's cue ledger asks of this pull request:

| Check | Where |
| --- | --- |
| Start, finish and attention each play once for a top-level agent in a live run | live playback log; fixture agents, real players |
| Permission and login events do not double-play through their status update | the permission and authentication tests |
| Several simultaneous needs produce one cue and a visible count | the two-agent test and the VS Code scenario |
| With Audio Mode off the daemon starts no player | off-mode test and live check |
| Audio works after VS Code closes; a second client does not double-play | two-client test and the VS Code scenario |
| An unknown or missing sound pack fails quietly | blocked cache test, missing Commander folder live |
| Only original signal assets and synthetic fixtures are in the pull request | pack check: 12 tracked audio files, all in the pack |

## Not covered

- No platform other than macOS was run.
- The owner's Commander recordings were not read; the live check used three generated beeps
  in a temporary private folder.
- Nobody listened. The checks prove which files are played, not how they sound.
- The in-flight pull requests #2 (Auto Mode), #3 (Swarm) and #6 (terminal UI controls) were
  not rechecked against this branch in this pass.
