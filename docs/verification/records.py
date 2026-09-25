#!/usr/bin/env python3
"""Generates docs/verification/AC-NN.md evidence records (except AC-01/AC-03, which are
hand-written) from the structured entries below. Run from the repository root:

    python3 docs/verification/records.py <tested-commit>

Each entry follows the template in docs/verification/README.md. Keep statuses honest:
only 'verified' criteria get checked in the RFC."""
import sys, pathlib

COMMIT = sys.argv[1] if len(sys.argv) > 1 else "UNSET"
CODEX_COMMIT = "e51c70e"
ENV = "macOS 26.6.2 (25G83) arm64; VS Code 1.139.0 (stable, isolated --user-data-dir/--extensions-dir, VSIX installed with `code --install-extension`); Rust 1.89.0; Node 24.20.0; Git 2.50.1 (Apple)"
HARN = "codex-cli 0.155.0-alpha.16.4 (ChatGPT.app bundle); Claude Code 2.1.246; OpenCode 1.15.13; overseerd 0.1.0 (protocol 1, parser 2026-09-24.1)"
UI = "evidence/ui"
T = "`cargo test` (daemon/tests/protocol.rs, real daemon binary + real Git fixtures)"

R = {}

def rec(n, title, status, **kw):
    R[n] = dict(title=title, status=status, **kw)

rec(2, "Account and child feasibility spikes", "verified (research criterion with investigated blockers)",
    commit=CODEX_COMMIT, harness="Codex: owner's existing ChatGPT login (plan `pro`, account fingerprint `2e1fa921…`, see note); Claude Code: default config dir (OAuth expired); OpenCode: isolated profile with the local mock provider",
    fixture="Disposable repos under the session scratchpad and /tmp; tiny prompts only",
    steps="""1. Codex: `codex exec --json -m gpt-5.6-luna -s workspace-write "Spawn exactly one sub-agent … create hello.txt …"` (clean env) → [transcript](../../fixtures/transcripts/codex-0.155-exec-subagent-live.jsonl). Then the 4-turn Overseer UI session [codex-live](evidence/ui/codex-live/).
2. Claude Code: `claude -p --model haiku --output-format stream-json --input-format stream-json --verbose` with one Task-delegation prompt, clean env → [transcript](../../fixtures/transcripts/claude-2.1.246-auth-expired-live.jsonl).
3. OpenCode: `opencode run --format json -m mock/mock-coder "please delegate twice"` with isolated XDG dirs and the mock provider; read `$XDG_DATA_HOME/opencode/opencode.db` `session.parent_id`.
4. Two Codex subscriptions: create two isolated profiles (Accounts → Add Account Profile) and check `profile.status`.""",
    expected="Two isolated Codex subscription sessions and native child capture for Codex, Claude Code and OpenCode — or the exact blocking behaviour and the next alternative.",
    actual="""- **Codex native child: captured live.** `collab_tool_call` `spawn_agent` gives the child thread id in `receiver_thread_ids`; `wait` gives `agents_states[<id>].status/message`. Overseer attached the child to the parent with `exact` confidence in both the CLI probe and the UI session.
- **OpenCode native child and grandchild: captured** through the real OpenCode 1.15.13 runtime with mock model responses. The root stream reports the child session id in `task` tool `state.metadata.sessionId`; grandchildren only appear in the session store (`session.parent_id`), which Overseer reads read-only. Requires `agent.general.permission.task=allow` for a subagent to delegate.
- **Claude Code: blocked.** Exact behaviour: `system/init` then `assistant` with `"error":"authentication_failed"`, text "Failed to authenticate: OAuth session expired and could not be refreshed", `result.is_error=true`; `claude auth status` → `{"loggedIn": false, "authMethod": "none"}`. Next alternative: owner signs in (`claude auth login`, or Overseer Accounts → Add Account Profile (claude) → Sign In); then rerun the live probe and AC-14/16/19 checks. Claude 2.1.246 documents nested subagents (spawn depth up to 3) via `parent_tool_use_id` / `system/task_*`, implemented and fixture-tested.
- **Two isolated Codex subscriptions: blocked.** A new isolated profile reports `Not logged in`; signing it in needs the owner's browser/device login for the second OpenAI account. Observed: the shared `~/.codex/auth.json` changed from one account (plan `plus`, account id prefix `f8aa9a`) to another (plan `pro`, `6dbb93`) around 21:46 local time, most likely by the ChatGPT desktop app that shares `~/.codex`. Copying tokens between homes was rejected: refresh-token rotation could invalidate the owner's login. Next: owner signs in two isolated Overseer profiles (A/B) and runs `node test/ui/scenario-codex-live.js`-style concurrent tasks (AC-12).""",
    evidence="[codex transcript](../../fixtures/transcripts/codex-0.155-exec-subagent-live.jsonl), [claude transcript](../../fixtures/transcripts/claude-2.1.246-auth-expired-live.jsonl), [opencode transcripts](../../fixtures/transcripts/), [codex-live UI evidence](evidence/ui/codex-live/), protocol test `ac19_fixture_codex_live_transcript_children`",
    live="Codex live (one account). OpenCode: real runtime, mock model. Claude: live failure transcript only.",
    limits="This research criterion does not pass AC-12, AC-14 or AC-19.",
    blocker="Claude re-login and a second signed-in ChatGPT profile are owner actions (README follow-ups).")

rec(4, "Installable macOS foundation with portable design", "verified",
    fixture="Fresh `git clone` of the published branch into a temporary directory",
    steps="""1. `git clone --branch <branch> https://github.com/beelol/overseer.git /tmp/ovs-clean && cd /tmp/ovs-clean`
2. `npm ci --prefix extension/branch-diff/tooling/review --ignore-scripts && npm ci --prefix extension/tooling/vsce --ignore-scripts`
3. `node extension/scripts/package.js` (release daemon + VSIX)
4. `OVERSEER_HOME=<tmp> target/release/overseerd serve &` then `overseerd ctl hello`, `ctl harness.list`, `ctl daemon.shutdown`
5. `cargo test` in the clean clone.""",
    expected="Clean build of daemon and extension, daemon smoke run with logs; platform boundaries isolated for Linux.",
    actual="See [clean-build.log](evidence/ac-04/clean-build.log). Portable boundaries: all state paths in `daemon/src/paths.rs` (macOS `~/Library/Application Support/Overseer`, Linux `$XDG_DATA_HOME/overseer`, `$XDG_RUNTIME_DIR` for the socket), peer credentials behind `peer_uid_fd` (`getpeereid` on macOS/BSD, `SO_PEERCRED` on Linux), process groups/signals via POSIX `libc`, no macOS-only APIs in the daemon; the extension resolves `bin/overseerd-<platform>-<arch>`.",
    evidence="[evidence/ac-04/clean-build.log](evidence/ac-04/clean-build.log)",
    live="Build/smoke only.", limits="Linux build/run not attempted (AC-41). The `code` CLI, Keychain-backed harness logins and the bundled binary name are the only macOS-specific pieces.")

rec(5, "Independent durable state", "verified",
    steps=f"""1. {T}: `ac05_ac07_state_survives_daemon_crash_and_reattaches` — start a generic run, SIGKILL the daemon mid-run, restart, compare `state.tasks`, run count, event history and process generation.
2. `ac18_fixture_recursive_tree_duplicates_and_delayed_parent` — parent relationships identical after restart.
3. Packaged UI: [codex-live](evidence/ui/codex-live/) closes VS Code (Cmd+Q) during a live run, reopens, and the tree/output are rebuilt from the daemon; [main](evidence/ui/main/) kills the daemon while the UI is open and verifies reconnect with identical task ids; [review](evidence/ui/review/) reloads the window.""",
    expected="Tasks, runs, parents, workspaces and events survive extension close/reload and daemon restart without duplicate tasks or silent replacement work.",
    actual="All listed checks pass: identical task JSON, one run, `process_generation` stays 1, no duplicated output lines, output produced while the daemon was down is recovered; UI shows the same tasks after reconnect and after reopening.",
    evidence="protocol tests above; `evidence/ui/codex-live/06-reopened.png`, `evidence/ui/main/result.json` (check 'UI survives daemon crash'), `evidence/ui/review/result.json`",
    live="Codex live for UI closure; fixtures for crash/restart.")

rec(6, "Honest process lifecycle", "verified",
    steps=f"""{T}: `ac06_lifecycle_states_follow_real_signals` (exit 0 → completed; exit 3 → failed "exit code 3"; interrupt → interrupted; external SIGKILL of the harness → failed "killed by signal 9 (not requested by Overseer)"; SIGKILL of the supervisor → disconnected), `ac06_structured_harness_silence_is_not_completion` (exit 0 without a turn-completion event → `unknown`, children `unknown`), `ac16_fixture_permission_*` (waiting_for_user), `ac07_lost_session_is_reported_not_running`. Packaged UI: [main](evidence/ui/main/) daemon SIGKILL → status bar "disconnected" → daemon restarted and UI reconnected.""",
    expected="States come from actual signals with exit reasons; silence is not completion; connection loss/recovery is visible.",
    actual="All pass. Queued/starting/running/waiting_for_user/completed/failed/interrupted/disconnected/unknown are produced only from supervisor, exit and harness events.",
    evidence="protocol tests; `evidence/ui/main/` (reconnected screenshot)", live="Fixtures; Codex live completed/interrupted states in codex-live.")

rec(7, "Persistent sessions", "verified",
    commit=f"{CODEX_COMMIT} (live closure), final commit for crash tests",
    harness="Codex, owner's existing ChatGPT login (turn 3: `sleep 25`, minimal tokens)",
    steps=f"""1. [codex-live](evidence/ui/codex-live/): during turn 3 the scenario quits VS Code with Cmd+Q, verifies the run is still `running`, waits until it completes, relaunches VS Code and finds the completed run and its output in the tree/output panel.
2. {T}: `ac05_ac07_state_survives_daemon_crash_and_reattaches` (waiting fixture, forced daemon SIGKILL, reattach, no duplicate launch), `ac07_lost_session_is_reported_not_running` (daemon, supervisor and harness killed → `disconnected` with "lost session", not running).
3. The 10-minute load scenario keeps four fixture runs alive (AC-35).""",
    expected="Closing VS Code leaves agents running; daemon recovery reattaches or reports lost sessions truthfully.",
    actual="Live Codex turn continued and completed while VS Code was closed; the reopened UI showed it. Crash tests reattach without relaunch; lost sessions are reported as disconnected.",
    evidence="`evidence/ui/codex-live/05-turn3-before-close.png`, `06-reopened.png`, `result.json`; protocol tests", live="Live Codex + fixtures.")

rec(8, "Local access boundary", "blocked",
    steps=f"""1. {T}: `ac08_socket_is_owner_only_and_requests_are_not_shell` — socket mode 0600, socket directory 0700; malformed JSON → `parse_error`; non-object params → `invalid_params`; non-string method → `invalid_request`; >1 MiB request → `request_too_large`; shell fragments in `repo`/`path` rejected; argv with `$(touch …)`/`; rm -rf /` passed literally (no file created); unknown methods rejected.
2. Every accepted connection is checked with `getpeereid` against the daemon's uid (`daemon/src/server.rs`); shim control sockets do the same.
3. Packaged UI [trust](evidence/ui/trust/): an untrusted window (VS Code Restricted Mode via `security.workspace.trust.emptyWindow=false`) — the trust editor shows "You are in Restricted Mode", **Overseer: New Task** is not offered in the command palette, the Agents view explains that launching requires trust, and no task exists afterwards.""",
    expected="Only the local user can command the daemon; untrusted workspaces cannot launch; malformed requests cannot execute shell fragments.",
    actual="Untrusted-workspace, malformed-request and shell-injection checks pass; socket/directory modes are owner-only. **Not verified:** an actual connection attempt from a different local user being rejected (no second macOS account is available to the agent), so the access-rejection part of the Verify clause is unproven.",
    evidence="protocol test; `evidence/ui/trust/`",
    live="Real VS Code workspace trust.",
    blocker="Needs a second local macOS user (owner creates a standard test account). Next: as that user, `nc -U <socket>` / `overseerd ctl hello` with the owner's `OVERSEER_HOME` must fail with a permission error, and a relaxed-permission socket must still be refused by the peer-uid check (log line `rejected connection from uid …`).",
    limits="Rejection of a different local uid is enforced by filesystem permissions plus the peer-uid check but was not exercised with a second macOS user account. On this machine the VS Code trust list trusts `/` for folders, so the untrusted case uses an empty window; folder-level Restricted Mode uses the same `isWorkspaceTrusted` gating.")

rec(9, "Complete task controls", "verified",
    commit=f"{CODEX_COMMIT} (live), final commit for UI/protocol checks",
    harness="Codex, owner's existing ChatGPT login",
    steps=f"""1. [codex-live](evidence/ui/codex-live/): from the packaged UI, **Overseer: New Task** → repository → harness `codex` → account `codex (existing login)` → New worktree → start ref `HEAD` → model → prompt; output panel streams; **Send follow-up** (turn 2, turn 3 after reopening); **Interrupt** (turn 4). Follow-ups resume the same native thread (`codex exec resume`).
2. [main](evidence/ui/main/): same flow with OpenCode (mock), plus a native grandchild selected in the tree shows Interrupt/Send disabled with "controlled through their parent run" / "Follow-ups go to the top-level run".
3. {T}: `ac09_follow_up_reaches_only_the_selected_run`.""",
    expected="An actual harness completes create/choose/output/follow-up/interrupt/resume; unsupported controls are disabled with an explanation; messages reach only the selected run.",
    actual="All pass; four live Codex turns with fresh snapshots each.", evidence="`evidence/ui/codex-live/`, `evidence/ui/main/`, protocol test", live="Codex live; OpenCode mock.")

rec(10, "Event replay and bounded output", "verified",
    steps=f"""{T}: `ac10_replay_cursor_burst_and_retention` (12,000-line output burst, retention prunes to 5,000 events per run and inserts a `retention` marker; resubscribe from a mid-stream cursor returns exactly the retained later events with no duplicates; raw output reports `truncated`), `ac10_redaction_of_secrets_in_output` (API-key and `access_token` patterns redacted in events and raw output), `ac18_fixture_recursive_tree_duplicates_and_delayed_parent` (duplicate and out-of-order child events, no duplicate nodes after restart). Raw segments beyond four 8 MiB files are deleted with a `retention` event. The extension drops replayed events at or below its cursor and resubscribes from the cursor after reconnect/lag.""",
    expected="Cursor-based replay without duplicates or gaps; bounded, redacted, inspectable output with explicit truncation markers.",
    actual="All pass.", evidence="protocol tests; load scenario retention samples (AC-35)", live="Fixtures.")

rec(11, "Account profiles", "blocked",
    steps="Accounts view: Add Account Profile (codex/claude/opencode), rename, Sign In (runs the harness's own login in a terminal with the profile's credential home), Sign Out (isolated profiles only), status refresh; New Task picks a profile and warns when it is not signed in. Protocol test `ac13_isolated_profiles_have_separate_homes_and_no_keys`. Packaged UI: profile created and selected in [main](evidence/ui/main/).",
    expected="Login and reauthentication through the UI for each supported account path, including a missing/expired login.",
    actual="Add/name/select work in the packaged UI; the expired Claude login is shown as *not signed in* and launching surfaces the `auth` error class. **Not verified:** completing a sign-in or reauthentication — that requires the owner's browser/device login.",
    evidence="`evidence/ui/main/` (profile created), protocol test", live="None for login.",
    blocker="Owner must perform the sign-in flows (Codex profile A/B, Claude). Next: run Accounts → Add Account Profile → Sign In for each, then record status/identity fingerprints and a reauthentication after `Sign Out`.")

rec(12, "Two simultaneous ChatGPT subscriptions", "blocked",
    expected="Two distinct paid ChatGPT profiles run Codex tasks concurrently in separate worktrees with overlapping timestamps and distinct identities.",
    actual="Not run. Only one ChatGPT login is available to the agent (the shared `~/.codex`); a second isolated profile is not signed in. Concurrent separate-worktree execution itself is covered by AC-21 fixtures and the four concurrent runs in AC-35.",
    evidence="AC-02 record", live="None.",
    blocker="Owner signs in two isolated Codex profiles in Overseer (Accounts → Add Account Profile → Sign In, one per OpenAI account). Next: launch two tiny tasks concurrently and record `profile.status` identity fingerprints, overlapping run timestamps and both edits.")

rec(13, "Credential isolation on macOS", "blocked",
    steps="Implemented: per-profile `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `XDG_*` homes (0700), no credential values stored by Overseer (only one-way fingerprints of account ids), system logins never logged out. Protocol test `ac13_isolated_profiles_have_separate_homes_and_no_keys`.",
    expected="Live A/B logout/login during B's work, restart both, refresh/expiry fault tests, no leakage.",
    actual="Not run: needs two signed-in dedicated test profiles. Observation relevant to isolation: the shared `~/.codex` login switched accounts underneath during the session (see AC-02), which is exactly why isolated profiles are the default recommendation.",
    evidence="protocol test; AC-02", live="None.",
    blocker="Owner-provided dedicated test logins (A and B). Next: A logout/login while B runs a `sleep` turn; restart; compare fingerprints; inspect SQLite/events for token leakage (`grep` for token patterns).")

rec(14, "Initial adapters", "blocked",
    commit=f"{CODEX_COMMIT} (Codex live), final commit (OpenCode mock, Claude fixtures)",
    harness="Codex: owner's ChatGPT login (live). OpenCode 1.15.13 with the deterministic mock provider (mock model responses; no OpenCode account). Claude Code 2.1.246: synthetic fixtures only.",
    steps="Codex: [codex-live](evidence/ui/codex-live/) (edit, follow-up, interrupt, streaming, capabilities). OpenCode: [main](evidence/ui/main/) (8-edit run, follow-up, interrupt through the real `opencode run --format json` adapter) and protocol/store tests. Claude: protocol fixture tests `ac16_*`, `ac18_*`, `ac20_*`.",
    expected="Codex and Claude Code live account-authenticated edit/follow-up/interrupt; OpenCode integrated (mock/local allowed).",
    actual="Codex ✅ live (`exec` and the app-server transport). OpenCode ✅ with the mock model (edit/follow-up/interrupt through the UI) and additionally with real local models via Ollama: `qwen3-coder:30b` wrote the requested file through the adapter with reported file activity, while `qwen2.5-coder:14b` printed its tool call as plain text and edited nothing (a model/tool-calling limitation, recorded as such). No OpenCode account authentication is claimed. Claude Code ❌ live not possible: OAuth session expired on this machine.",
    evidence="see above; [evidence/ac-14/opencode-ollama.log](evidence/ac-14/opencode-ollama.log)", live="Codex live; OpenCode mock + local Ollama models; Claude fixture.",
    blocker="Claude Code login (owner). Next: after `claude auth login`, run a tiny stream-json task through Overseer: edit, follow-up, interrupt.")

rec(15, "Generic harness fallback", "verified",
    steps=f"{T}: `ac15_generic_harness_paths_with_spaces_failures_and_unknown_capabilities` (script at `my tools/fake agent.sh`, args `two words`/`x y z`, stdin prompt, exit 7 → failed, missing binary → failed 'could not start', trap/interrupt → interrupted, capabilities children/usage/quota/approvals = unknown). UI: generic runs show 'Native children: unknown' and the Follow limitation note ([review](evidence/ui/review/)).",
    expected="Configured executable with cwd/profile env and interactive control, truthful unknown capabilities.",
    actual="All pass.", evidence="protocol test; `evidence/ui/review/`", live="Fixture executables.")

rec(16, "Permissions and limits", "verified",
    commit="7036cd6 (live approvals), final commit (fixtures)",
    harness="Codex 0.155 through the app-server transport (`codex-app`), owner's existing ChatGPT login (plan `pro`), model gpt-5.6-luna, approval policy `untrusted`; 3 tiny turns (~17k tokens each)",
    steps=f"""1. Packaged UI [codex-approval-live](evidence/ui/codex-approval-live/) (`node test/ui/scenario-codex-approval.js`): task 1 created from the command palette (harness `codex-app`, policy `untrusted`, prompt asking to `touch approved.txt`). The run waits in `waiting_for_user` (checked again after a delay: not auto-approved), a VS Code notification appears, and the run panel shows the command with **Allow once** / **Deny**. Allow → Codex runs the command, `approved.txt` exists, run completes. Task 2 → **Deny** → Codex reports `Rejected("rejected by user")`, replies "declined", no file. Task 3 → **Interrupt** while waiting → run `interrupted`, no file.
2. Earlier live run (same build family): a pending live approval survived a daemon restart (reattached) and was then answered through the daemon API; the command ran ([AC-07](AC-07.md) related).
3. Error classes with actual formats: the live Claude Code transcript "Failed to authenticate: OAuth session expired…" → `auth`; strings from the pinned codex 0.155 binary "Usage limit reached", "You've reached your workspace credit limit", "Your workspace is out of credits…" → `quota`, "exceeded retry limit, last status: 429 Too Many Requests" → `rate_limit` (unit test `classify_errors`); live `account/rateLimits/updated` credit/limit state is recorded as usage. Fixtures `ac16_fixture_*` cover Claude's stdio permission protocol and the app-server protocol.
4. No switch to an API key or another account: harness env is allow-listed (API keys stripped, `forbidden_env`), profile status treats API-key logins as not signed in, and runs keep their profile.""",
    expected="Native permission requests actionable in UI; sign-in failures, rate limits and quota distinguishable; allow/deny/interrupt a waiting run; actual error formats; no silent approval or API-key/account switch.",
    actual="All pass. Claude Code's permission path (stdio) is implemented and fixture-tested only, because its login is expired (tracked in AC-14/AC-19).",
    evidence="`evidence/ui/codex-approval-live/` (screenshots `permission-request`, `allowed`, `deny`, `interrupt`; result.json with usage and identity fingerprint), unit/protocol tests",
    live="Codex live (app-server). Claude fixture only.",
    limits="Rate-limit/quota states were not provoked live (that would require exhausting a paid account); their actual message formats come from the live Claude transcript and the pinned Codex binary.")

rec(17, "Compatibility truthfulness", "verified",
    steps="Compared [docs/compatibility.md](../compatibility.md) and the UI capability strings (`daemon/src/adapters.rs` `capabilities`, shown by **Overseer: Show Harness Capabilities** and in each output panel) against the evidence records.",
    expected="Support per harness, account path, version and capability, including Gemini/Devin outcomes and mock labelling.",
    actual="Matrix and UI state live (Codex), mock (OpenCode), fixture-only (Claude), unknown (generic), not installed (Gemini), skipped (Devin). Each capability string carries a `verification` field.",
    evidence="docs/compatibility.md; `evidence/ui/main/` (capabilities section)", live="n/a")

rec(18, "Recursive run tree", "verified",
    steps=f"{T}: `ac18_fixture_recursive_tree_duplicates_and_delayed_parent` (root → child → grandchild, duplicated child event, grandchild reported before its parent; delayed parent adopts the provisional child; no cycles; identical after daemon restart; children share the parent workspace), `ac21_*` (independent tasks in separate workspaces). OpenCode mock run produced a real three-level tree in the packaged UI ([main](evidence/ui/main/), 'three-level native tree visible').",
    expected="Children and descendants with identity/status/harness/account/workspace; ≥3 levels; shared and separate workspaces; duplicates and delayed parents; no cycles after reconnect.",
    actual="All pass. Tree items show status, harness, account, workspace and relationship evidence/confidence.",
    evidence="protocol tests; `evidence/ui/main/`", live="Fixtures + OpenCode mock runtime.")

rec(19, "Actual native children", "blocked",
    harness="Codex live (owner's ChatGPT login); OpenCode real runtime with mock model; Claude blocked",
    steps="Codex: live `spawn_agent` child in the CLI probe and in [codex-live](evidence/ui/codex-live/) (turn 1) attached to the correct parent with status/final message. OpenCode: child and grandchild via task tool + session store ([main](evidence/ui/main/), store test). Claude: fixtures only.",
    expected="Live delegation for Codex, Claude Code and OpenCode, plus a native grandchild where supported.",
    actual="Codex depth-1 live ✅ (grandchild not observed; Codex exec does not stream child-of-child events). OpenCode ✅ children and grandchildren, but with mock model responses. Claude ❌ (login expired).",
    evidence="see AC-02", live="Codex live; OpenCode mock.",
    blocker="Claude login (owner); Codex grandchild needs a prompt that makes the child delegate (small extra paid run) and possibly the app-server transport's `subAgentActivity`. Next: after Claude login, run a Task-delegation prompt with a nested Agent.")

rec(20, "Evidence-backed inference", "verified",
    steps=f"{T}: `ac20_prose_is_not_a_child_and_unknown_events_are_visible` (text claiming delegation creates no child; unknown Codex event type retained as `raw_unparsed` with `parser_version`, confidence `unknown`), `ac06_structured_harness_silence_is_not_completion` (incomplete telemetry → run and child `unknown`, not completed), `ac18_*` (unknown parent → provisional attachment labelled `inferred: reported parent … not seen yet`, then exact once the parent appears).",
    expected="Structured events first; inferred/unknown relationships visibly marked with source evidence; no inference presented as ground truth.",
    actual="All pass. Every child row stores `relation_source` (evidence) and `relation_confidence`; the UI shows '(inferred)' for non-exact edges.",
    evidence="protocol tests", live="Fixtures + real transcripts.")

rec(21, "Worktrees by default", "verified",
    steps=f"{T}: `ac21_parallel_worktrees_are_independent_and_collisions_are_safe` (two simultaneous tasks write `shared.txt` differently; a pre-existing `overseer/same-name` branch is left untouched and new branches get `-2`/`-3`; source checkout fingerprint including dirty file, index, stash and HEAD unchanged). UI: [main](evidence/ui/main/) and [codex-live](evidence/ui/codex-live/) confirm the source checkout status is unchanged.",
    expected="Identified branch/worktree per task; source unchanged; collisions handled without deletion.",
    actual="All pass.", evidence="protocol test; UI results", live="Fixtures; Codex live.")

rec(22, "Current dirty checkout", "verified",
    steps=f"{T}: `ac22_current_checkout_preserves_preexisting_work` (staged + unstaged + untracked + recorded unsaved draft; run edits and is interrupted; index diff, file contents and stash unchanged; `initial_dirty` recorded; Latest run diff shows only `agent.txt`/`b.txt`; fork labelled as a detected candidate). UI: [review](evidence/ui/review/) current-checkout run, review edit saved to the checkout path.",
    expected="Pre-existing staged/unstaged/untracked/unsaved work recorded and intact; initial dirtiness distinguished from run changes without inventing authorship.",
    actual="All pass. The Workspace Dirty view shows all layers; git dirtiness is not attributed to the agent.",
    evidence="protocol test; `evidence/ui/review/`", live="Fixtures.")

rec(23, "Shared workspace ownership", "verified",
    steps=f"{T}: `ac23_unrelated_writer_rejected_on_current_checkout` (second independent writer rejected while the first is active; allowed after it ends), `ac18_*` / `ac19_*` (native children share the parent's workspace id). UI output panel labels child workspaces 'shared with parent'.",
    expected="Children share the parent's workspace; unrelated writers rejected; read-only labels only when enforced.",
    actual="All pass. Overseer shows no read-only labels (it does not enforce read-only access).",
    evidence="protocol tests; `evidence/ui/main/` native-child screenshot", live="Fixtures.")

rec(24, "Safe workspace retention", "verified",
    steps=f"{T}: `ac24_cleanup_reports_and_preserves_until_confirmed` (current checkout never removed; active run blocks cleanup; interrupted run keeps untracked work; cleanup plan lists dirty files; cleanup without explicit discard refused; confirmed discard removes the worktree and keeps the branch). UI: **Clean Up Worktree…** shows the plan in a modal before removal.",
    expected="Finished/failed/interrupted runs keep their work; cleanup reports dirty files and live users; never removes the current checkout.",
    actual="All pass.", evidence="protocol test", live="Fixtures.")

rec(25, "Correct repository selection", "verified",
    steps="Packaged UI [review](evidence/ui/review/): two repositories with identical relative filenames (`a.txt`), worktrees outside the opened folder; selecting run A opens A's worktree review, run B opens B's; an edit saved in B's review changes only B's worktree (A's worktree and the source repos unchanged); refresh timings measured on B only.",
    expected="Selecting an agent opens its worktree review even outside the workspace; no leaks between paths.",
    actual="All pass.", evidence="`evidence/ui/review/`", live="OpenCode mock runs.")

rec(26, "Run snapshots and selectable bases", "verified",
    steps=f"{T}: `ac26_snapshots_and_selectable_bases` (stacked `feature-a`→`feature-b` with commits; staged/unstaged/untracked start; run 1; user edit between runs; follow-up turn 2 with its own baseline excluding the between-runs edit; turn-1 baseline still addressable; task-start preserves dirty starting contents; merge-base vs tip; target advance; missing target reported unavailable and unresolved base is an error, not an empty diff; rebase does not change recorded snapshots; baselines survive daemon restart; no staging/stash/mutation), `ac26_unknown_fork_is_reported_unavailable`. UI: [main](evidence/ui/main/) base icon tooltip names the snapshot id and base SHA; [codex-live](evidence/ui/codex-live/) latest-run shows only turn-2 changes while task-start shows the file as added.",
    expected="Latest-run default with dirty-inclusive baselines; task-start/fork/branch comparisons; icon shows identity/provenance; the listed edge cases.",
    actual="All pass. Snapshots are commits of trees built in a private temporary index and pinned under `refs/overseer/snapshots/*`; the user's index, working tree, branches and stash are untouched.",
    evidence="protocol tests; UI evidence", live="Fixtures + Codex live.")

rec(27, "Complete change and dirty views", "verified",
    steps=f"{T}: `ac27_complete_change_and_dirty_views` (clean main/main empty; staged, rename, untracked, binary, 3 MiB file listed; ignored directory and `*.log` not listed; deletion as D; a new run with an empty run diff still has dirty work in the status view), `ac26_*` (branch mode includes committed changes). UI [review](evidence/ui/review/): binary/invalid-UTF-8 and oversized files listed with explicit limitation messages; Workspace Dirty shows staged/unstaged/untracked/conflicted/unsaved.",
    expected="Selected-comparison changes and all dirtiness accessible, including main/main and limitations for binary/oversized files.",
    actual="All pass.", evidence="protocol tests; `evidence/ui/review/05-unsupported-files.png`", live="Fixtures.")

rec(28, "No cancellation blind spot", "verified",
    steps=f"{T}: `ac28_opposing_layers_remain_inspectable` (stage A→B, restore A unstaged: net diff empty but status lists staged and unstaged `a.txt`; staged deletion + untracked recreation: staged D and untracked listed). UI: Workspace Dirty opens HEAD↔index and index↔working-tree native diffs per layer.",
    expected="Opposing staged/unstaged edits remain inspectable; staged deletion + untracked recreation visible.",
    actual="All pass.", evidence="protocol test", live="Fixtures.")

rec(29, "Follow across and within files", "verified",
    commit="b5693b8",
    harness="Codex 0.155 (app-server transport), owner's existing ChatGPT login, gpt-5.6-luna, one small turn; plus OpenCode 1.15.13 driven by the mock model for sustained edits",
    steps="""1. **Live**: [codex-follow-live](evidence/ui/codex-follow-live/) (`node test/ui/scenario-codex-follow.js`): New Task from the command palette asks Codex to edit `a.txt`/`b.txt` alternately at lines 20/280/60/240/150/200/100/30. Follow reveals `a.txt:60 → b.txt:240 → a.txt:100 → b.txt:30` (across files and to new hunks within the already-open file), each labelled "agent-reported edit".
2. Sustained: [main](evidence/ui/main/) (OpenCode with the mock model, 8 paced edits) shows the same behaviour over a longer run.
3. Attribution: [review](evidence/ui/review/) writes an unrelated file during a run — it is listed in the live review but never followed or attributed; a generic (filesystem-only) run shows "Filesystem evidence only… cannot attribute or jump to them".""",
    expected="Follow navigates to observed agent edits including new hunks in the open file; unrelated user edits cannot claim agent attribution; filesystem-only limitation visible.",
    actual="All pass. Attribution comes only from harness-reported file activity (`reported` for Codex, `tool-input` for OpenCode/Claude); the first edit of a run can precede the review opening and is then shown in the list but not jumped to.",
    evidence="`evidence/ui/codex-follow-live/` (screenshots, result.json with reveals and usage), `evidence/ui/main/`, `evidence/ui/review/`", live="Live Codex; OpenCode mock for sustained runs.")

rec(30, "Navigation ownership", "verified",
    commit="b5693b8",
    harness="Codex 0.155 live (scroll/pause/resume); OpenCode mock-model runs for the longer sequences",
    steps="""1. **Live** [codex-follow-live](evidence/ui/codex-follow-live/): while Codex keeps editing, a mouse-wheel scroll pauses Follow with a visible **Resume**; the view stays at the same scrollTop while the agent makes further edits (edit count rose 4 → 6 while paused); **Resume** jumps to the latest edit and following continues.
2. [main](evidence/ui/main/) during continuous edits: scroll pause (view unchanged for 6 s), selecting another file in the navigator also pauses, Resume, unchecking Follow preserves position for 6 s of edits.
3. [review](evidence/ui/review/): selecting an existing run does not turn Follow on; switching to another agent during its live edits neither inherits Follow nor jumps (scrollTop unchanged).
4. [perf](evidence/ui/perf/): the file navigator no longer scrolls under a user who is pointing at it during live refreshes (fixed in this session).""",
    expected="Follow off preserves position; manual navigation pauses with visible Resume; review and agent switching do not unexpectedly jump.",
    actual="All pass (scrollTop unchanged in paused/off/switched states; caret stays where the user clicked when editing, see AC-32).",
    evidence="`evidence/ui/codex-follow-live/`, `evidence/ui/main/`, `evidence/ui/review/`", live="Live Codex + mock-driven sustained edits.")

rec(31, "Live Review refresh", "verified",
    steps="Packaged UI [review](evidence/ui/review/): with the review open and no manual refresh, time from the filesystem change to the updated file list: new file, atomic replace (write + rename), staging (Workspace Dirty), rename, delete, branch switch, and a write in a folder excluded from the watcher (`files.watcherExclude`) that only the reconciliation poll can see. Navigator selection before/after compared. Drafts preserved (AC-33). Load behaviour in AC-35.",
    expected="Normal changes within 2 s; missed watcher event within 5 s; selection and drafts preserved.",
    actual="See `timings` in `evidence/ui/review/result.json` (typical 0.3–0.6 s, staging ≤ ~1 s, excluded-folder ≈1.4 s); selection preserved.",
    evidence="`evidence/ui/review/result.json`, `04-refresh.png`", live="Fixture writes.")

rec(32, "Edit selected workspace", "verified",
    steps="Packaged UI: worktree mode — [main](evidence/ui/main/) and [codex-live](evidence/ui/codex-live/) type into the working-tree side of the review and Save; the file on disk in the selected worktree changes and the source checkout does not. Current-checkout mode — [review](evidence/ui/review/) saves `SAVED-FROM-REVIEW` into the checkout path; **Open in Native Diff** then type/Cmd+Z/Cmd+Shift+Z verifies native undo/redo; the unsaved buffer is labelled in Workspace Dirty ('Unsaved drafts (1)'). The base side is read-only (`originalEditable: false`).",
    expected="Working-tree side editable; save writes only the selected run's file; base immutable; native undo/redo; unsaved labelled.",
    actual="All pass.", evidence="UI results and screenshots", live="Codex live + mock/fixture runs.",
    limits="Undo/redo inside the embedded Monaco review is intentionally routed to the native editor (Branch Diff behaviour).")

rec(33, "Preserve conflicting drafts", "verified",
    steps="Packaged UI [review](evidence/ui/review/): an unsaved draft on line 5 in the native editor, then an external (agent-style) write to the same line on disk: the buffer keeps the draft and the disk keeps the external version (both preserved; VS Code's save-conflict flow reconciles). Developer: Reload Window → the dirty draft is restored and the review panel restores. Rename during an unsaved review edit keeps the draft as a dirty buffer. Branch Diff's edit journal keeps review drafts across webview reloads.",
    expected="External edits, base changes, deletion or reload cannot silently discard a draft; both versions preserved; pending draft recovered after reload.",
    actual="All pass.", evidence="`evidence/ui/review/07-native-draft.png`, `after-reload`, `rename-during-edit`, result.json", live="Fixture writes.")

rec(34, "Safe file boundaries", "verified",
    steps=f"Packaged UI [review](evidence/ui/review/): a symlink pointing outside the workspace is shown (link target) but typing into it does nothing and Save stays disabled, and the outside file is unchanged; invalid UTF-8 → 'The encoded data was not valid for encoding utf-8'; 3 MiB file → 'File exceeds the 2 MiB preview limit'; a real merge conflict is listed as Conflicted with its markers in the review; rename during edit keeps the draft. {T}: `ac34_merge_conflicts_do_not_break_snapshots_or_diffs` (snapshots/diffs work with unmerged entries without touching the real index). Traversal: review writes only target document URIs derived from the daemon's repository-relative diff paths and re-validated by `Editing.writable` (containment, `.git` block, realpath/symlink check, read-only checks); webview messages carry opaque ids, never paths.",
    expected="No writes escape via traversal/symlinks; unsupported/binary/oversized/conflicted files have truthful states and safe native access.",
    actual="All pass.", evidence="`evidence/ui/review/05-unsupported-files.png`, `conflict`, result.json; protocol test", live="Fixtures.",
    limits="Traversal is prevented by construction (no path input from the webview); there is no separate fuzz test of forged webview messages.")

rec(35, "Responsive review", "__PERF__",
    steps="Packaged UI [perf](evidence/ui/perf/) (`PERF_MINUTES=10 node test/ui/scenario-perf.js`): 10,000 tracked files; four generic fixture runs each editing its own 100 files every 0.4 s and emitting output for 10 minutes; review open on a run with 100 changed files. Samples every 30 s: extension-host RSS, daemon RSS, renderer RSS, webview JS heap, retained events per run. Navigation latency = real click in the navigator → target diff in view; refresh latency = new file write → listed.",
    expected="Navigation p95 < 250 ms; refresh within AC-31 bounds; memory/queues stabilise after draining.",
    actual="__PERFRESULT__", evidence="`evidence/ui/perf/result.json`, screenshots", live="Fixture load; no paid-model work.",
    limits="Two load bugs found here were fixed before the passing run: continuous writes starved the vendored comparison (it restarted on every change; it now publishes after one restart and catches up, and Overseer sessions skip the Git extension's status rescan), and live refreshes scrolled the file navigator under the user's pointer (it no longer auto-scrolls while the user is pointing at or scrolling it). Renderer memory (all VS Code renderer processes) grew ~24% during the load, mostly the open run panel's event log (capped at 4,000 rows), and stopped growing once the runs were drained; extension-host memory was flat for the second half and the daemon shrank after draining. The measured fixture is generic-harness load, not model runs.")

rec(36, "Packaged macOS UI", "verified",
    steps="`node extension/scripts/package.js` → `extension/overseer-0.1.0.vsix`; each scenario installs it with `code --user-data-dir <isolated> --extensions-dir <isolated> --install-extension overseer-0.1.0.vsix` (see `install.log` in each evidence folder) and launches stable VS Code 1.139.0; the flow task → follow → edit → review is driven by keyboard/mouse input through the real UI (command palette, quick picks, tree, webviews) with screenshots captured from the workbench.",
    expected="VSIX and daemon install and complete task→follow→edit→review in stable VS Code on macOS with install logs and screenshots.",
    actual="Passed in [main](evidence/ui/main/) (OpenCode mock) and [codex-live](evidence/ui/codex-live/) (Codex live).",
    evidence="`evidence/ui/*/install.log`, screenshots, result.json", live="Codex live + mock.")

rec(37, "Automated regression coverage", "verified",
    steps="`cargo test` (unit: adapters/redaction; protocol: 24 named `acNN_*` tests with real Git fixtures and fixture harnesses) and packaged-UI scenarios `test/ui/scenario-{main,review,trust,perf}.js` (fixture/mock, no paid tokens) plus `scenario-codex-live.js` and `scenario-codex-approval.js` (live, run deliberately; `DRY_RUN=1` uses a recorded transcript / synthetic app-server). Protocol tests pin every fixture harness path so they can never reach a real, paid harness. Mocks and fixtures are labelled in test names/headers and do not satisfy live-only criteria.",
    expected="Passing clean-checkout macOS checks with named tests mapped to ACs.",
    actual="See [evidence/ac-37/test-run.log](evidence/ac-37/test-run.log) (clean clone) and scenario results.",
    evidence="`evidence/ac-37/test-run.log`, `evidence/ac-04/clean-build.log`, UI results", live="Fixtures/mocks; live Codex scenario separate.",
    limits="Linux not run (AC-41).")

rec(38, "Reproducible acceptance ledger", "verified",
    steps="Audited every AC record against the implementation and evidence before handoff (see the audit table in the ledger README); reopened AC-08 (foreign-user rejection unproven) and AC-29/30 until a live-model Follow run existed; left unchecked everything lacking live or complete evidence (AC-08, AC-11–14, AC-19, AC-41).",
    expected="Every checked criterion links to evidence with commit, environment, steps, expected/actual and limitations; failures reopen; missing credentials/hardware remain blocked.",
    actual="Done for this handoff.", evidence="docs/verification/README.md", live="n/a")

rec(39, "Minimal dogfood flow", "verified",
    commit=CODEX_COMMIT,
    harness="Codex 0.155, owner's existing ChatGPT login, model gpt-5.6-luna",
    steps="[codex-live](evidence/ui/codex-live/): Overseer launched Codex on the Overseer repository in an isolated worktree (branch `overseer/spawn-exactly-one-sub-agent-whose-only-t-5`), the agent spawned a native sub-agent and created `docs/dogfood/hello.md`; Follow revealed the edit; the scenario edited the file from the review and saved; a follow-up appended a line; `git diff --check` passed; the result was committed on that branch and pushed to `beelol/overseer` (commit `943d682`). The source checkout was unchanged.",
    expected="Tiny change in an isolated Overseer worktree, follow, edit from review, checks, preserved result.",
    actual="Pass. Native delegation observed (Codex `spawn_agent`).", evidence="`evidence/ui/codex-live/`, `dogfood-hello.md`; branch on GitHub", live="Live Codex.")

rec(40, "Repository handoff", "verified",
    commit="adb2dc9 (fresh-reader clone); README fixes from that review in the following docs commit",
    steps="""1. README states progress (verified count, unverified list), links the RFC/ledger/compatibility matrix, and documents build/install, accounts, capabilities (matrix), recovery and known blockers (Follow-ups).
2. The branch was pushed to `beelol/overseer` and PR #1 opened; the fresh reader cloned it from GitHub (remote readback).
3. An independent agent with no prior context followed only the README in a new clone ([evidence/ac-40/fresh-reader.md](evidence/ac-40/fresh-reader.md)): both `npm ci`, `node extension/scripts/package.js`, VSIX install into an isolated VS Code profile (`beelol.overseer@0.1.0` listed), `cargo test` (29 passed), daemon `serve`/`ctl hello`/`ctl state`/`ctl daemon.shutdown` — all passed. Its four documentation findings (stale unverified list, binary location, `overseerd serve`, branch to clone) were fixed in the README.
4. `git grep` secret scan before pushing: only synthetic test strings matched; evidence identifies accounts only by one-way fingerprints.""",
    expected="Accurate README linked to the checklist; install/accounts/capabilities/recovery/blockers documented; implementation and evidence on GitHub without credentials; remote readback and a fresh reader succeed.",
    actual="Pass (verdict: a fresh reader could build, install and run from the README).",
    evidence="`evidence/ac-40/fresh-reader.md`; https://github.com/beelol/overseer/pull/1", live="n/a")

rec(41, "Linux verification (deferred by owner)", "blocked",
    expected="Linux build/install/regressions/account isolation/UI flow.", actual="Not attempted: no Linux environment (owner decision).",
    evidence="—", live="None.", blocker="Needs a Linux machine with VS Code and the harnesses. Next: run the README build, `cargo test`, and the UI scenarios there.")

HEAD = """# AC-{n:02d} — {title}
Status: {status}
Tested implementation commit: {commit}
Verification date and verifier: 2026-09-24/25, implementing agent (Claude Code), overnight session
OS / architecture / VS Code / harness versions: {env}; {harn}
Harness, provider and redacted account identities (if applicable): {harness}
Prerequisites and fixture: {fixture}

Steps or exact reproducible commands:

{steps}

Expected result: {expected}

Actual result: {actual}

Evidence paths (test logs, redacted transcripts, screenshots/recording): {evidence}
Live vs fixture coverage: {live}
Known limitations and remaining platform/account combinations: {limits}
Blocker, attempted alternatives and next action (if blocked): {blocker}
"""

def main():
    out = pathlib.Path(__file__).parent
    perf = pathlib.Path(out, "perf-summary.txt")
    perf_text = perf.read_text().strip() if perf.exists() else "pending"
    for n, r in sorted(R.items()):
        status = r["status"]
        if status == "__PERF__":
            status = "verified" if perf_text.startswith("PASS") else "blocked"
        text = HEAD.format(n=n, title=r["title"], status=status, commit=r.get("commit", COMMIT), env=ENV, harn=HARN,
                           harness=r.get("harness", "not applicable (fixture harnesses; no accounts)"),
                           fixture=r.get("fixture", "Real Git repositories created per test under /tmp; isolated OVERSEER_HOME; isolated VS Code profile for UI scenarios"),
                           steps=r.get("steps", "—"), expected=r["expected"], actual=r["actual"].replace("__PERFRESULT__", perf_text),
                           evidence=r.get("evidence", "—"), live=r.get("live", "—"), limits=r.get("limits", "macOS only; Linux belongs to AC-41."),
                           blocker=r.get("blocker", "not blocked"))
        (out / f"AC-{n:02d}.md").write_text(text)
    print(len(R), "records written")
    sync(out)


EXTRA_FOLLOWUPS = [
    "Decide a retention policy for snapshot refs under `refs/overseer/snapshots/*` (they accumulate per turn; harmless but unbounded). Clearly labeled follow-up; no AC covers it.",
    "Decide whether the *existing login* Codex profile should be discouraged: on this machine `~/.codex` is shared with the ChatGPT desktop app and switched accounts during the session (see [AC-02](docs/verification/AC-02.md)). Clearly labeled follow-up.",
    "Map the Codex app-server `subAgentActivity` / child-thread notifications so Codex grandchildren and child output stream live (would strengthen [AC-19](docs/verification/AC-19.md)); approvals already use the app-server transport.",
    "Remove or update the stale `~/Library/pnpm/codex` (0.1.x) on PATH; Overseer ignores it in favour of the ChatGPT.app bundle. Owner environment note.",
    "VS Code on this machine trusts `/` in its workspace-trust list, so folders never open in Restricted Mode; the trust test uses an empty window ([AC-08](docs/verification/AC-08.md)). Owner environment note.",
]


def status_of(path):
    for line in path.read_text().splitlines():
        if line.startswith("Status:"):
            return line.split(":", 1)[1].strip()
    return "not started"


def sync(out):
    import re
    root = out.parent.parent
    rfc = root / "docs/overseer-rfc.md"
    text = rfc.read_text()
    titles = dict(re.findall(r"\*\*AC-(\d\d) — ([^*]+?)\.\*\*", text))
    statuses = {}
    for n in range(1, 42):
        f = out / f"AC-{n:02d}.md"
        statuses[n] = status_of(f) if f.exists() else "not started"
    verified = [n for n, st in statuses.items() if st.startswith("verified")]
    for n in range(1, 42):
        box = "[x]" if n in verified else "[ ]"
        text = re.sub(r"- \[[ x]\] \*\*AC-%02d " % n, f"- {box} **AC-{n:02d} ", text)
    rfc.write_text(text)
    rows = ["| AC | Criterion | Status | Record |", "| --- | --- | --- | --- |"]
    for n in range(1, 42):
        rows.append(f"| AC-{n:02d} | {titles.get(f'{n:02d}', '')} | {statuses[n]} | [AC-{n:02d}.md](AC-{n:02d}.md) |")
    unverified = [n for n in range(1, 42) if n not in verified]
    audit = [f"- Verified: {len(verified)} / 41 ({', '.join(f'AC-{n:02d}' for n in verified)}).",
             f"- Not verified: {', '.join(f'AC-{n:02d}' for n in unverified)} — each record states the exact blocker and next action.",
             "- Every verified record was re-read against its evidence folder/test before checking; anything that relied only on fixtures where the criterion demands live evidence stays unchecked."]
    ledger = out / "README.md"
    lt = ledger.read_text()
    if "__TABLE__" in lt:
        lt = lt.replace("__TABLE__", "\n".join(rows)).replace("__AUDIT__", "\n".join(audit))
    else:
        lt = re.sub(r"(## Status by criterion\n\n)(.*?)(\n\n## Evidence layout)", lambda m: m.group(1) + "\n".join(rows) + m.group(3), lt, flags=re.S)
        lt = re.sub(r"(## Audit notes \(handoff\)\n\n)(.*)$", lambda m: m.group(1) + "\n".join(audit) + "\n", lt, flags=re.S)
    ledger.write_text(lt)
    follow = []
    for n in unverified:
        rec_text = (out / f"AC-{n:02d}.md").read_text() if (out / f"AC-{n:02d}.md").exists() else ""
        blocker = next((l.split(":", 1)[1].strip() for l in rec_text.splitlines() if l.startswith("Blocker, attempted")), "see record")
        follow.append(f"- [ ] [AC-{n:02d}](docs/verification/AC-{n:02d}.md) ({titles.get(f'{n:02d}', '')}): {blocker}")
    follow += [f"- [ ] {x}" for x in EXTRA_FOLLOWUPS]
    readme = root / "README.md"
    rt = readme.read_text()
    rt = re.sub(r"Verified acceptance\ncriteria: \*\*[^*]+\*\*", f"Verified acceptance\ncriteria: **{len(verified)} / 41**", rt)
    rt = rt.replace("__VERIFIED__ / 41", f"{len(verified)} / 41")
    rt = rt.replace("__UNVERIFIED__", ", ".join(f"AC-{n:02d}" for n in unverified))
    rt = re.sub(r"(Unverified:\n)AC-[0-9, AC-]+(\. The biggest gaps)", lambda m: m.group(1) + ", ".join(f"AC-{n:02d}" for n in unverified) + m.group(2), rt)
    rt = re.sub(r"(## Follow-ups\n\n.*?\n\n)(.*?)(\n\n## Project documents)", lambda m: m.group(1) + "\n".join(follow) + m.group(3), rt, flags=re.S)
    rt = rt.replace("__FOLLOWUPS__", "\n".join(follow))
    readme.write_text(rt)
    print("verified:", len(verified), "unverified:", unverified)

if __name__ == "__main__":
    main()
