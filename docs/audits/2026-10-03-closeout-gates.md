# Remaining merge and security evidence

Read-only reconciliation on 2026-10-03 against PR52 baseline 715ee85b and current criteria. This is a plan, not a passing record.

## AC-200

The existing builder review at docs/verification/evidence/ac-200/review.md is not an independent review. PR54 independently reviewed the redaction correction only; do not relabel that as a complete Gate S review.

Existing ac200_what_agents_say_is_data covers report attribution, no unsolicited report turn, forged tokens, watcher findings at all three levels, refusal of a Confirm action from a finding, and the daily self-start cap. It does not establish the entire criterion by itself. Its report/file case is outside the three-level loop. The file is written by the fixture, but this test does not visibly demonstrate that Overseer reads the malicious file. Review the traffic test and fixture paths before claiming that vector is exercised.

Before closing: independently review token/role lookup, proposal/action classes, scope/caps, watcher restrictions, file boundary and shares; exercise each untrusted source through actual consumption; assert Auto redirects retain the recorded file snapshot; collect packaged-flow traffic with planted synthetic credentials. Existing redaction regression evidence is at PR54 c22da6fc. Findings and resolutions must be retained, including any still-open limitations.

Source checks so far: token_holder resolves hashed token to stored run/role, tool availability is checked against that role, and watcher subject reads compare the argument ID to the daemon's watch. Proposal Confirm gating uses the authoritative action table as well as daemon escalations, so a caller-supplied lower class does not bypass it. Phone capability classes come from protocol/protocol.json, separately from the action table; do not confuse the two.

The traffic test at daemon/tests/overseer.rs:2496 explicitly collects file/diff results, prompts, cards, shares and other boundaries with four planted token strings. It catches tool errors into the traffic list as well as successes; a future stricter gate should positively prove the intended successful file read, not accept an error as credential-free coverage. The file-injection consumption gap above is separate from credential redaction coverage.

File reads need a focused boundary review: inside_worktree canonicalizes then file_text opens the original path, so a concurrent symlink swap is not ruled out by the static path test. file_text also checks its 4 MiB bound only after reading the full file. These are source-level review candidates requiring deterministic fixtures before any finding is called reproduced or fixed.

## AC-201

The current one-command run is from a fresh worktree, not yet a clean-clone completion claim. Final combined integration should use an actual clean clone of the finished tested heads plus current main, with the two pinned extension tooling dependency installs completed before the run. Preserve its exact commit, dependency setup, environment and log.

Historical on/off evidence dates to d09978a in September. It checks protocol on/off and selected packaged scenarios on/default; it must not be described as fresh evidence of every combination. Recheck protocol and affected Gate S scenarios with channel/briefings off and on and check-ins off and on; use a bounded explicit matrix. No paid harness.

## AC-204 and AC-146

The combined PR52/53/54 merge can supply the first current slice record only after all three builders are done, independent reviews accepted, current main integrated, and the full gate passes. Retain individual merge commits, final tested head, conflict resolutions, agent completion evidence, and the exact remaining criteria/tracker rows. PR55 library checkpoint is not yet the completed usable delivery slice and remains excluded.

## AC-210 and AC-213

Actual missing implementation, not a stale dependency alone: scripts/dev phone is a stub; gateway advertise always uses the standard service when mDNS is enabled; GatewayHello has no instance. Default dev mDNS remains off. The phone-isolation plan must enforce release/exact-dev policy before consuming pairing codes or inserting devices, and prove it with two isolated lab daemons, never the installed daemon. Queue agent is preparing the plan.
