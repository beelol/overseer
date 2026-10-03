# Session assertion setup diagnosis

The first fresh read implementation attempt ran eleven tests: ten passed (six feature cases plus four helpers); the ordinary self case failed its final absolute-zero session assertion after its before/after authority snapshots matched. This did not establish a read mutation.

Temporary staged counts at runtime source `116c9cb8c5ebfe02dfdcd05c7003e69a6ded1f7b` placed session creation at the first ordinary run, before all native Mods reads. A supported global `agent.cadence: off` diagnostic still created the session; the ordinary run was completed with exit 0. That setting was removed.

A temporary session INSERT backtrace directly identified `handle_event(task_created, agent) -> started_card -> overseer_session`. The existing Started card legitimately creates the shared session regardless of check-in cadence. The saved stack and source/binary hashes record this diagnostic. Instrumentation was reverted before corrected verification; it is not production code.

Test-only correction `e78fb21` preserves actual completed ordinary starts, immutable last-turn privacy, full authority snapshot equality, and the unchanged existing session count. The unknown-token case separately proves a fresh-daemon unauthenticated refusal leaves zero sessions before/after. It does not claim an authenticated real-run zero-session state. No fabricated run rows, product default changes, paid turns or UI were used.
