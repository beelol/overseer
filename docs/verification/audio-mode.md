# Audio Mode verification (draft PR #5)

The source pack is `daemon/assets/reactor`: twelve original synthesized MP3 files, 34,224 bytes in total, each under 0.5 seconds. The separate Machines voice lab and its private Commander WAVs are outside this repository. The daemon only stores the path to a user-selected private pack and plays it in place.

| Requirement | Evidence | State |
|---|---|---|
| Off until explicitly enabled; setting survives restart | `audio_mode_is_off_until_enabled_and_survives_restart` | Verified |
| Off mode starts no player or cache | `disabled_audio_never_materializes_a_player_asset` | Verified |
| Root start and completion sound once while VS Code is closed | `live_root_events_play_once_without_or_with_multiple_ui_clients` | Verified with fixture |
| A second UI client does not duplicate playback | Same two-client protocol test | Verified with fixture |
| Child and grandchild activity stays silent | `child_completion_stays_silent_until_root_finishes` runs a nested Codex fixture: the child completes before the waiting root, but only the root start, attention, and completion cues play | Verified with fixture |
| Permission event and waiting status share one attention cue | `live_permission_and_waiting_status_share_one_attention_cue` | Verified with fixture |
| Auth failure needs attention once | `authentication_failure_makes_one_attention_cue` | Verified with fixture |
| Simultaneous needs coalesce, including interleaved starts | `simultaneous_attention_is_coalesced_even_when_starts_interleave` and `simultaneous_permissions_make_one_cue_and_two_visible_needs` | Verified with two live fixture agents |
| Attention remains queueable after routine bursts; memory stays bounded | `attention_can_queue_when_routine_cues_fill_their_lane`, `attention_history_stays_bounded_during_long_daemon_uptime` | Verified in queue tests |
| Missing local cache does not interrupt agents | `missing_local_cache_does_not_interrupt_agents` | Verified |
| System speech and private Commander import | `system_and_private_commander_tracks_are_selectable_without_bundling_voice_files`, `installed_system_voice_can_be_selected`; isolated macOS playback smoke for both | Verified locally |
| Visible count for multiple needs | Two-agent daemon fixture confirms both waiting roots remain in state. In an isolated VS Code Extension Development Host running this draft extension against a two-waiting-root fixture, the accessible status bar displayed `Overseer 2 active · 2 need you`. | Verified locally |
| Auto Mode, Swarm, and TUI changes | Non-checkout merges with #2 and #3 are clean. All 9 audio protocol tests pass in a temporary tree combined with #3 head `62f0849`; later changes through `fac4ffa` merge cleanly and leave the audio event contract unchanged. On the current #4 head, all 9 audio protocol tests pass and `cargo check -p overseer-tui --offline` passes in a temporary combined tree. #4 still has a README-only conflict from its older base. | Recheck when those draft PRs change |
| TUI control and duplicate-sound behavior | Stacked draft PR #6 adds TUI Audio Mode opt-in, track, installed voice, private folder import, and preview controls. The TUI queries `audio.get` before ringing its own bell for a newly waiting root. Three focused tests and the existing Help/navigation test passed. A temporary #4 + #5 + #6 live fixture test confirmed TUI opt-in and preview reached the daemon, and one waiting synthetic agent produced one Reactor attention cue without a TUI bell. | Verified in temporary combined tree; recheck after #4 and #5 merge |

The daemon tests use a disposable log sink instead of starting a player. A separate isolated macOS smoke test exercised `afplay` for Reactor and Commander, and `say` with an installed voice. The extension's `npm test` command currently references a missing `extension/test/run.js`; `npm run check` verifies its JavaScript syntax.
