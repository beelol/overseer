# AC229/230 closure audit — 2026-10-03

The coordinator read the literal main Verify clauses and assertions in frozen combined source `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. This maps evidence; it does not upgrade either criterion while full run39632 is active.

## AC229

- Vocabulary unit: `voice/src/recognize.rs::the_prompt_includes_the_vocabulary_and_the_names` checks repo, agent, merge, worktree and Overseer plus dynamic names. The recognizer passes that prompt to Whisper. The voice crate is a workspace member, so the full Rust command includes this unit; earlier selected daemon-only groups did not.
- Dynamic names: `daemon/tests/voice.rs::ac229_the_recognizer_expects_the_agents_and_the_repositories` creates named agents/repos and inspects the listener hint.
- Read-back/correction: `ac229_a_start_is_read_back_and_a_correction_inside_the_window_changes_it` asserts exact initial read-back, a window long enough to speak it, spoken correction after1.5seconds, corrected destination/prompt and exactly one new agent. Separate multiple-start/redirect tests retain every task in a confirmed bundle.
- Named request retention: `ac229_a_request_naming_overseer_with_an_instruction_is_never_dropped` uses the literal punctuation-heavy request, verifies read-back and task delivery, inspects the one retry after the fixture says not-for-me, and asserts no dropped terminal state.

## AC230

- Typed modes and Auto start: `daemon/tests/overseer_modes.rs::ac230_typed_conversations_set_each_mode_and_start_one_in_auto` checks each actual running-agent mode and launch arguments. Reviewed PR61 makes the existing public Stop pause explicit, asserts queued/no-new-turn state, then uses owner resume before the unchanged echo assertions. It does not weaken the mode requirement.
- Spoken modes/start/unasked permission: `daemon/tests/voice.rs::ac230_by_voice_modes_a_start_in_auto_and_a_permission_read_out_unasked` checks all named modes, live change evidence and voice cause; Auto remains waiting before explicit yes, then launch argv names auto. A new native fixture permission is announced without an owner request, appears as a needs card and the literal allow-it utterance answers it.
- Pending at every level/reason/refusal: `ac230_overseer_sets_auto_by_itself_only_where_allowed_and_says_why` rejects unallowed repos and missing reasons, checks an allowed suggestion stays open beyond its settle window at Auto, then verifies explicit yes records the exact reason and cause in event/card. Auto/Steer/Ask-first refusals leave mode-event count unchanged. Owner/voice mode and start proposals also remain open at all levels until answered; caller-supplied Steer class cannot bypass confirmation.
- Unasked view/click: `ac230_a_waiting_permission_comes_up_by_itself_and_a_click_answers_it` asserts the exact yes/no card and one Needs item with no owner message, answers via the VS Code surface and checks native completion/one card. Permission-card and voice-target regressions separately protect No, request identity, competing answers and queued speech target handling.

## Remaining gate

Focused reviewed PR52/61 logs exist; prior source63 packaged Voice/Talk checks passed, but its Rust stage failed before every required target completed. Require the final combined workspace/vocabulary and fresh package results, preserve their exact source/evidence, then update records on main and integrate through the merge gate. No real-room, microphone, paid model, Whistle quality or production claim follows from these fixture-specific criteria.
