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

rec(8, "Local access boundary", "verified", commit="8890f1d", date="2026-09-25",
    steps=f"""1. {T}: `ac08_socket_is_owner_only_and_requests_are_not_shell` — socket mode 0600, socket directory 0700; malformed JSON → `parse_error`; non-object params → `invalid_params`; non-string method → `invalid_request`; >1 MiB request → `request_too_large`; shell fragments in `repo`/`path` rejected; argv with `$(touch …)`/`; rm -rf /` passed literally (no file created); unknown methods rejected.
2. Every accepted connection is checked with `getpeereid` against the daemon's uid (`daemon/src/server.rs`); shim control sockets do the same.
3. {T}: `ac08_connections_from_a_foreign_uid_are_rejected_and_logged` — the daemon runs with the test-only `OVERSEER_TEST_EXPECT_UID=4242424`, which can only reject more peers (a peer must still be the daemon's own uid). The test process is therefore a foreign peer: its `task.create` gets no reply, nothing executes, and the log has `rejected connection from uid Some(<uid>)`.
4. Packaged UI [trust](evidence/ui/trust/): an untrusted window (VS Code Restricted Mode via `security.workspace.trust.emptyWindow=false`) — the trust editor shows "You are in Restricted Mode", **Overseer: New Task** is not offered in the command palette, the Agents view explains that launching requires trust, and no task exists afterwards.""",
    expected="Only the local user can command the daemon; untrusted workspaces cannot launch; malformed requests cannot execute shell fragments.",
    actual="Untrusted-workspace, malformed-request and shell-injection checks pass; socket (0600) and directory (0700) are owner-only. **Real second user:** the owner ran `sudo -u nobody … ctl hello` against the live daemon and got `Permission denied (os error 13)` on the socket; nothing reached the daemon (its log shows no connection). Defense in depth: a peer that could reach the socket but is not the owner uid is refused without a reply, nothing runs, and the daemon logs `rejected connection from uid …` (protocol test with the test-only owner override, which can only narrow access).",
    evidence="protocol tests; [second-user.txt](evidence/ac-08/second-user.txt); `evidence/ui/trust/`",
    live="Real VS Code workspace trust; a real second macOS user (nobody) against the owner's running daemon.",
    blocker="not blocked",
    limits="The real second user was stopped by filesystem permissions before reaching the daemon; the peer-uid check behind it is exercised by the protocol test. On this machine the VS Code trust list trusts `/` for folders, so the untrusted case uses an empty window; folder-level Restricted Mode uses the same `isWorkspaceTrusted` gating.")

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

rec(11, "Account profiles", "verified", commit="adab32b", date="2026-09-25 (owner-confirmed live on the owner's Mac)",
    harness="LIVE: a throwaway Overseer ChatGPT account signed in, signed out and signed in again by the owner through the UI (2026-09-25); the owner's two ChatGPT accounts signed in through Overseer's per-account login command (A in the browser, B with a device code; 2026-09-25). Fixtures: synthetic account CLI for missing, expired and renewed logins",
    steps="""1. `cargo test` — `ac46_accounts_by_provider_fixed_vs_desktop_linked_and_isolated_resign_in_and_removal` (creation-time folders, sign-in/sign-out/re-sign-in isolation).
2. `node test/ui/scenario-signin.js` (synthetic account CLI):
   - Add and name "Claude work" without signing in; open New Task → Claude Code.
   - Sign in from the palette; run a task.
   - Delete the credential (expired login); send a follow-up.
   - Click **Sign in again** in the conversation; send another follow-up.
3. `node test/ui/scenario-accounts.js`: provider picker, device-code/browser choice, sign out and sign in again.
4. Live: `profile.status` for ChatGPT A and B on the owner's daemon ([live-accounts.txt](evidence/ac-46/live-accounts.txt)).
5. Live re-sign-in with the owner ([session](evidence/ac-13/session.md), shared with AC-13). A throwaway Overseer ChatGPT account was created signed out. The owner signed it in through the UI; the agent signed it out; the owner signed it in again through the UI (right-click → **Sign In…**).""",
    expected="Login and reauthentication through the UI for each claimed supported account path, including a missing/expired login; no API keys.",
    actual="""- **Missing login:** "Claude work" shows "not signed in". Its New Task tile is disabled with "Sign in first (Accounts → Sign In). Account login only; no API keys."
- **Sign in:** **Overseer: Sign In** → Claude work ran the account's own login in a terminal; the account shows "signed in · max · f03ab3f8" and a task ran with it.
- **Expired login:** the next turn failed with `[auth]: Failed to authenticate: OAuth session expired and could not be refreshed`. The conversation shows the auth error with a **Sign in again** button.
- **Reauthentication:** clicking **Sign in again** ran that account's login again ("signed in · max · 55f70cc2"); the follow-up completed ("hello from claudia-again…").
- **ChatGPT:** Sign In offers "in the browser" and "with a device code" (`codex login --device-auth`).
- **Live:** ChatGPT A (Team, `2bb3fae1`) signed in through the browser flow and ChatGPT B (Plus, `27e64e3a`) through the device-code flow, each into its own profile folder. Both report ChatGPT-account login and no API key. The device-code attempt for one account first failed until the owner enabled device-code sign-in in ChatGPT security settings; the error text is passed through.
- **Live re-sign-in (2026-09-25):**
  - The throwaway account started "not signed in". The owner signed it in through the UI (team, `2bb3fae1`) and the agent signed it out (`logged_in: false`, its auth.json gone). The owner then signed it in again through the UI with another ChatGPT login (plus, `27e64e3a`). Owner: "ok i sigend into a diff acc".
  - Each login landed only in the throwaway's folder: `chatgpt-account`, no API key.
  - **Bug found:** a signed-out account's menu offered Sign Out, and Sign In was only an inline icon. The menu now follows the sign-in state (adab32b; checked in `scenario-accounts.js`).
- **Claude:** the Claude account paths are AC-53.""",
    evidence="[live re-sign-in session](evidence/ac-13/session.md), [signin scenario](evidence/ui/signin/) (missing login, expired login with Sign in again, after re-sign-in), [accounts scenario](evidence/ui/accounts/), [live account status](evidence/ac-46/live-accounts.txt)",
    live="Live ChatGPT sign-ins (A browser, B device code) and a live sign-in → sign-out → sign-in again of a throwaway ChatGPT account through the UI. Expiry is exercised with the synthetic CLI. Claude accounts: AC-53.",
    blocker="not blocked")

rec(12, "Two simultaneous ChatGPT subscriptions", "verified", commit="1c4c856", date="2026-09-25",
    harness="LIVE: Codex 0.155 exec, model gpt-5.6-luna, on the owner's daemon with two fixed accounts: ChatGPT A (Team) and ChatGPT B (Plus), each signed in through Overseer into its own profile folder",
    fixture="Disposable repository `/tmp/ovs-ac12-5BVtIM`; one tiny prompt per account",
    steps="""1. `node docs/verification/evidence/ac-12/run-ac12.js`: `profile.status` for both accounts; two `task.create` calls back to back (codex, account A / account B, separate worktrees), each asking the agent to write its own file and run `sleep 20` before replying; poll until both finish; read events, worktrees and files.
2. Check where each run's native Codex session was stored ([session-homes.txt](evidence/ac-12/session-homes.txt)) and each run's launch `CODEX_HOME`. No token is read or printed.""",
    expected="Overlapping live timestamps, redacted distinct account identities, successful independent file edits from both; not two processes under one account and not subscription-plus-API-key.",
    actual="""- **Distinct accounts:** A is `chatgpt-account`, plan team, account fingerprint `2bb3fae1`, user `676d42e6`. B is `chatgpt-account`, plan plus, account `27e64e3a`, user `76880f38`. Neither has an API key.
- **Overlap:**
  - A was running 1790367383175 → turn done 1790367411173.
  - B was running 1790367383547 → turn done 1790367417212.
  - That is 27.6 s of overlap, and both runs showed `running` together in 28 one-second polls.
- **Independent edits:**
  - A's worktree `overseer/ac-12-from-a-txt` has from-a.txt "written with ChatGPT account A".
  - B's worktree `overseer/ac-12-from-b-txt` has from-b.txt "written with ChatGPT account B.".
  - Neither worktree has the other's file. Both runs completed with exit 0.
- **Account routing:** run A launched with `CODEX_HOME=<Overseer>/profiles/p-f262c1bc4958/codex` and run B with `…/p-52fb6421edd2/codex`. A's native thread `01a0da36-40a3…` is stored only in A's home, and B's `01a0da36-44cc…` only in B's.""",
    evidence="[concurrent-a-b.json](evidence/ac-12/concurrent-a-b.json) (identities redacted to fingerprints, timestamps, files), [session-homes.txt](evidence/ac-12/session-homes.txt), [run-ac12.js](evidence/ac-12/run-ac12.js)",
    live="Live, two paid ChatGPT subscriptions (Team and Plus).",
    limits="Codex exec transport; the same accounts work with codex-app (shared profile folder).")

rec(13, "Credential isolation on macOS", "verified", commit="adab32b", date="2026-09-25 (owner-confirmed live on the owner's Mac)",
    harness="LIVE on the owner's daemon: ChatGPT B (Plus) doing real Codex work, ChatGPT A (Team), the desktop-linked login (Pro), a disposable OpenAI account C, and a throwaway ChatGPT account the owner signed in, the agent signed out, and the owner signed in again (2026-09-25). Fixtures: synthetic account CLI for sign-in/out/expiry isolation",
    steps="""1. `node docs/verification/evidence/ac-13/run-ac13.js` against the owner's daemon (no other runs active):
   - Record the identities of A, B and the desktop login; start B on a Codex task (sleep 25, then write b.txt) in a disposable repository.
   - While it runs: create account C, fetch its sign-in command, sign it out, remove it.
   - After B finishes: restart the daemon and re-check identities.
   - Scan every file under the Overseer data directory, except credential homes and worktrees, for the last 40 characters of each real token and for JWT-like strings. Only counts are printed.
2. `cargo test` — `ac46_accounts_by_provider_fixed_vs_desktop_linked_and_isolated_resign_in_and_removal` (fixture sign-in/sign-out/re-sign-in isolation, including the desktop login switching).
3. `node test/ui/scenario-signin.js` (fixture expiry of one account and re-sign-in from the UI).
4. Live session with the owner ([session](evidence/ac-13/session.md)): identities recorded; a throwaway Overseer ChatGPT account was created and ChatGPT B started on tiny Codex tasks in a throwaway repository. The owner signed the throwaway in through the UI while B was running. The agent signed it out while B was running. The owner signed it in again through the UI. After B finished, [step-d.js](evidence/ac-13/step-d.js) restarted the daemon (no runs active), re-checked the identities, ran the leak scan (counts only) and removed the throwaway.""",
    expected="Login, logout, refresh and expiry in profile A do not switch profile B's identity or corrupt its credentials/configuration; no leakage in logs/database; unrelated active logins undisturbed.",
    actual="""- **Before:** A team `2bb3fae1`, B plus `27e64e3a`, desktop pro `2e1fa921`. All `chatgpt-account`, no API key.
- **During B's run** (r-53d79f5a9418, running):
  - C was created with its own `codex` folder (mode 700) and reported not signed in.
  - C's sign-in command is `codex login --device-auth` with `CODEX_HOME` set to C's own folder.
  - `profile.logout` on C exited 0. A and B were unchanged mid-run.
  - Removing C deleted only C's folder; A and B were unchanged.
- **B finished:** completed (exit 0), and b.txt reads "B still works.".
- **Daemon restart:** pid 63741 → 21639. A, B and the desktop login had identical identities afterwards, and B's run stayed `completed`.
- **Leak scan:** 9 token suffixes checked against 99 files (database, WAL, logs, launch files, raw output segments): 0 hits, and no JWT-like content.
- **Fixtures:** signing one account out, in again, or letting it expire only changes that account. The desktop login switching changes only the linked account.
- **Live sign-in / sign-out / sign-in again (2026-09-25):**
  - **Sign-in:** the owner signed the throwaway in during B's run r-f7dc53c30938 (team `2bb3fae1`, the same ChatGPT account as A, in its own folder). A, B and the desktop login were unchanged, and B completed and wrote b2.txt.
  - **Sign-out:** the agent signed the throwaway out during B's run r-c180b4e20206. A (the same ChatGPT account) stayed signed in, B and the desktop login were unchanged, and B completed and wrote b3.txt.
  - **Sign-in again:** the owner signed it in again through the UI with B's ChatGPT account (plus `27e64e3a`). A, B and the desktop login were unchanged.
  - **Daemon restart:** pid 73218 → 76753, with identical identities afterwards.
  - **Leak scan:** 12 token suffixes (A, B, throwaway, desktop) checked in 125 Overseer files and 4 extension logs: 0 hits, and no JWT-like content.
  - **Throwaway removed:** its folder is gone. A, B and the desktop login were never signed out.""",
    evidence="[live session](evidence/ac-13/session.md), [step-d result](evidence/ac-13/step-d-result.json), [isolation-live.json](evidence/ac-13/isolation-live.json), [run-ac13.js](evidence/ac-13/run-ac13.js), [accounts scenario](evidence/ui/accounts/), [signin scenario](evidence/ui/signin/)",
    live="Live for B's concurrent work, a throwaway account's sign-in, sign-out and sign-in again through the UI (two real ChatGPT logins), the daemon restart and the leak scan. Token refresh and expiry are exercised with the synthetic CLI.",
    limits="Codex stores credentials in each account's auth.json (file backend). Claude Code on macOS may use the Keychain; its per-account isolation is checked only with fixtures here.",
    blocker="not blocked")

rec(14, "Initial adapters", "verified",
    commit=f"{CODEX_COMMIT} (Codex exec live), 7036cd6 (Codex app-server live), fdf1340 (Claude Code live), b5693b8 (OpenCode)",
    harness="Codex 0.155 on the owner's ChatGPT login (plan pro); Claude Code 2.1.246 on the owner's claude.ai login (plan max, signed in on 2026-09-25), model haiku; OpenCode 1.15.13 with the deterministic mock provider and with local Ollama models (no OpenCode account)",
    steps="""1. **Codex** (live): [codex-live](evidence/ui/codex-live/) — edit, follow-up (resume), interrupt, streaming, capabilities; [codex-approval-live](evidence/ui/codex-approval-live/) — the app-server transport with approvals.
2. **Claude Code** (live): [claude-live](evidence/ui/claude-live/) (`SHARED_DAEMON=1 node test/ui/scenario-claude-live.js`) — New Task from the command palette; the Write permission request is answered with Allow in the run panel and `hello.md` lands in the worktree; nested Agent child + grandchild; follow-up (`--resume`) edits the file after a second Allow with its own latest-run baseline; a running Bash loop is interrupted from the UI (`interrupted by user`).
3. **OpenCode**: [main](evidence/ui/main/) through the real `opencode run --format json` adapter with the mock model (edit/follow-up/interrupt), plus real local models via Ollama ([log](evidence/ac-14/opencode-ollama.log)): `qwen3-coder:30b` wrote the requested file; `qwen2.5-coder:14b` printed its tool call as text (model limitation).
4. A live Claude run exposed an adapter bug, fixed before the passing run: Claude emits an interim `result` while a background subagent runs, and closing stdin then denied later tool permissions ("Stream closed"). Regression test `ac14_fixture_claude_background_subagent_keeps_session_open_for_permissions`.""",
    expected="Tiny live account-authenticated edit/follow-up/interrupt probes for Codex and Claude Code; OpenCode integrated through its real adapter with honest mock/local coverage; exact versions recorded.",
    actual="All pass. No OpenCode account authentication is claimed.",
    evidence="`evidence/ui/codex-live/`, `evidence/ui/codex-approval-live/`, `evidence/ui/claude-live/`, `evidence/ui/main/`, `evidence/ac-14/opencode-ollama.log`", live="Codex live; Claude Code live; OpenCode mock + local models.")

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

rec(19, "Actual native children", "verified",
    commit="fdf1340 (Claude), e055167 (Codex app-server grandchild, OpenCode live delegation)",
    harness="Codex 0.155 (owner's ChatGPT login); Claude Code 2.1.246 (owner's claude.ai login, haiku); OpenCode 1.15.13 with a real local model (Ollama qwen3-coder:30b)",
    steps="""1. **Claude Code**: [claude-live](evidence/ui/claude-live/) — the prompt asks for an Agent subagent that itself launches an Agent; Overseer shows root → child → grandchild with exact provenance (Agent `tool_use` ids, `parent_tool_use_id` nesting, `system/task_*`), each child's output and completed status, all sharing the parent's workspace.
2. **Codex**: the exec transport captured a live depth-1 child ([codex-live](evidence/ui/codex-live/)). With the app-server transport and `extra_args: ["-c", "agents.max_depth=2"]`, a live run produced root → child → grandchild; each child thread's items and completion are attributed to that child ([log](evidence/ac-19/live-r-84d950aec2f6.log)). Default Codex depth is 1 (the first probe without the setting produced no grandchild).
3. **OpenCode**: a live run with a real local model (not the mock) delegated twice; the child and grandchild sessions were attached from OpenCode's session store ([log](evidence/ac-19/live-r-9db11694a476.log)); requires `agent.general.permission.task=allow` for a subagent to delegate further.
4. Fixture/regression coverage: `ac18_*`, `ac19_fixture_codex_live_transcript_children`, `ac19_fixture_codex_app_child_threads_nest_and_do_not_end_the_parent`.""",
    expected="Live delegation sessions for Codex, Claude Code and OpenCode attached to the correct parent with output/status, plus a native grandchild where supported; unsupported depth documented.",
    actual="All pass. Depth limits: Codex default 1 (2 with `agents.max_depth=2`); Claude Code nests (depth 2 observed); OpenCode subagents need the `task` permission to delegate.",
    evidence="`evidence/ui/claude-live/`, `evidence/ac-19/*.log`, `evidence/ui/codex-live/`", live="Live for all three harnesses (OpenCode with a local model).")

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

rec(42, "Hunk accept and reject", "verified", commit="af98cce", date="2026-09-25",
    harness="Fixture generic runs (deterministic edits); LIVE: Codex exec (`codex (existing login)`, gpt-5.6-luna), one tiny prompt",
    steps="""1. `node extension/scripts/package.js`, then `LIVE=1 node test/ui/scenario-hunks.js` (isolated profile, packaged VSIX, CDP clicks on the hunk toolbar).
2. A worktree run edits a.txt (3 hunks) and b.txt (2 hunks), adds new.txt and scratch.txt; b.txt is then staged with `git add`.
3. Reject a.txt L100, Accept a.txt L10; open a.txt in the native editor, Cmd+Z then Cmd+Shift+Z; Reject staged b.txt L50; Accept new.txt, Reject scratch.txt.
4. Agent-write races: the file is rewritten right before the Reject click reaches VS Code (and again for Accept).
5. Refresh; change the accepted hunk again; reload the window.
6. A current-checkout run: Reject L20, Accept L30.
7. LIVE: a Codex run edits a.txt L40/L140 and b.txt L90; Reject L140, Accept L90.
8. Reran `scenario-review.js`, `scenario-main.js`, `scenario-restore.js`; `cargo test`.""",
    expected="Per-hunk Accept (reviewed, no staging) and Reject (base restored in that workspace) with native undo; no effect on other hunks/files/drafts/workspaces; concurrent agent edits are conflicts; reviewed state survives refresh and reload and clears when the hunk changes; worktree and current checkout.",
    actual="""- Reject a.txt L100 → disk "L100: original" while L10/L200 and b.txt keep their agent edits (4 hunks remain).
- Accept L10 → hunk marked ✓ reviewed; a.txt bytes and `git diff --cached` unchanged.
- Native editor Cmd+Z → the rejected hunk returns as an unsaved change (tab dirty, review shows 3 hunks); Cmd+Shift+Z → gone again; disk keeps the saved Reject.
- Staged b.txt: Reject restores L50 in the working tree; the index still has the staged edit (`git diff --cached` identical).
- Untracked: new.txt Accept → reviewed; scratch.txt Reject → empty file (a new file's base content is empty).
- Agent write during Reject → "Reject was not applied: this file changed while the hunk was being rejected (conflict). Nothing was overwritten…"; disk keeps "L200: agent edit v2". During Accept → "Not marked reviewed: a.txt changed while you were accepting this hunk (conflict)…".
- Reviewed state survives Refresh and a window reload (workspace state); editing the accepted hunk again unmarks it.
- Current checkout: L20 rejected on disk, L30 accepted; nothing staged.
- LIVE Codex: the run completed; L140 rejected (disk "L140: original"), L40 still "codex edit", b.txt L90 accepted.
- Found on the way:
  - The vendored editing path trusted a clean VS Code document that had not yet reloaded an external write, so a Reject could have overwritten the agent's newer text. Clean documents are now compared with disk.
  - Conflict notices were cleared by the next live refresh within milliseconds; they now persist for 10 s.
  - The first toolbar design overlapped code in narrow editors and caught a click meant for the text; it is now a compact icon toolbar on the right edge (✓ Accept, ○ Unmark, ↶ Reject, with labels and tooltips).""",
    evidence="[hunks scenario](evidence/ui/hunks/) (screenshots before, accepted and rejected, after conflicts, after reload, current checkout, live Codex; result.json)",
    live="Live Codex run for the several-hunks case; the other cases use deterministic generic-harness edits (same review path).",
    limits="Reject on a new (untracked) file leaves it empty rather than deleting it. Undo is in the native editor (the review's own editor defers undo to it, as before).")

rec(43, "Structured run conversation view", "verified", commit="80112ee", date="2026-09-25",
    harness="LIVE: Codex exec and Codex app-server (`codex (existing login)`, model gpt-5.6-luna), Claude Code (`claude (existing login)`, haiku); one tiny prompt each. Fixtures: codex-app and Claude fixtures, generic bursts",
    steps="""1. `cargo test` — `ac43_fixture_tool_calls_carry_inputs_and_results_for_the_conversation_view` (tool input/result events for Claude and app-server), `ac14_fixture_claude_background_task_finishing_before_the_interim_result_still_keeps_the_session_open` (regression found by the live run).
2. `node test/ui/scenario-conversation.js` (fixtures, packaged UI): codex-app run with child and grandchild threads, command approval and file change; Claude run with a Write permission; a 6,000-line generic run (retention bound 5,000) and a live 6,000-line burst.
3. `node test/ui/scenario-conversation-live.js` (LIVE): for Codex exec, Codex app-server (approval policy `untrusted`) and Claude Code, a prompt that spawns one sub-agent replying "hi" and then creates a file; permission requests answered with the conversation's inline **Allow once**; expand/collapse the first tool call; click the file edit.
4. Reran `scenario-main.js`, `scenario-review.js`, `scenario-trust.js` and `scenario-restore.js` (all pass).""",
    expected="Conversation with turns, collapsible tool calls with inputs/results, file edits opening the hunk in the right worktree review, inline permissions with decisions, children nested under the spawning tool call with their own output, highlighted errors, per-turn usage; raw event log and raw output still available; responsive at the retention bound with truncation visible.",
    actual="""- **Live Codex exec:** tools `shell`, `collab:spawn_agent`, `collab:wait`, `apply_patch`; the sub-agent "Reply with exactly: hi." is nested under `collab:spawn_agent` with its reply "hi."; usage "195,934 in · 968 out"; clicking `hello.txt` opened the Codex worktree review at that file.
- **Live Codex app-server:** three approvals answered inline (two of the sub-agent's `sed` reads, then the file change), each recorded as "✔ Allowed: …"; the child is nested under `collab:spawn_agent` with its reply "hi"; usage shows tokens plus "rate limits reported"; `app.txt` opened in the app-server worktree review.
- **Live Claude Code:** `Agent` then `Write`; the subagent is nested under `Agent` with its reply "hi"; the Write permission was answered inline ("✔ Allowed: Write"); usage "28 in · 431 out · $0.0446"; `hello.md` opened in the Claude worktree review.
- **Fixtures:** child and grandchild nesting with their own output; tool calls expand to input and result and collapse again; the codex-app edit and the Claude edit each open their own worktree's review (checked by path) at line 1; the Event log tab lists the raw events.
- **Bounds:** 5,002 retained events render in 55 ms with "Older history was truncated by the retention bound" visible and the newest line shown. During a live 6,000-event burst, webview event-loop lag was p95 2 ms, max 90 ms (AC-35 bound 250 ms).
- **Bugs found and fixed on the way:**
  - History requested the oldest 5,000 events, so at the bound it dropped the newest ones and the truncation marker; it is now paged.
  - Live events were posted one per message with a forced scroll each (a 2.8 s stall); they are now batched with one scroll per frame.
  - Claude 2.1.x continues with another turn after a backgrounded task finishes, even when it finished before the interim `result`. Overseer had closed stdin, so the Write permission failed with "Stream closed". The session now stays open until every expected turn has a result.
  - Subagent replies arrive only in `task_notification.summary`; they are now recorded as the child's output.""",
    evidence="[fixture scenario](evidence/ui/conversation/) (screenshots: pending approval, tool expanded, edit opened in review, event-log tab, burst), [live scenario](evidence/ui/conversation-live/) (result.json with each run's conversation summary; screenshots of each conversation, permission and edit-in-review)",
    live="Live for Codex exec, Codex app-server and Claude Code. OpenCode shares the renderer (tool inputs/results from its tool parts), exercised by the fixture scenarios' generic and mock runs.",
    limits="Child tool calls inside a child are shown as text lines (harnesses report them without separate ids). Codex exec has no interactive permissions (sandbox policy), so its conversation has none.")

rec(44, "Merge back", "verified", commit="75c7375", date="2026-09-25",
    harness="LIVE: Codex exec (`codex (existing login)`, gpt-5.6-luna) for the clean and conflicting merges, including the conflict follow-up; generic runs for the dirty-target and active cases",
    steps="""1. `cargo test` — `ac44_clean_merge_back_commits_the_worktree_and_merges_only_on_request`, `ac44_conflicts_are_resolved_in_the_worktree_before_the_target_changes`, `ac44_refuses_dirty_target_active_runs_and_current_checkout_tasks`.
2. `node test/ui/scenario-merge.js` (LIVE, disposable repository, packaged UI, custom dialogs): two Codex runs (append to b.txt; change a.txt L5), then main commits a different L5. **Merge back…** from each run panel: Prepare → review → Complete. A generic run with a dirty README in the source checkout. A running generic run.""",
    expected="Never automatic; clean merge back; conflicts handed to the same harness/account/session and reviewed before completing; dirty target refused and untouched; disabled with an explanation while active or when unmergeable.",
    actual="""- **No automatic merge:** main was unchanged after both live runs finished.
- **Clean merge back:** the dialog explained the three steps: commit 1 uncommitted worktree file, merge main into `overseer/merge-clean` in the worktree, then review. The review switched to "Merge-base with main" for the run's worktree. The confirmation listed exactly what lands. Confirming produced the merge commit "Merge overseer/merge-clean into main (Overseer merge back)" with b.txt's new line, and the checkout stayed clean.
- **Conflicting merge back:**
  - main had moved L5 to "from main", so `merge_prepare` stopped with a conflict in a.txt inside the worktree.
  - "Sent to codex as a follow-up in the same session": the same run got turn 2 (Codex resumed its native thread) and resolved the file. main did not move meanwhile.
  - The second **Merge back…** staged the resolution, completed the worktree merge, and showed "1 file(s): M a.txt" for review. Confirming merged it into main with no conflict markers (L5 is "from agent").
- **Dirty target:** preparing only touched the worktree. Completing was refused: "The source checkout … has uncommitted changes (README.md). Overseer never disturbs them; commit or stash them first." The checkout's status, HEAD, index and README were byte-identical. Protocol tests also refuse a target checkout on another branch ("switch it to main") and current-checkout tasks ("no separate branch to merge back"), and treat an already-merged branch as "Nothing to merge".
- **Active run:** the button was disabled with "Wait for the run to finish or interrupt it before merging back."; the daemon refuses too ("The run is still running …").""",
    evidence="[merge scenario](evidence/ui/merge/) (dialog screenshots for prepare/complete, merged states, busy button; result.json); `cargo test` ac44_* tests",
    live="Live Codex for both merges and the conflict resolution; the refusal cases use generic runs (harness-independent daemon logic).",
    limits="The final merge uses the user's Git identity from the repository's config. Opening a PR instead is AC-50 (coming soon). Generic runs cannot take the conflict follow-up, so the user resolves those files and runs Merge back again.")

rec(45, "Visible background agents", "verified", commit="1e2f24e", date="2026-09-25",
    harness="Claude Code 2.1.x with the owner's existing claude.ai login (`system-claude`), model haiku, one tiny turn",
    steps="""1. `cargo test` — `ac45_last_vscode_window_closing_with_active_runs_posts_a_notice_but_a_reload_does_not`, `ac45_no_notice_when_nothing_is_running`, `ac45_stop_all_interrupts_runs_forces_stragglers_and_exits_the_daemon` (fixture notifier via `OVERSEER_NOTIFY_COMMAND`; a SIGINT-ignoring run proves the SIGTERM fallback).
2. `node test/ui/scenario-background.js` (LIVE, isolated profile and OVERSEER_HOME, real `osascript` notifier, grace shortened to 3 s via `OVERSEER_BACKGROUND_NOTICE_MS`): a Claude Haiku run executes a 2-minute `echo`/`sleep` loop (Bash permission allowed); quit VS Code with Cmd+Q; wait; relaunch; run **Overseer: Stop Agents and Daemon…** and confirm; start the daemon again with nothing running and quit VS Code again.""",
    expected="A notification naming running agents when the last window closes (none for a reload or with nothing running); reopening shows them; Stop Agents and Daemon confirms, interrupts and stops everything with no Overseer or harness processes left.",
    actual="""- The daemon counts VS Code windows (`hello` with `client: "vscode"`). Closing one of two windows, or reloading (reconnect within the grace period, default 15 s), sends nothing.
- After Cmd+Q the daemon posted: title "Overseer: 1 agent still running", body "claude: tick loop. They keep running with VS Code closed. Reopen VS Code to watch them, or run “Overseer: Stop Agents and Daemon”." — delivered via `osascript (ok)` and recorded as a `background_notice` event. The Claude run stayed `running`.
- Reopening VS Code showed "Overseer agents kept running while VS Code was closed: claude: tick loop. 1 still active." with **Show Agents** / **Stop Agents and Daemon**, and the Agents view listed the running run.
- **Stop Agents and Daemon…** showed a modal "Stop 1 running agent and the Overseer daemon?" listing `claude: tick loop (running)`. Confirming interrupted the run (`interrupted`), and afterwards no daemon, shim or Claude process remained. The window showed "Overseer stopped" and did not respawn the daemon; other windows get `daemon_stopping` and stay stopped too.
- With nothing running, quitting VS Code logged "no active agents, no notice" and posted nothing.
- **Banner on screen:** the agent cannot capture the screen, so the owner watched. Two notifications were fired from an isolated daemon with the same code path (a VS Code client disconnects while an agent runs, then the real `osascript` notifier), at 22:29 and 22:33 UTC. The owner confirmed "yes banner showed up" ([banner-confirmed.txt](evidence/ac-45/banner-confirmed.txt)).""",
    evidence="[banner confirmation](evidence/ac-45/banner-confirmed.txt), [fire-banner.js](evidence/ac-45/fire-banner.js), [background scenario](evidence/ui/background/) (scenario.log, result.json, screenshots: running before close, reopened, confirm stop, stopped; `overseerd.log` excerpt); `cargo test` ac45_* tests",
    live="Live Claude Code run (tiny Haiku turn); real macOS `osascript` notifier. Protocol tests use fixture runs and a fixture notifier.",
    limits="`osascript` notifications appear under Script Editor; if its notifications are turned off in System Settings, macOS drops the banner silently (the reopen message still appears).",
    blocker="not blocked")

rec(46, "Simple account governance", "verified", commit="5dce9f5", date="2026-09-25",
    harness="Synthetic account CLI (`fixtures/fake-harness/account-cli.js`) standing in for `codex`/`claude` login commands, plus a read-only check of the owner's real accounts on the updated daemon",
    steps="""1. `cargo test` — `ac46_accounts_by_provider_fixed_vs_desktop_linked_and_isolated_resign_in_and_removal`.
2. `node test/ui/scenario-accounts.js` (packaged UI; `OVERSEER_TEST_SYSTEM_HOME` points the desktop-linked logins at a fixture home; the "desktop apps" are signed in as desk1/deskclaude before Overseer starts):
   - **Add Account** provider list; try Devin.
   - Add "Work ChatGPT" (OpenAI) and sign in with the device-code flow; add "Claude fixed" (Anthropic) and sign in.
   - New Task for codex and for claude.
   - Switch the desktop Codex login to desk2.
   - Sign Out, then re-sign in "Work ChatGPT"; Remove "Claude fixed".
3. After reinstalling the VSIX and restarting the owner's daemon (no active runs), `overseerd ctl account.list` and `profile.status` for the real accounts ([live-accounts.txt](evidence/ac-46/live-accounts.txt)).""",
    expected="The RFC acceptance list: one account per available provider added through the UI; compatible-only account choice per harness; switching the desktop app's account does not change a fixed account; re-sign-in and removal affect only that account. No API keys.",
    actual="""- **Providers:** Add Account lists OpenAI / ChatGPT (codex, codex-app), Anthropic / Claude (claude), OpenCode (local models) and Devin, which is unavailable ("no account-login CLI yet (only API keys, which Overseer does not use)"; creating one is refused).
- **Adding accounts:** "Work ChatGPT" was added and signed in through the provider flow picker (browser or device code; device code used) and shows "signed in · team · c4a1579f". "Claude fixed" shows "signed in · max · f03ab3f8". Each account's credential folder exists (0700) as soon as it is created.
- **Labels:** desktop logins show "· follows app" (tooltip: follows the ChatGPT / Codex or Claude app login; Overseer never signs it out); fixed accounts do not.
- **Compatible-only choice:** New Task for codex offered only `codex (existing login)` and `Work ChatGPT`; for claude only `claude (existing login)` and `Claude fixed`. Each option is labeled "fixed account" or "follows the desktop app (can change)".
- **Desktop switch:** switching the desktop Codex login desk1 (pro) → desk2 (plus) changed only the linked account (bc92d05b → d1dddc49); "Work ChatGPT" stayed "team · c4a1579f".
- **Isolation:** Sign Out made only "Work ChatGPT" "not signed in"; re-signing in as another account changed only its fingerprint. Remove deleted "Claude fixed" and its folder only. The desktop logins were untouched, and removing or signing out a desktop login is refused. No token appears in the database.
- **Live, read-only on the owner's machine:** ChatGPT A (fixed, team, `2bb3fae1`) and ChatGPT B (fixed, plus, `27e64e3a`) are signed in with ChatGPT accounts and no API key. The desktop-linked Codex login is currently a third account (pro, `2e1fa921`), so fixed accounts keep their identity whichever account the ChatGPT app uses. The Claude desktop login is `claude.ai` (max).""",
    evidence="[accounts scenario](evidence/ui/accounts/) (screenshots: accounts by provider, New Task choices for codex and claude, after removal; result.json); [live account listing](evidence/ac-46/live-accounts.txt); `cargo test` ac46 test",
    live="Sign-in, sign-out and switching use the synthetic account CLI (no real login is touched). The real accounts were read live. Real ChatGPT sign-ins through Overseer's flows (browser for A, device code for B) happened earlier on 2026-09-25; see AC-11/AC-12.",
    limits="Signing a new fixed Anthropic account in for real needs the owner's browser login (tracked under AC-11). OpenCode accounts are local-provider folders by owner decision.")

rec(47, "Polished, theme-compatible UI", "verified", commit="d12a870", date="2026-09-25",
    harness="Synthetic account CLI (signed-in desktop Codex login), Claude fixture (nested) and generic runs; no paid tokens",
    steps="""1. `node test/ui/scenario-theme.js` (packaged UI).
   - A lint of the Overseer UI sources for hard-coded colors (hex, rgb/hsl, named).
   - Keyboard-only task creation in the New Task form.
   - For Default Dark Modern, Light Modern, High Contrast and High Contrast Light: screenshots of the New Task form, the Overseer view with review and conversation, and a nested-run conversation, plus an accessible-name audit of every visible control in each webview.
2. Reran `scenario-conversation.js` and `scenario-main.js`.""",
    expected="Designed task creation (tiles with icons, status and capability hints) and run views; consistent spacing/typography/states; theme tokens only so light, dark and high contrast look right; keyboard navigation and screen-reader labels.",
    actual="""- **New Task form:** replaces stock pickers with rounded tiles.
  - Repository: open folders and recent task repositories, plus "Choose repository…".
  - Harness: icon, version, "ready"/"missing" pill, and capability hints such as native children, approvals, follow-ups, interrupt, "filesystem-only edits".
  - Account: only compatible accounts, each with a signed-in pill, plan, id fingerprint and "Follows the desktop app login" text; accounts that are not signed in are disabled with the reason.
  - Workspace: tiles, the "recommended" pill on New worktree, and a start branch select.
  - Codex app-server: approval-policy tiles.
  - Model, task prompt, and a sticky Start button that explains why it is disabled.
- **Keyboard-only creation:** click-free after focusing the form. Tab moves between groups (repository → harness → program → arguments → workspace → … → Start); arrow keys select tiles (Codex → … → Generic program), which are role=radio in role=radiogroup; Enter on Start created the run, which wrote kb.txt. Focus stays on the chosen tile across re-renders (this was a bug on the first attempt).
- **Themes:** the body class matched each theme (vscode-dark, vscode-light, vscode-high-contrast, vscode-high-contrast-light). The screenshots show all four views following the theme. In high contrast, buttons carry contrast borders and the selected Conversation/Event log tab is outlined (fixed after the first high-contrast screenshot showed borderless buttons).
- **Accessibility audit:** 0 unnamed controls in the form (19 controls), Overseer view (10), conversation (8) and review (15) in every theme. Trees use role=tree/treeitem with aria-level/expanded/selected; tiles and hunk buttons have aria-labels.
- **Hard-coded colors:** none across the 11 Overseer UI files. The review's hard-coded fallbacks (`#73c991`, `#f48771`, `#e2c08d`, `rgba(255, 200, 0, .25)`) were replaced with theme tokens.
- **Found on the way:** the conversation ignored `child_reparented`, so a grandchild reported before its parent sat beside the child. It now nests inside the child (checked).""",
    evidence="[theme scenario](evidence/ui/theme/) (13 screenshots: keyboard New Task, then New Task / views / conversation in dark, light, high contrast, high contrast light; result.json with the lint and audits)",
    live="Fixture runs; styling and accessibility do not depend on the harness. Live runs render with the same components (AC-43/AC-44 evidence).",
    limits="Monaco's own editor colors in the review come from VS Code theme tokens read at runtime, as before. The narrowest review column squeezes the \"no changes\" text beside the file navigator (collapsible with Files).")

rec(48, "Overseer view (command center)", "verified", commit="83ed5b6", date="2026-09-25",
    harness="Claude fixture (`CLAUDE_FIXTURE_MODE=nested`: native child and grandchild) and generic runs; no paid tokens",
    steps="""1. `node test/ui/scenario-center.js` (packaged UI). The window opens `open-folder`; runs live in `repo-x` and `repo-y`, which are never opened: a nested Claude fixture run and a generic edit in X, a still-running generic edit in Y.
2. Close the primary sidebar (Cmd+B until hidden); **Overseer: Open Overseer View**.
3. Expand/collapse at repository, task and native-child level; select runs in X and Y; keyboard navigation; reload the window; emulate a 1024×760 and a 1900×1100 window.
4. Reran `scenario-restore.js` and `scenario-main.js`.""",
    expected="Works with the native sidebar closed and independent of the window's folder; agents column (tasks → runs → descendants across repositories, expand/collapse, live status); the selected run's live review to its right plus its conversation and event log; switching runs switches both without jumps; narrow and wide windows.",
    actual="""- With the primary sidebar hidden, the view opened as three editor columns (Overseer | review | conversation). It listed `repo-y` and `repo-x` but not the window's `open-folder`.
- Tree levels: repository → task → run → "child task" (level 4) → "grandchild task" (level 5). Collapsing the child hid the grandchild, collapsing the task hid its runs, collapsing `repo-y` hid its task, and expanding restored each.
- Selecting "X edits" put "Review: X edits" in column 2 (X's worktree, with its diff) and "generic: X edits" in column 3. Selecting "Y live" switched to Y's worktree review and Y's conversation.
- Live status: the Y run shows a running (pulsing) dot and repo-y a "1 active" badge.
- Keyboard: arrows move between treeitems (role/aria-level/aria-label). Left collapses a task and then moves to its repository; Right expands again. Selecting keeps focus in the agents column.
- After a window reload the view came back with the same run selected.
- 1024 px window: columns 220/463/283 px; 1900 px window: 405/884/553 px. The agents column never overflows horizontally.
- Found on the way: live re-renders dropped keyboard focus, and selecting a run moved focus into the review. Both are fixed.""",
    evidence="[center scenario](evidence/ui/center/) (screenshots: wide view, X selected, Y selected, narrow and wide windows; result.json)",
    live="Fixture and generic runs (the view reads the daemon's run tree; it is harness-independent). Live runs appear the same way (see AC-43/AC-44 screenshots).",
    limits="The three-column layout replaces the window's current editor layout when the view opens. A worktree file hierarchy in the view is AC-51.")

rec(49, "Restore the open session", "verified", commit="8103a2e", date="2026-09-25",
    steps="""1. `node extension/scripts/package.js` then `node test/ui/scenario-restore.js` (isolated VS Code profile, VSIX installed with the `code` CLI, CDP; generic-harness runs, no paid tokens).
2. Three runs: R1 (repoA worktree, long review), R3 (repoA worktree), R2 (repoB worktree, still running; repoB is never opened in the window). Select each in the Agents view so its review and run panel open.
3. On R1: choose **Since task start** from the review's comparison button, turn Follow on, scroll the review to 1200 px, scroll the run panel to 400 px and type an unsent follow-up; collapse task R2 in the Agents view.
4. `Developer: Reload Window`; check tabs, Agents view, R1 review (comparison, scroll, Follow), R1 run panel (scroll, draft), R2 review, R2 still running.
5. Remove R3's worktree (`workspace.cleanup`), quit VS Code with Cmd+Q and relaunch it with the same profile; repeat the checks and open R3's review tab.
6. Repeated 4 consecutive times (all passed) after fixing a race in the review's scroll restore; `scenario-review.js` and `scenario-main.js` rerun and pass.""",
    expected="After reload and restart the same runs, panels, comparison modes, Follow state (never auto-resumed), file and scroll positions and sidebar expansion return; a removed worktree is explained.",
    actual="""- Review and run panels for all three runs reopen after reload and after restart (webview serializers for `overseer.review` and `overseer.output`; the review now records its run id instead of guessing the newest run in that folder).
- R1's review returns with **Since task start** (per-run comparison persisted in workspace state) and scrollTop 1200 → 1200. The saved anchor is re-applied as diffs render and relayout until the user interacts, because rows start as short placeholders and a later snapshot can release rendered rows.
- Follow returns checked but **paused** with "Follow was on before VS Code reloaded. It stays paused until you resume it." — never auto-resumed.
- R1's run panel returns at scrollY 400 with the unsent follow-up draft intact.
- The Agents view keeps R1 selected and task R2 collapsed (expansion persisted per workspace).
- R2's review (repository not open in the window) returns, and R2 keeps running throughout.
- R3's review tab, after its worktree was removed, shows "Review unavailable — The worktree for "R3 removed later" (<path>) was removed on <date>. Its branch overseer/r3-removed-later was kept, so the commits are still in the repository. The run panel still has its history."
- Harness fix found on the way: Cmd+Q and the command palette were sent while focus was inside a webview, so VS Code was SIGTERM'd; `Cdp.focusWorkbench` now runs first.""",
    evidence="[restore scenario](evidence/ui/restore/) (scenario.log, result.json, screenshots before reload, after reload, after restart, removed worktree explained); `cargo test` green",
    live="Fixture runs (generic harness). Restoring is harness-independent: it uses the daemon's run/workspace records and VS Code webview state.",
    limits="Native file editors are restored by VS Code itself. Run panels show the unavailable page if the daemon is unreachable for 20 s at startup (reopen from the Agents view).")

rec(50, "Open a pull request from a run", "verified", commit="0cdd312", date="2026-09-25 (owner-confirmed live on the owner's Mac)",
    harness="codex (ChatGPT A, `p-f262c1bc4958`, gpt-5.6-luna) for the live run; the owner's VS Code GitHub sign-in (account `beelol`) for the push and the pull request",
    steps="""1. `cargo test` — `ac50_pr_plan_explains_refusals_and_prepares_a_github_branch_without_merging` (refusals, owner/repo parsing through an `insteadOf` stand-in, commit without merging, `pull_request` event, bad URLs refused) and the `pr::tests` unit test (github.com remote parsing: https, ssh, scp-like; others rejected).
2. `node test/ui/scenario-pr.js` (packaged UI; `fixtures/mock-github/server.js` as the API; a local bare repository stands in for `https://github.com/test-owner/pr-demo.git` via `url.<bare>.insteadOf`). A generic run edits a.txt and adds NOTES.md. Then **Open PR…** four times: with no remote; with a GitLab remote; with the GitHub remote while the API is real GitHub and VS Code has no GitHub session; and after pointing `overseer.github.apiUrl` at the mock (test token honored only for a loopback API). Finally open it again.
3. `node test/unit/webview-scripts.js`; reran `scenario-main.js`.
4. Live with the owner ([session](evidence/ac-50/session.md)): the owner asked for a new repository, so the agent created the private `beelol/overseer-pr-sandbox` and cloned it fresh under /tmp. One tiny Codex run added `OVERSEER.md`. The owner pressed **Open PR…** in their own VS Code, approved VS Code's GitHub sign-in and confirmed. The agent checked GitHub with read-only `gh`.""",
    expected="A PR created from a live run against a repository the owner chooses; missing remote and signed-out cases explained; no automatic merge.",
    actual="""- **Live, owner's Mac (2026-09-25):**
  - **Bugs found:** the plan targeted `origin/master` in a fresh clone (fixed in 4fa7166, with a regression test). With no run selected, Open PR did nothing (it now asks which run). With VS Code's Do Not Disturb on (as on the owner's Mac), every answer was an invisible toast: the sign-in prompt, the refusals and the result. They are dialogs now (0cdd312), and the scenario runs with Do Not Disturb on.
  - Then the owner pressed Open PR… on the run and approved VS Code's GitHub sign-in. The extension log showed "no GitHub session with repo access" first, then the PR URL. Owner: "worked! amazing".
  - [beelol/overseer-pr-sandbox#1](https://github.com/beelol/overseer-pr-sandbox/pull/1): open, not merged; head `overseer/add-overseer-pr-check-note` = the worktree HEAD; base `master` (unchanged); title "Add Overseer PR check note". It has one file (`OVERSEER.md`) and the generated description (run, task, commits, files, "never merges automatically").
  - The run recorded a `pull_request` event. Token-like strings in Overseer's database, the daemon log and the extension log: 0. `extraheader` in git config: 0.
- **Scenario (mock API):** the messages are now dialogs; the texts below are from the toast version and read the same.
- **No remote:** "Open PR is unavailable: The repository … has no Git remote. Add a GitHub remote (git remote add origin https://github.com/OWNER/REPO.git) …".
- **Non-GitHub remote:** "The remote origin (https://gitlab.example.invalid/…) is not on GitHub; Open PR only supports github.com remotes."
- **Signed out:** "Open PR uses the GitHub sign-in VS Code already has, and VS Code is not signed in to GitHub. No personal access token is needed." with **Sign in to GitHub**. Nothing was pushed or sent.
- **Signed in (mock API):** the confirmation read "test-owner/pr-demo: overseer/pr-demo-change → main / Commits 2 uncommitted worktree file(s) … first / Pushes … Nothing is merged." Confirming did the following:
  - committed the worktree and pushed the branch; the stand-in's `refs/heads/overseer/pr-demo-change` equals the worktree HEAD;
  - POSTed an authorized `/repos/test-owner/pr-demo/pulls` with head `overseer/pr-demo-change`, base `main` and title "PR demo change";
  - generated a body with "Opened by Overseer from run …", the task prompt, the commits, `` `M` a.txt `` / `` `A` NOTES.md ``, and "_Overseer never merges automatically._";
  - showed "Pull request #42 is open: …" with **Open on GitHub**, and recorded a `pull_request` event (URL and number only).
- **Opened again:** the existing PR #42 was found (422 "already exists" → looked up) instead of failing.
- **No merge, no stored token:** main never moved; the token is absent from Overseer's database, the daemon log, the mock's log, the worktree's git files and the repository's git config.
- **Found on the way:** a quote in the new button's tooltip broke the run panel's whole inline script, which rendered as an empty "Run". `test/unit/webview-scripts.js` now parses every shipped webview script, including generated inline ones.""",
    evidence="[owner session](evidence/ac-50/session.md), [GitHub checks](evidence/ac-50/github-checks.txt); [pr scenario](evidence/ui/pr/) (signed-out message, confirmation, PR opened; result.json); `cargo test` ac50 and pr::tests",
    live="Live: a real pull request on github.com from a live Codex run, with the owner's VS Code GitHub sign-in (no token pasted anywhere). Refusal cases and the reuse of an existing PR are covered with the mock API and a local stand-in remote.",
    limits="github.com remotes only (GitHub Enterprise via the `overseer.github.apiUrl` setting is untested). The first push of a branch that needs extra GitHub permissions (for example a fork) reports GitHub's refusal instead of guessing.",
    blocker="not blocked")

rec(51, "Worktree file hierarchy", "verified", commit="0496e0b", date="2026-09-25",
    steps="""1. `cargo test` — `ac51_worktree_tree_lists_one_directory_marks_changes_and_stays_inside` (directories first, `.git` hidden, A/M/D marks and per-folder counts against the task-start snapshot, `..`/absolute/`.git` paths refused, a 6,000-entry folder listed in under 2 s and capped at 5,000 with `truncated`).
2. `node test/ui/scenario-files.js` (packaged UI, generic runs, no paid tokens). The window opens `open-folder`. The runs are in `repo-x` (modify a.txt, add sub/new.txt, delete c.txt), `repo-y` (modify b.txt) and `repo-z`, a 10,000-file repository (6,000 files in `big/`, 4,000 nested under `pkg/`) where the change is `pkg/m39/x99.txt`. Steps: **Open Overseer View**; select each run; expand folders; open files; scroll the 6,000-entry folder; keyboard.
3. Reran `scenario-center.js`, `scenario-restore.js` and `scenario-theme.js`.""",
    expected="Browse and open files in worktrees of two different repositories from one window; changed files marked; large repositories stay responsive.",
    actual="""- **repo-x (not open in the window):** the Files pane lists `sub (1)`, `a.txt M`, `b.txt`, `README.md` and `c.txt D` (struck through), without `.git`. Expanding `sub` shows `sub/new.txt A` at level 2. Clicking `a.txt` opened it in the editor; its breadcrumbs point into `…/worktrees/repo-x-…/x-files/a.txt`.
- **repo-y:** selecting Y switched the pane (`b.txt M`, no `sub`), and `b.txt` opened from `…/repo-y-…/y-files/`.
- **Large repository:** from the root, `pkg (1)` points to the single deep change. Opening the 6,000-entry `big/` took 993 ms and shows "… 1000 more entries not shown". Scrolling it kept webview event-loop lag at p95 2 ms (max 5 ms). `pkg/m39/x99.txt M` was found and opened.
- **Keyboard:** items are labelled treeitems ("pkg, folder, 1 changed inside"), and arrow keys move between them.
- **Found on the way:**
  - The Files pane at first squeezed the agents list to one or two rows; the agents list now keeps up to 45% of the height.
  - The pane's ready flag was not reset when switching runs.
  - Injected clicks right after focus moves between webviews are sometimes not delivered; the scenario retries that click (a test-input quirk).""",
    evidence="[files scenario](evidence/ui/files/) (screenshots for X, Y and the large repository; result.json)",
    live="Fixture runs; the tree reads the worktree and the daemon's task-start snapshot, independent of the harness.",
    limits="Folders list at most 5,000 entries (the rest are counted). Deleted files cannot be opened (they are listed for context; the review shows their change).")

rec(52, "Native Overseer notifications (macOS)", "verified", commit="2b162cd", date="2026-09-25 (owner-confirmed live on the owner's Mac)",
    harness="generic throwaway run (`/bin/sleep`) on the owner's own daemon; no accounts",
    steps="""1. `node extension/scripts/package.js` builds `bin/Overseer Notifier.app` (`extension/notifier/build.js`: swiftc arm64 + x86_64 → lipo, Info.plist, AppKit-rendered transparent icon → .icns, `codesign -s -`).
2. `cargo test` — `ac52_notifications_use_the_overseer_helper_and_fall_back_when_denied_or_missing`, `ac52_background_notice_is_delivered_by_the_helper`, `ac52_the_daemon_finds_the_notifier_app_next_to_its_own_binary` (fake helpers; no banners).
3. `node test/ui/scenario-notify.js`:
   - Inspect the helper inside the installed VSIX: codesign, bundle id, name, archs, icon, `--status`.
   - Run **Overseer: Test Notification** with a fake helper that allows, then one that denies.
   - Open `vscode://beelol.overseer/open-center` twice with **Developer: Open URL**.
4. Live with the owner ([session](evidence/ac-52/session.md)): reload VS Code with the VSIX, run **Overseer: Test Notification**, allow Overseer when macOS asks, look at the banner and System Settings → Notifications; then start a throwaway `/bin/sleep` run with `overseerd ctl task.create`, quit VS Code, click the banner.""",
    expected="Overseer-branded banner (name, icon) when the last window closes with agents running; a click opens VS Code at the Overseer view; Overseer listed in Notifications settings; osascript fallback when denied, recorded in delivered_via; Test Notification posts a sample.",
    actual="""- **Installed helper:** the helper inside the installed extension passes `codesign --verify --deep`. It has bundle id `com.beelol.overseer.notifier`, name "Overseer", archs `x86_64 arm64`, and `AppIcon.icns` (transparent corners, checked). `notifier --status` answers `notDetermined` without prompting.
- **Allowed:** Test Notification reported "Sent a test notification from Overseer". The helper got `--title "Overseer notifications are on" --body … --open vscode://beelol.overseer/open-center`, and the fallback was not used.
- **Denied:** "Sent a test notification, but not as Overseer: overseer-notifier (denied); fell back to …/fallback.sh (ok). Allow Overseer in System Settings → Notifications…". Protocol tests also cover "permission not answered yet" (the helper gives up after 20 s instead of blocking) and "not installed".
- **Background notice:** it goes through the helper (`delivered_via: overseer-notifier (ok)`, title "Overseer: 1 agent still running"). The daemon finds `Overseer Notifier.app` next to its own binary, which is the installed layout.
- **Click link:** `vscode://beelol.overseer/open-center` opens the Overseer view. VS Code asks once ("Allow 'Overseer' extension to open this URI?", with "Do not ask me again"), and later links open it without asking.
- **Live, owner's Mac (2026-09-25):**
  - The first live try found a real bug: run directly, the helper was refused by macOS ("Notifications are not allowed for this application") without a prompt, so the daemon fell back to osascript (Script Editor icon). The daemon now launches the helper through LaunchServices (`open -n -W`, outcome read from a `--result` file; commit 2b162cd).
  - macOS then asked "“Overseer” Notifications" with the eye icon (owner screenshot). Owner: "overseer is in there listed as off", then "I allowed it"; `notifier --status` → `authorized`.
  - Test notices via the owner's daemon: `delivered_via: overseer-notifier (ok)`. Owner, asked whether the banner shows "Overseer" with the eye icon, not Script Editor: "i saw it", "yes". Before permission was granted, the osascript fallback banner (Script Editor icon) appeared and was recorded as `overseer-notifier (denied); fell back to osascript (ok)` (owner screenshot `owner-02-fallback-before-allow.png`).
  - With one throwaway run active, the owner quit VS Code. The daemon posted "Overseer: 1 agent still running" (`background_notice` event, `delivered_via: overseer-notifier (ok)`). Owner, asked whether the banner appeared and whether clicking it opened VS Code at the Overseer view: "yes to both. for 1. it took like 5 seconds though". The run was then stopped.""",
    evidence="[owner session](evidence/ac-52/session.md), [daemon checks](evidence/ac-52/daemon-checks.txt), owner screenshots (evidence/ac-52/owner-*.png); [notify scenario](evidence/ui/notify/) (toasts, VS Code's URI prompt, Overseer view opened; result.json); `cargo test` ac52_* tests",
    live="Live on the owner's Mac: real permission prompt, real banners, System Settings entry and banner click (owner-confirmed). Automated tests use fake helpers so no banner or permission prompt appears during automation.",
    limits="Ad-hoc signed, not notarized: from a downloaded VSIX Gatekeeper may warn once; Developer ID signing belongs with release packaging. VS Code asks once before Overseer handles its vscode:// link.",
    blocker="not blocked")

rec(53, "Fixed Claude accounts", "partial", date="2026-09-25",
    proven="the design for keeping each Claude account's credentials separate is written (docs/rfcs/claude-credentials.md: check per-folder Keychain entries first, otherwise Overseer-managed credentials); the account flows it builds on (Add Account → Anthropic → Sign In with its own CLAUDE_CONFIG_DIR, sign-out, expiry and Sign in again) pass with the synthetic account CLI",
    deferred="a live test with a second Claude account (the owner asked not to test Claude yet, and has one Claude account)",
    expected="See the RFC criterion and the [Claude credentials RFC](../rfcs/claude-credentials.md) (moved out of AC-11/AC-13 by the owner on 2026-09-25).",
    actual="Not verified live. The flows exist (Add Account → Anthropic → Sign In runs `claude auth login` with the account's own `CLAUDE_CONFIG_DIR`), and they pass with the synthetic account CLI (AC-11 sign-in, expiry and Sign in again; AC-46 isolation). The owner's only Claude login is the desktop one (`claude (existing login)`, max), which is never signed out.",
    evidence="[signin scenario](evidence/ui/signin/), [accounts scenario](evidence/ui/accounts/)", live="—",
    blocker="Needs a second Claude account (the owner has one today); not to be tested yet (owner, 2026-09-25). Next: check whether Claude keeps a separate Keychain entry per CLAUDE_CONFIG_DIR, otherwise add Overseer-managed Claude credentials (docs/rfcs/claude-credentials.md); then Add Account → Anthropic → Sign In with it, Sign Out and Sign In again while a Claude run on the desktop login keeps working; confirm both identities and the macOS Keychain entries stay separate.")

# Gate J (added by the owner on 2026-09-26; docs/rfcs/orchestrator-ui.md).
FIX = "Fixture harnesses only (Claude fixture, synthetic account CLI, generic programs); no paid tokens"
rec(54, "Clean, calm presentation with less text", "verified", commit="8bcac2f", date="2026-09-26",
    harness=FIX,
    steps="""1. `AUDIT_UI=baseline node test/ui/scenario-audit.js` with a VSIX built from ce56432 (the UI before Gate J), then `AUDIT_UI=new node test/ui/scenario-audit.js` with the Gate J VSIX, on the same fixture state (runs in two repositories, a nested Claude run, a permission request, a failure, a streaming run).
2. Each view (agents, chat, files, review, new agent, accounts; plus dashboard and grid for the new UI) at 1280 and 900 px, in Overseer Dark, Overseer Light and Default Dark Modern (baseline: Dark and Light Modern). The audit (`test/ui/audit.js`) counts visible text outside the Monaco diff, fails on horizontal overflow, on text runs over 80 characters outside code, and on icon-only controls without an accessible name or tooltip.
3. The odd-looking items in the baseline were listed and fixed ([docs/design/audit.md](../design/audit.md)); the other Gate J scenarios show no function was lost (chat, composer, dashboard, grid, keyboard, history, usage, accounts, review).""",
    expected="No overflow, no long runs outside code, every icon-only control named; at least 40% less visible text per view than the baseline with no function lost; before/after screenshots; the odd-looking items listed and fixed.",
    actual="""- **Visible text (characters, same fixture state):** agents 531 → 238 (−55%), chat 2,795 → 1,063 (−62%), files 127 → 59 (−54%), review 350 → 175 (−50%), new agent 1,037 → 133 (−87%), accounts 381 → 189 (−50%). The counts are the same at both widths and in every theme.
- **Checks:** no overflow, no long runs and no unnamed icon controls in any view, width or theme (the new dashboard and grid included). The baseline new-task form had 68 overflowing elements at 900 px.
- **Odd-looking items:** 15 found and fixed (full paths, run metadata sentences, five-button rows, raw Markdown, JSON tool calls, duplicate task/run rows, competing pills, the card-page New Task form, long account labels, a separate follow-up button, status-bar text, truncated headers, mixed disclosure glyphs, a crowded review header); see [audit.md](../design/audit.md).""",
    evidence="[baseline audit](evidence/ui/audit-baseline/) (12 screenshots, result.json), [Gate J audit](evidence/ui/audit/) (24 screenshots, result.json), [audit list](../design/audit.md)",
    live="Presentation does not depend on the harness; live runs render with the same components (AC-55 live screenshots).",
    limits="Two items are still open for the owner's review: in the narrow review diff column the hunk Accept/Revert buttons overlap the start of the code line, and at 1280 px with the file list open the chat header shortens the title. Grid tile titles shorten to a letter or two at 3×3 in a 1280 px window.",
    blocker="not blocked")

rec(56, "Overseer themes, light and dark", "verified", commit="3f8c0f9", date="2026-09-26",
    harness=FIX,
    steps="""1. `node extension/design/build-themes.js` generates `themes/overseer-dark-color-theme.json`, `themes/overseer-light-color-theme.json` and `media/tokens.css` from one token set (`extension/design/tokens.js`).
2. `node test/unit/theme-contrast.js`: every text foreground/background pair both themes define (workbench, editor, diff, terminal, notifications, lists, inputs, buttons, badges, syntax, ANSI) against WCAG AA.
3. `node test/ui/scenario-look.js`: the dashboard with a chat and a diff, a terminal printing ANSI colors, the grid and the composer menu in Overseer Dark, Overseer Light and High Contrast; switch themes with Overseer views open.
4. `node test/ui/scenario-theme.js`: the lint for hard-coded colors in webview sources and the accessible-name audit in stock themes.""",
    expected="WCAG AA for every text pair in both themes; screenshots of the dashboard, a diff and a terminal in both themes; open Overseer views restyle live when the theme changes.",
    actual="""- **Contrast:** 160 text pairs checked, 0 below AA (normal text 4.5:1, large and UI text 3:1).
- **Screenshots:** dashboard + diff, terminal and grid in Overseer Dark, Overseer Light and High Contrast.
- **Live switch:** the open dashboard's background changed from rgb(23, 22, 29) to rgb(251, 250, 253) when the theme changed, with no reload.
- **Stock themes:** Overseer views use only VS Code theme variables (the lint found no hard-coded colors), so they follow Dark/Light Modern and both High Contrast themes (theme scenario).""",
    evidence="[look scenario](evidence/ui/look/) (10 screenshots, result.json), [theme scenario](evidence/ui/theme/), `test/unit/theme-contrast.js` output",
    live="Themes do not depend on the harness.",
    limits="The themes are optional; Overseer never switches the user's theme.",
    blocker="not blocked")

rec(57, "Overseer dashboard", "verified", commit="3f8c0f9", date="2026-09-26",
    harness=FIX,
    steps="""`node test/ui/scenario-dashboard.js`: a window with no folder, the Explorer side bar, a terminal panel and two editor groups; **Overseer: Open Dashboard**; reload the window; **Overseer: Exit Dashboard**; compare the layout and user settings before and after; then set `overseer.dashboard.openOnStartup` and reload.""",
    expected="Entering and exiting restores the prior layout; the dashboard survives a reload; it works in a window without a folder; no setting changes unless the user opts in.",
    actual="""- **Enter:** side bar, panel and secondary side bar hidden; the dashboard fills the editor area and lists agents from a repository that is not open in the window.
- **Reload:** after Developer: Reload Window the dashboard is back and still in dashboard mode (parts still hidden).
- **Exit:** side bar, panel, secondary side bar and both editor groups restored (421×468 before, 421×469 after).
- **Settings:** the user settings file is identical before and after (apart from VS Code's own migration of `extensions.autoUpdate`). Dashboard mode detects which parts were open by measuring its own webview, so it writes no settings.
- **Open on startup:** with `overseer.dashboard.openOnStartup` the dashboard opens when the window starts.""",
    evidence="[dashboard scenario](evidence/ui/dashboard/) (before, dashboard, after exit, open on startup; result.json)",
    live="Layout does not depend on the harness.",
    limits="VS Code gives extensions no way to hide the minimap or breadcrumbs without changing settings, so dashboard mode leaves them as they are.",
    blocker="not blocked")

rec(58, "Agent grid", "partial", commit="8bcac2f", date="2026-09-26",
    proven="nine concurrent fixture streams tile 3×3 and a maximum of 4 tiles 2×2; a permission request is answered from its tile; a pinned finished run stays; arrow keys move between tiles and Enter opens the agent; webview event-loop lag p95 2 ms; screenshots at 4 and 9 tiles in both themes",
    deferred="the per-tile update time: a streamed line reaches its tile in 853 ms at p95 (target 250 ms); the daemon records the same lines within 62 ms p95, so the delay is between the daemon and the webview",
    harness=FIX,
    steps="""`node test/ui/scenario-grid.js` with `overseer.grid.maxTiles` 9: a pinned finished run, a Claude fixture waiting for permission, and seven generic runs printing a millisecond timestamp every 200 ms. A MutationObserver in the webview measures, for each new tile line, now − printed time; a 25 ms timer measures event-loop lag. Then Allow from the tile, maximum 4, arrow keys and Enter. Separately, the same seven streams on an isolated daemon, comparing each event's recorded time with the printed time.""",
    expected="Each tile updates within 250 ms of its event with event-loop lag p95 under 50 ms; permission answered from a tile; pinned finished run stays; screenshots at 4 and 9 tiles in both themes.",
    actual="""- **Layout:** 9 tiles as 3×3; with a maximum of 4, 2×2.
- **Timing:** 346 lines measured; tile latency p95 853 ms, max 2,279 ms; event-loop lag p95 2 ms. On the daemon alone the same streams are recorded within 34 ms median, 62 ms p95.
- **Found and fixed on the way:** the extension's state refresh was a trailing debounce that never fired while events streamed, so new runs did not appear on the grid; it now refreshes at most every 120 ms and skips output-only events.
- **Permission:** Allow on the Claude tile → the agent continued and wrote perm.txt.
- **Pinned:** the finished, pinned run stayed with its pin pressed.
- **Keyboard:** Right moved to the next tile; Enter opened that agent in the chat.""",
    evidence="[grid scenario](evidence/ui/grid/) (9 and 4 tiles, dark and light; scenario.log with the latency numbers)",
    live="Fixture streams; the grid uses the same feed as live runs.",
    limits="Tile titles shorten to a letter or two at 3×3 in a 1280 px window.",
    blocker="Per-tile latency 853 ms p95 vs 250 ms. Next: time each hop in the extension host (daemon socket → RunFeed batch → postMessage) and the tile renderer, and remove the slow hop.")

rec(59, "Start a new agent from the chat", "partial", commit="3f8c0f9", date="2026-09-26",
    proven="with no agent selected the middle is the composer; Claude and Codex agents start keyboard-only and stream in place as the selected agent; a signed-out account is shown inline with Sign in and Start disabled; a harness that is not installed is labelled so; the Full form link stays",
    deferred="a generic program started keyboard-only (choosing Run a program from the agent menu left the chip on Codex) and defaults remembered across a reload (the composer did not finish loading after the reload in the scenario)",
    harness="Synthetic account CLI standing in for codex and claude; generic /bin/echo",
    steps="""`node test/ui/scenario-composer.js`: open the dashboard with no agent selected; for Claude, Codex and a generic program, pick the agent from the agent chip's menu with the keyboard, type the task and press Enter; pick a signed-out account; open the agent menu for OpenCode (not installed); reload and check the remembered defaults.""",
    expected="Codex, Claude and generic runs started keyboard-only from the composer; defaults remembered across reloads; each problem case shown inline; the full New Task form reachable.",
    actual="""- **Claude and Codex:** started from the composer with the keyboard; each became the selected agent and streamed in place.
- **Problems inline:** "ChatGPT Signed Out is not signed in. Sign in", Start disabled; the agent menu heads OpenCode with "not installed". A missing harness path now reads as not installed in the daemon too (it used to be reported as installed).
- **Default agent:** with nothing remembered, the composer now prefers a harness that has a signed-in account.
- **Not yet:** choosing **Run a program** by keyboard did not switch the agent chip, and after a reload the composer stayed in its loading state in the scenario.""",
    evidence="[composer scenario](evidence/ui/composer/)",
    live="Fixture accounts; live starts through the same path are in the AC-60 live session.",
    limits="—",
    blocker="Keyboard selection of Run a program and remembered defaults after reload. Next: fix the agent-menu keyboard pick for the generic entry and the composer's reload state, then rerun scenario-composer.js.")

rec(61, "Needs-you inbox and keyboard control", "verified", commit="3f8c0f9", date="2026-09-26",
    harness="Claude fixture (two permission requests), generic runs (a failure, a finished edit, a long loop)",
    steps="""`node test/ui/scenario-keyboard.js`: four runs need the user; then, with the keyboard only, ⌥⌘J (next that needs you), ⌥⌘Y (allow), ⌥⌘J, ⌥⌘⌫ (deny), ⌥⌘J until Needs you is empty, ⌥⌘A (switch agent, searchable), ⌥⌘. (stop), ⌥⌘N (new agent) and Enter, a follow-up with Enter; then an accessible-name audit of every visible control.""",
    expected="A scripted keyboard-only session over three or more concurrent runs answers permissions, reviews changes and sends follow-ups without a click; all controls have screen-reader labels.",
    actual="""- **Needs you:** 2 × Approve, 1 × Failed, 1 × Review, badge 4; the status bar reads "Overseer 3 active" with a bell and 4.
- **Keyboard:** ⌥⌘J selected the first permission request; ⌥⌘Y allowed it (completed); ⌥⌘J moved to the second; ⌥⌘⌫ denied it (`permission_answered` allow=false). ⌥⌘J then visited the failure, the allowed run (now a Review, since it wrote a file) and the finished edit; Needs you emptied.
- **Switch and stop:** ⌥⌘A → "Long loop" → ⌥⌘. interrupted it.
- **New agent and follow-up:** ⌥⌘N focused the composer; typing and Enter started an agent that became selected; Enter in the chat sent a follow-up (2 turns).
- **Labels:** 57 controls checked, none without a name.
- **Fixed on the way:** ⌥⌘J now goes to the most urgent item that is not already open (it used to cycle past items).""",
    evidence="[keyboard scenario](evidence/ui/keyboard/)",
    live="Fixture runs; permissions from live Claude and Codex app-server use the same path (AC-43).",
    limits="The follow-up step focuses the chat's prompt field from the test before typing (keyboard focus lands there after switching in normal use).",
    blocker="not blocked")

rec(63, "History that stays tidy", "verified", commit="3f8c0f9", date="2026-09-26",
    harness="300 generic runs in worktrees, each printing a unique word",
    steps="""1. `cargo test` — `ac63_search_finds_tasks_by_title_output_and_status_and_archive_hides_without_deleting` (output text, status, LIKE wildcards taken literally, archived tasks stay searchable, archive never deletes, restore).
2. `node test/ui/scenario-history.js`: create 300 finished runs; search by title and by a word only in one run's output; archive a finished run from the rail with Delete and restore it under Show archived; archive three runs (one with uncommitted work) and run **Clean Up Archived Worktrees…** twice; set `overseer.history.autoArchiveDays` and reload.""",
    expected="With 300 runs, search answers in under 200 ms; archive, restore and bulk cleanup never discard unmerged work without confirmation.",
    actual="""- **Search:** by title in 2 ms, by output text in 69 ms (300 runs).
- **Archive:** hidden from the rail, listed under Show archived, restored with Delete.
- **Bulk cleanup:** the dialog lists 2 clean worktrees and 1 with uncommitted work; "Remove 2 Clean" removed only the clean ones; the file in the third stayed; branches kept. "Remove All, Discarding Uncommitted Work" asked again ("Discard and Remove") before removing it; its branch stayed.
- **Automatic archive:** after the chosen age all 300 finished runs were archived after a reload and the rail showed only active and recent runs.
- **Fixed on the way:** UI tests now use VS Code's in-window dialogs (`window.dialogStyle: custom`); the native macOS dialog is invisible to them.""",
    evidence="[history scenario](evidence/ui/history/), `cargo test` ac63 tests",
    live="Fixture runs.",
    limits="Search also matches prompts, file activity, repository paths and account names (the daemon's search query covers them); only title, output text and status are asserted in tests.",
    blocker="not blocked")

rec(65, "Provider logos", "verified", commit="3f8c0f9", date="2026-09-26",
    harness=FIX,
    steps="""1. `node extension/design/build-logos.js` builds monochrome `currentColor` SVGs from Simple Icons (CC0: Claude, Claude Code, Anthropic, OpenCode, GitHub) and LobeHub Icons (MIT: OpenAI, Codex).
2. `node test/ui/scenario-look.js`: check the installed VSIX for NOTICE.md and each license; collect the logo on agent rows, the chat header, the composer chip and agent menu, grid tiles and the Accounts view; screenshots in Overseer Dark, Overseer Light and High Contrast.""",
    expected="The license and source of every bundled logo in a notices file shipped in the VSIX; screenshots of each place a logo appears in both Overseer themes and high contrast.",
    actual="""- **Notices:** the VSIX ships `NOTICE.md` (source, version and license per logo), `media/vendor/licenses/simple-icons-LICENSE.md` and `lobehub-icons-LICENSE.txt`.
- **Places:** agent rows (codex, claudecode), chat header (claudecode), composer chip (codex) and menu (codex, claudecode, opencode), Accounts view (openai, claude, opencode, light and dark variants), grid tiles (the Claude tile shows the Claude Code mark; generic tiles keep the terminal codicon).
- **Themes:** logos are monochrome and follow the text color, so they read in Overseer Dark, Light and High Contrast (screenshots). Every other icon is a codicon.""",
    evidence="[look scenario](evidence/ui/look/), [grid scenario](evidence/ui/grid/), `extension/NOTICE.md`",
    live="Logos do not depend on the harness.",
    limits="No suitably licensed Devin logo was needed (Devin is unavailable). Brand guidelines: marks are used only to identify the provider, unmodified apart from color.",
    blocker="not blocked")

rec(55, "A chat that feels great", "verified", commit="8bcac2f", date="2026-09-26",
    harness="Claude fixture (showcase: Markdown, table, code, six tool calls, an edit) and generic streams; LIVE Claude Code (existing login, haiku) and Codex (gpt-5.6-luna, ChatGPT A) on the owner's daemon",
    steps="""1. `node test/ui/scenario-chat.js` (packaged UI): the showcase conversation at 1600 and 900 px in Overseer Dark and Light; layout of bubbles and the column; Markdown; tool rows; Jump to latest; a 2,000-line stream (positions of the first 50 messages sampled while it streams); a 2,000-event conversation scrolled for frame times; 20 appended events timed.
2. `node test/ui/scenario-live-gatej.js` (LIVE, owner's daemon, tiny prompts): Claude and Codex runs with an image, a mentioned file, a stopped turn and a resumed turn; each chat captured in Overseer Dark and Light at 1600 and 900 px.""",
    expected="Live Claude Code and Codex runs and fixture runs rendered in light and dark at 900 and 1600 px; Markdown with code, tables and long lines correct; no layout shift while streaming; a 2,000-event conversation scrolls with p95 frame time under 16 ms and appends a new event in under 100 ms.",
    actual="""- **Layout:** the user's message is a bubble on the right; agent replies are plain text in a centered column (720 px wide, or the available width when narrower).
- **Markdown:** heading, list, table (3 rows), highlighted `ts` code with its language and Copy, a link with its URL, and a long path shortened with the full path in the tooltip.
- **Tools:** six consecutive tool calls fold into "6 steps · Read · Searched · …"; expanded they read "Read README.md", "Ran npm test -- --grep …", "Created session-refresh-coordinator.ts +8 −0"; no raw JSON. Edits are chips; the turn ends with a quiet footer (status, time, tokens, cost).
- **Scrolling:** scrolled up, Jump to latest appears and returns to the newest message.
- **Streaming:** 0 of the first 50 messages moved while 2,000 lines streamed.
- **Performance:** 2,000-event conversation: frame p95 9.6 ms (median 8.3, max 10); appending takes 2.7 ms per event.
- **Live:** Claude's reply "…the image is red, and the first line of README.md is "# fixture"." and Codex's "Red; # fixture" render with their steps, stop and resume turns, in both themes at both widths (screenshots).""",
    evidence="[chat scenario](evidence/ui/chat/) (4 screenshots, result.json), [live session](evidence/ui/live-gatej/) (8 live chat screenshots)",
    live="Live Claude Code (haiku) and Codex (gpt-5.6-luna) runs on the owner's daemon, 2026-09-26.",
    limits="A stopped Claude turn shows an empty Error card and \"Failed\" in its footer beside \"Stopped\" (the daemon now records the turn as interrupted); one \"Unparsed output\" row appears after a Claude reply. Both are for the owner's review.",
    blocker="not blocked")

rec(60, "Native-CLI parity for everyday use", "partial", commit="8bcac2f", date="2026-09-26",
    proven="live Claude Code and Codex runs take model, reasoning effort and permission mode per turn (argv from each run's launch record), an attached image and a mentioned worktree file reach the agent (replies name the red color and README.md's first line), a running turn is stopped and the next message answered, and finished runs continue their session after the daemon restarts; support per harness is in docs/compatibility.md",
    deferred="the same capabilities driven from the chat composer in the packaged UI: after the options menu closes, Enter does not send, so the paste, @-mention, options, queue and ⌥Enter checks in scenario-parity.js fail; the live turns used the daemon API the composer calls",
    harness="LIVE Claude Code 2.1.x (existing login, haiku) and Codex (gpt-5.6-luna) on ChatGPT A, owner's daemon; Claude fixture (echo, slow) for the packaged-UI scenario",
    steps="""1. `cargo test` — `ac60_turn_options_reach_claude_and_unsupported_ones_are_refused`, `ac60_repo_files_lists_mentionable_files_best_first`, `ac60_an_interrupted_claude_turn_is_interrupted_not_failed`, and the adapter tests for argv per harness.
2. `node test/ui/scenario-live-gatej.js` (LIVE, owner's daemon after checking no runs were active and no window connected): per harness, a first turn; a follow-up with a 32×32 red PNG, "…first line of README.md?" naming the file, effort low and Plan only / Read only; a 60-item list interrupted after 3 s, then "Stop. Reply with exactly: stopped ok"; then `daemon.shutdown`, **Overseer: Start Daemon** and "Reply with exactly: resumed ok".
3. `node test/ui/scenario-parity.js` (packaged UI, Claude echo fixture): paste, @-mention popup, options menu, queue while working, ⌥Enter.""",
    expected="Tiny live Claude Code and Codex runs exercise each capability; support per harness recorded in docs/compatibility.md; a pasted image and an @-mentioned file demonstrably reach the agent (its reply refers to their content).",
    actual="""- **Options per turn (live):** Claude argv `--model haiku --resume <session> --effort low --permission-mode plan`; Codex argv `exec resume <thread> … -c sandbox_mode="read-only" -m gpt-5.6-luna -c model_reasoning_effort="low" -i <attachment>`.
- **Image and file (live):** Claude: "the image is red, and the first line of README.md is "# fixture""; Codex: "Red; # fixture". Attachments are stored in the run folder with mode 0600.
- **First pass found two problems:** the test image was a corrupt PNG (bad IDAT checksum), which both agents correctly reported ("No image was actually attached", "corrupted attachment"); and a stopped Claude turn was recorded as failed. The image was replaced and the daemon now records an interrupted Claude turn as interrupted (new protocol test). Pass 1 is kept in [live-pass1](evidence/gate-j/live-pass1/).
- **Stop and send (live):** turn 3 interrupted, turn 4 answered "stopped ok", for both harnesses.
- **Resume (live):** after the daemon restarted, both runs continued their session (same session id) and replied "resumed ok".
- **Packaged UI:** the paste shows an image chip and the @-mention popup inserts README.md, but after choosing options Enter does not send.""",
    evidence="[live session](evidence/ui/live-gatej/) (result.json with argv and replies), [pass 1](evidence/gate-j/live-pass1/), [parity scenario](evidence/ui/parity/), [compatibility](../compatibility.md#everyday-parity-ac-60), `cargo test` ac60 tests",
    live="Live on the owner's daemon, 2026-09-26: five tiny turns per harness per pass, one attempt per step; two passes (the second after replacing the corrupt test image), plus one turn per pass on ChatGPT B.",
    limits="Queueing a message while the agent works is done by the extension (it sends when the turn ends); it is covered only by the packaged-UI scenario, which does not pass yet.",
    blocker="Composer keyboard focus after the options menu (and the queue display). Next: keep focus in the prompt when the options menu closes, then rerun scenario-parity.js.")

rec(62, "Usage and limits", "partial", commit="8bcac2f", date="2026-09-26",
    proven="Claude's live usage (5 hours 17%, week 48%, reset times) is exactly its own rate_limit_event; both ChatGPT accounts report plan and usage from Codex's session log (ChatGPT A team 0%/0%, ChatGPT B plus 0%/16%); OpenCode says not reported; tokens and cost per turn are shown; near-limit warning with fixtures",
    deferred="an independent check of the Codex numbers against the raw token_count line in each account's session log (those logs sit in the account folders next to the credentials, which this session does not read)",
    harness="LIVE Claude Code (existing login, haiku) and Codex (ChatGPT A and B, gpt-5.6-luna) on the owner's daemon; Claude fixture limits modes and synthetic Codex session log for the packaged-UI scenario",
    steps="""1. `cargo test` — `ac62_account_usage_is_what_the_harness_reports` and the `usage.rs` unit tests `codex_session_log_limits_are_read_from_the_newest_token_count`, `claude_rate_limit_info_is_normalized`.
2. `node test/ui/scenario-usage.js` (packaged UI): a Claude account at 95% of 5 hours and another at 12%, a Codex account at 20%; the accounts menu, the chat footer, the composer's warning and the Accounts view.
3. `node test/ui/scenario-live-gatej.js` (LIVE): after the runs, `account.usage` for claude (existing login), ChatGPT A, ChatGPT B and OpenCode; Claude's last `rate_limit_event` read from the run's raw output and compared window by window.""",
    expected="Live Codex and Claude runs show the usage their harness reports (or \"not reported\"), matching the harness's own output; the near-limit warning with fixtures.",
    actual="""- **Claude (live):** `account.usage` → 5 hours 0.17 (resets 1790429400000), week 0.48 (resets 1790568000000); the raw event has five_hour utilization 0.17 / resetsAt 1790429400 and seven_day 0.48 / 1790568000. Match.
- **Codex (live):** ChatGPT A: plan team, 5 hours 0%, week 0%; ChatGPT B: plan plus, 5 hours 0%, week 16% (source: Codex session log). Codex's streamed `exec --json` output carries no limits; its session log is the only place it reports them.
- **OpenCode:** not reported.
- **Fixtures:** the accounts menu shows "5 hours 95% · week 40%" with reset times in the tooltip; the chat footer shows "20k tokens · $0.04" (18,423 in · 1,204 out · 9,321 cached · $0.0412); starting on the 95% account warns "…is at 95% of its 5 hours limit (resets 4:09 AM)" with "Use Claude Second" and does not block; the Accounts view marks the account.""",
    evidence="[live session](evidence/ui/live-gatej/) (result.json, accounts menu screenshot), [usage scenario](evidence/ui/usage/), `cargo test` ac62 and usage tests",
    live="Live Claude and Codex (both ChatGPT accounts) on the owner's daemon, 2026-09-26.",
    limits="Codex limits appear after a Codex run writes its session log; before that the account says not reported.",
    blocker="Independent Codex comparison. Next: have the daemon include the raw token_count line it read in account.usage (no credentials), then compare it in the live scenario.")

rec(64, "Default-to-Overseer session (owner-confirmed)", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate J) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md).",
    actual="Not started.", live="—", blocker="Owner action after the design review (AC-66): work for an hour using only Overseer for Claude Code and Codex; log friction.")
rec(66, "Design review against references (owner-confirmed)", "partial", commit="5b41948", date="2026-09-26",
    proven="the references are studied and what Overseer adopts is written down (docs/design/references.md, the RFC's \"What Overseer adopts\"); the review page shows every view before and after in both Overseer themes and a stock theme, plus the new views (chat, live chats, grid, dashboard mode, composer, Needs you, history, usage, themes and logos), with a Looks right / Needs work mark and a note per view saved for the owner",
    deferred="the owner's marks, the changes they ask for, and the owner's dated confirmation that the UI looks clean and polished",
    harness="—",
    steps="""1. Study the RFC's references (web, read-only) and record what Overseer adopts ([references](../design/references.md), [RFC](../rfcs/orchestrator-ui.md#what-overseer-adopts-research-2026-09-26)).
2. Publish the review page (a private claude.ai artifact, https://claude.ai/artifact/BX8zPJfgpAvtFvH5FEdXss) from the audit, scenario and live-session screenshots. Marks are stored in the page's own database (`marks/<view>`).""",
    expected="The reference notes, the review page(s), the owner's marked items with their outcomes, and the owner's dated confirmation.",
    actual="Reference notes and the review page exist. No marks yet (the owner reviews in the morning of 2026-09-26).",
    evidence="[references](../design/references.md), [audit list](../design/audit.md), review page (private artifact linked above)",
    live="—",
    blocker="Waiting for the owner's marks. Next: read the marks from the page, change each marked item, republish the page and ask for confirmation.")

HEAD = """# AC-{n:02d} — {title}
Status: {status}{partial}
Tested implementation commit: {commit}
Verification date and verifier: {date}, implementing agent (Claude Code)
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
        partial = ""
        if status.startswith("partial"):
            partial = f"\nPartial evidence — proven: {r.get('proven', '—')}\nPartial evidence — deferred: {r.get('deferred', '—')}"
        text = HEAD.format(n=n, title=r["title"], status=status, partial=partial, date=r.get("date", "2026-09-24/25"), commit=r.get("commit", COMMIT), env=ENV, harn=HARN,
                           harness=r.get("harness", "not applicable (fixture harnesses; no accounts)"),
                           fixture=r.get("fixture", "Real Git repositories created per test under /tmp; isolated OVERSEER_HOME; isolated VS Code profile for UI scenarios"),
                           steps=r.get("steps", "—"), expected=r["expected"], actual=r["actual"].replace("__PERFRESULT__", perf_text),
                           evidence=r.get("evidence", "—"), live=r.get("live", "—"), limits=r.get("limits", "macOS only; Linux belongs to AC-41."),
                           blocker=r.get("blocker", "not blocked"))
        (out / f"AC-{n:02d}.md").write_text(text)
    print(len(R), "records written")
    sync(out)



# Gate K (added by the owner on 2026-09-26; docs/rfcs/orchestrator-ui.md#gate-k-layout). Not started.
rec(67, "One agents list: the native side bar", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(68, "Provider logos in the side bar", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(69, "Search and filter in the side bar", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(70, "Quiet row actions", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(71, "Take an agent out", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(72, "Chat in the middle when there is nothing to review", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(73, "Changes bring the diff forward", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(74, "Follow or manual review", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(75, "One place for changes", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(76, "Review that stays clean at any width", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(77, "Chat that works beside a diff", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(78, "Quiet turn endings", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(79, "Grid and dashboard mode in the new layout", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(80, "Remembered place", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(81, "Gate J still holds", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")
rec(82, "Gate K design review (owner-confirmed)", "not started", date="—", commit="—",
    expected="See the RFC criterion (Gate K) and the [orchestrator UI RFC](../rfcs/orchestrator-ui.md#gate-k-layout).",
    actual="Not started.", live="—", blocker="Not started (added by the owner on 2026-09-26; built in its own pull request).")

SHORT_BLOCKERS = {
    8: "blocked: rejecting a different local user was never exercised (needs a second macOS account)",
    12: "not yet run: ChatGPT A and B are signed in; concurrent A/B tasks pending",
    41: "deferred: no Linux environment",
    42: "not started (added by the owner on 2026-09-25)",
    43: "not started (added by the owner on 2026-09-25)",
    44: "not started (added by the owner on 2026-09-25)",
    45: "not started (added by the owner on 2026-09-25)",
    46: "not started (added by the owner on 2026-09-25; see docs/rfcs/account-governance.md)",
    47: "not started (added by the owner on 2026-09-25)",
    48: "not started (added by the owner on 2026-09-25)",
    49: "not started (added by the owner on 2026-09-25)",
    51: "not started (added by the owner on 2026-09-25)",
    53: "not started (needs a second Claude account)",
    54: "not started (added by the owner on 2026-09-26)",
    55: "not started (added by the owner on 2026-09-26)",
    56: "not started (added by the owner on 2026-09-26)",
    57: "not started (added by the owner on 2026-09-26)",
    58: "not started (added by the owner on 2026-09-26)",
    59: "not started (added by the owner on 2026-09-26)",
    60: "not started (added by the owner on 2026-09-26)",
    61: "not started (added by the owner on 2026-09-26)",
    62: "not started (added by the owner on 2026-09-26)",
    63: "not started (added by the owner on 2026-09-26)",
    64: "owner session after the rest of Gate J",
    65: "not started (added by the owner on 2026-09-26)",
    66: "owner design review after the Gate J build",
    67: "not started (Gate K, added by the owner on 2026-09-26)",
    68: "not started (Gate K, added by the owner on 2026-09-26)",
    69: "not started (Gate K, added by the owner on 2026-09-26)",
    70: "not started (Gate K, added by the owner on 2026-09-26)",
    71: "not started (Gate K, added by the owner on 2026-09-26)",
    72: "not started (Gate K, added by the owner on 2026-09-26)",
    73: "not started (Gate K, added by the owner on 2026-09-26)",
    74: "not started (Gate K, added by the owner on 2026-09-26)",
    75: "not started (Gate K, added by the owner on 2026-09-26)",
    76: "not started (Gate K, added by the owner on 2026-09-26)",
    77: "not started (Gate K, added by the owner on 2026-09-26)",
    78: "not started (Gate K, added by the owner on 2026-09-26)",
    79: "not started (Gate K, added by the owner on 2026-09-26)",
    80: "not started (Gate K, added by the owner on 2026-09-26)",
    81: "not started (Gate K, added by the owner on 2026-09-26)",
    82: "not started (Gate K, added by the owner on 2026-09-26)",
}
TOTAL = 53

EXTRA_FOLLOWUPS = [
    "Decide a retention policy for snapshot refs under `refs/overseer/snapshots/*` (they accumulate per turn; harmless but unbounded). Clearly labeled follow-up; no AC covers it.",
    "Decide whether the *existing login* Codex profile should be discouraged: on this machine `~/.codex` is shared with the ChatGPT desktop app and switched accounts during the session (see [AC-02](docs/verification/AC-02.md)). Clearly labeled follow-up.",
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
    global TOTAL
    TOTAL = max(int(n) for n in titles)
    statuses = {}
    for n in range(1, TOTAL + 1):
        f = out / f"AC-{n:02d}.md"
        statuses[n] = status_of(f) if f.exists() else "not started"
    verified = [n for n, st in statuses.items() if st.startswith("verified")]
    partials = [n for n, st in statuses.items() if st.startswith("partial")]

    def partial_line(n, key):
        f = out / f"AC-{n:02d}.md"
        prefix = f"Partial evidence — {key}:"
        return next((l[len(prefix):].strip() for l in f.read_text().splitlines() if l.startswith(prefix)), "—") if f.exists() else "—"
    for n in range(1, TOTAL + 1):
        box = "[x]" if n in verified else "[ ]"
        text = re.sub(r"- \[[ x]\] \*\*AC-%02d " % n, f"- {box} **AC-{n:02d} ", text)
    rfc.write_text(text)
    rows = ["| AC | Criterion | Status | Record |", "| --- | --- | --- | --- |"]
    for n in range(1, TOTAL + 1):
        rows.append(f"| AC-{n:02d} | {titles.get(f'{n:02d}', '')} | {statuses[n]} | [AC-{n:02d}.md](AC-{n:02d}.md) |")
    unverified = [n for n in range(1, TOTAL + 1) if n not in verified]
    audit = [f"- Verified: {len(verified)} / {TOTAL} ({', '.join(f'AC-{n:02d}' for n in verified)}).",
             f"- Partial (box unchecked): {len(partials)} ({', '.join(f'AC-{n:02d}' for n in partials) or 'none'}).",
             f"- Not verified: {', '.join(f'AC-{n:02d}' for n in unverified)} — each record states the exact blocker and next action.",
             "- Every verified record was re-read against its evidence folder/test before checking; anything that relied only on fixtures where the criterion demands live evidence stays unchecked."]
    items = []
    for n in range(1, TOTAL + 1):
        title = titles.get(f"{n:02d}", "")
        link = f"[evidence](docs/verification/AC-{n:02d}.md)"
        if n in verified:
            items.append(f"- [x] **AC-{n:02d}** {title} — {link}")
        elif n in partials:
            items.append(f"- [ ] **AC-{n:02d}** {title} — ◐ partial: {partial_line(n, 'proven')} / deferred: {partial_line(n, 'deferred')} — {link}")
        else:
            items.append(f"- [ ] **AC-{n:02d}** {title} — {SHORT_BLOCKERS.get(n, statuses[n])} — {link}")
    rows = ["See the [acceptance criteria list in the README](../../README.md#acceptance-criteria) for every criterion's checkbox, status and evidence link."]
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
    rt = re.sub(r"(<!-- ac-list:start -->\n)(.*?)(<!-- ac-list:end -->)", lambda m: m.group(1) + "\n".join(items) + "\n" + m.group(3), rt, flags=re.S)
    rt = re.sub(r"Verified acceptance\ncriteria: \*\*[^*]+\*\*( · \*\*\d+\*\* partial)?", f"Verified acceptance\ncriteria: **{len(verified)} / {TOTAL}** · **{len(partials)}** partial", rt)
    rt = rt.replace("__VERIFIED__ / 41", f"{len(verified)} / {TOTAL}")
    rt = rt.replace("__UNVERIFIED__", ", ".join(f"AC-{n:02d}" for n in unverified))
    rt = re.sub(r"(Unverified:\n)AC-[0-9, AC-]+(\. The biggest gaps)", lambda m: m.group(1) + ", ".join(f"AC-{n:02d}" for n in unverified) + m.group(2), rt)
    rt = re.sub(r"(## Follow-ups\n\n.*?\n\n)(.*?)(\n\n## Project documents)", lambda m: m.group(1) + "\n".join(follow) + m.group(3), rt, flags=re.S)
    rt = rt.replace("__FOLLOWUPS__", "\n".join(follow))
    readme.write_text(rt)
    print("verified:", len(verified), "partial:", partials, "unverified:", unverified)

if __name__ == "__main__":
    main()
