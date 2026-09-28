# Auto Mode: open criteria and the live-test plan (2026-09-27)

Written on `claude/auto-swarm` after the continuous-allocation slice (see the ledger's "Selection on the booking's numbers"). Nothing here has run live. No paid turn, sign-in or sign-out was made to write it.

## Where each open criterion stands

Classes: **(a)** fixtures and targeted tests can finish it; **(b)** it needs a live account; **(c)** it waits on an owner decision; **(d)** it needs a packaged VS Code UI scenario. Several criteria have parts in more than one class.

| Criterion | What its Verify clause still needs | Class |
| --- | --- | --- |
| AUTO-AC-04 Distinct routes and shared pools | Fixtures cover one exhausted account across two harness routes and a linked alias, an independent account staying eligible, and an unresolved OpenCode upstream held back. Left: a real identity read showing two profiles on one OpenAI login share a pool while another login does not. | (a) done in fixtures; (b) read-only identity check |
| AUTO-AC-05 Codex remaining allowance | A tiny live read on an *isolated* signed-in account (so far only the system profile was read). | (b) read-only |
| AUTO-AC-06 Claude structured acquisition | A live `rate_limit_event` seen during an authorized tiny task. Fixtures for status-line settings preservation and cache age are still needed. | (b) one small turn per account; (a) fixtures |
| AUTO-AC-10 Freshness and reset | Provider-origin cache timestamps, more source-order cases, and collector scheduling under a fake clock. Whether Codex's `account/rateLimits/read` returns a cached snapshot, and how old it is, can only be seen live. | (a); (b) read-only (Step 0) |
| AUTO-AC-11 Constraints and harness preference | Verified in this slice. | (a), done |
| AUTO-AC-12 Task-aware model and effort | Verified in this slice. | (a), done |
| AUTO-AC-13 Expected allowance fit | Verified in this slice (fixture economics). | (a), done |
| AUTO-AC-14 Unknown and cold start | Needs a routing-classifier response that cannot turn an unknown balance into a fact. Depends on AUTO-AC-34. | (a) after 34 |
| AUTO-AC-15 Reproducible decision trace | Replay that includes an inference output, plus a secrets and privacy sweep of the traces. Depends on AUTO-AC-34. | (a) after 34 |
| AUTO-AC-16 No suitable route | Verified in this slice. | (a), done |
| AUTO-AC-17 Concurrent admission | Done in this slice: the last-window race (calibrated Auto against Swarm) and the Gate S last-slot check. Left: a competing manual run on a profile whose account identity was never read. Counting it as possibly the same account would block Auto and known-window bookings whenever the owner runs an unidentified manual task; reading the identity would change the manual path, which AUTO-AC-01 keeps unchanged. Also left: local endpoint aliases and a live watcher-wake race. | (a); (c) unknown-identity policy |
| AUTO-AC-19 Pre-effect fallback | Verified on 2026-09-28 (a launch rejected after discovery falls back; a path alias of a failed listener is excluded). | (a), done |
| AUTO-AC-20 Bounded retries and stable dispatch | Verified in this slice: one shared recovery check per endpoint across accounts. | (a), done |
| AUTO-AC-21 Safe handoff | "Controlled real harness processes" fail after an edit and continue in a linked run. Also automatic handoff selection. | (a) selection; (b) real harness (Codex, or OpenCode with local Qwen if the handoff accepts it) |
| AUTO-AC-23 User intent survives | Verified in this slice. It reopens if automatic continuation is added. | (a), done; (d) for the waiting UI |
| AUTO-AC-24 Durable recovery | A crash mid-handoff and a Swarm crash between claim and bind; two real UI clients reconnecting (phone if Gate N lands). A booked Auto unit across a crash is done (2026-09-28). | (a); (d) |
| AUTO-AC-25 Simple Auto UI | A packaged flow with a real browser child and its result, plus the Gate S home/docked targets. | (d); (b) for the real browser work |
| AUTO-AC-26 Credential and privacy boundary | Verified in this slice (fixtures). A sweep of real harness transcripts belongs to the Step 1 evidence. | (a), done; (b) live sweep |
| AUTO-AC-31 Repeated delegation while healthy | An actual parent delegating a real browser run-through. | (b) |
| AUTO-AC-32 Consumption attribution | Overseer's own turns meter once (done 2026-09-28); a watcher or check-in turn and an unchanged watch (a). A comparable live allowance delta (b). | (a); (b) |
| AUTO-AC-33 Outcome-aware selection evaluation | Real browser, routine-edit and diagnosis work scored against the frozen v1 rubric. | (b); (c) budget |
| AUTO-AC-34 Bounded routing inference | Not started. Needs a classifier call path with timeout, invalid-output, budget and escalation bounds, tested with a fixture classifier. Every call is a paid turn, so the owner must decide whether a classifier ships and on which model (the budget rules allow only luna-low or haiku). Until then no inference runs and traces record `not_used`. | (a) for the fixture bounds; (c) whether and which model |
| AUTO-AC-35 Delegation lifecycle | A real managed child returning a browser artifact exactly once. | (b) |
| AUTO-AC-36 Adaptation without configuration upkeep | Verified on 2026-09-28 (no churn, upgrade invalidation through dispatch). | (a), done |
| AUTO-AC-38 Minimal local measurement schema | Secret-sentinel fixtures (a). A real authorized Claude task record (b). | (a); (b) |
| AUTO-AC-39 Retention, deletion and export | The whole-volume floor is done (2026-09-28). Left: a measured worst-case journal size and slow-disk latency. | (a) |

**Owner decisions that touch these (defaults kept):** Claude calibration's plan source, allowance, bracketing and freshness (RFC, "Claude quota readings for calibration") decide whether a Claude route can ever get a qualified draw and so a known fit (AUTO-AC-13/17 in live use, not their fixture Verify clauses). The Swarm director and native workers decide the Swarm legs of AUTO-AC-17 in live use.

## The budget conflict to settle first

AGENTS.md allows ChatGPT turns only on `gpt-5.6-luna` at low effort and light Claude use (haiku). Auto's versioned capability priors offer only `gpt-6-sol`/medium and `gpt-6-astra`/high for Codex, and `sonnet`/medium and `opus`/high for Claude. **So no live Auto selection can land on an allowed model today.** The earlier AUTO-AC-28 live roots ran Sol/medium and Sonnet/medium. Two ways forward, for the owner to choose:

1. **Per-step exceptions.** The owner approves each listed turn on Sol/medium or Sonnet/medium. This exercises the product's real priors.
2. **A live-test prior.** A daemon setting, off by default and never set from a conversation, adds `gpt-5.6-luna`/low and `haiku`/low as general-tier routes for live verification only. Selection, admission, metering and launch are provider-neutral (AUTO-AC-03), so this checks everything except the choice of product model. It is not built yet.

Until then, only the read-only steps below may run.

## Accounts and sign-in (owner approves each)

- Use a disposable `OVERSEER_HOME` and new, non-system profiles (`profile.create`). Never the owner's system logins, checkouts or credential files. Nothing is signed out afterwards. The owner removes the disposable home.
- **Four browser sign-ins, one per account, each approved and completed by the owner:** `openai-pro` and `openai-plus` (harness `codex`); `claude-max` and `claude-regular` (harness `claude`). `profile.login_command` prints the command. The owner runs it and signs in in the browser. The agent never types credentials.
- **Optional fifth sign-in:** a second `codex` profile signed into the *same* Pro account, `openai-pro-2`, for AUTO-AC-04's shared-pool check.
- **Local Qwen 0.5B through Ollama:** no sign-in. The AC-140 memory guard applies. Load one model at a time.

## Step 0: read-only, before any turn (no model call)

For every signed-in profile, in this order, one attempt each:

1. `profile.status` gives the auth method and subscription login. API-key login must be refused for Auto.
2. Identity: Codex `auto.models.refresh` (account-scoped catalog) and Claude's bounded `claude auth status --json` collector. Record only the fingerprint and generation.
3. Allowance:
   - Codex: `auto.quota.refresh` then `auto.quota.state` (metadata-only `account/rateLimits/read`).
   - Claude: `auto.quota.state` must say **unknown**, because Claude has no read between runs. That is the expected result, not a failure.
4. Tools: `auto.tools.inspect` on a scratch repository.
5. Cache age (AUTO-AC-10): read Codex allowance twice, a minute apart, with no work in between, then once more after the Step 1 turn. Record whether the provider's values or reset times move without work, and whether any field reports an origin time. This tells us whether readings are cached upstream and how old they can be.

**Checking that Auto's quota queries return each account's real remaining usage:**

- **Codex (Pro, Plus).** The owner opens the provider's own usage display for the same account: the ChatGPT usage page, or `/status` in an interactive Codex session the owner starts. Within the same minute, compare each window's used percentage and reset time with Auto's normalized windows. Pass: every window Auto reports is present, used percentages agree within one point, resets agree, and no window is invented. A window the provider shows and Auto lacks is a failure for AUTO-AC-05. Pro and Plus must give different fingerprints and pools. `openai-pro-2` must give the same fingerprint and pool as `openai-pro` (AUTO-AC-04).
- **Claude (Max, regular).** There is no pre-turn value to compare. After the Step 1 turn below, compare the recorded `claude/native-rate-limit-event` windows with the owner's `/usage` display (interactive session) or the claude.ai usage page. Same pass rule. Plan and allowance are recorded as unknown (owner decision pending).
- **Qwen.** There is no subscription quota. Auto must not list a route for Qwen 0.5B: it has no capability prior, and only `gpt-oss-120b` is a local prior. Confirm this with `auto.root.preview`. It shows that a cheap, uncapable local model cannot win (AUTO-AC-14/16 on real data), with zero spend.

Evidence: redacted JSON under `docs/verification/evidence/auto-live/<date>/`, with fingerprints truncated and no email, token or raw provider text.

## Step 1: the smallest paid turns (only after the budget decision)

Every turn is one attempt. On failure, stop and report; do not retry or reroute to another paid account. Before each turn, take a quota reading. After each turn, take a settled reading (60 s later).

| Criterion | Account(s) | Turns | What passes |
| --- | --- | --- | --- |
| AUTO-AC-06 | claude-max, claude-regular | 1 tiny read-only root each (`node test/local/auto-live.js claude`, pointed at the isolated profile) | A normalized `rate_limit_event` observation appears during the turn, with account and model-family windows. Before the first response, state is unknown. Existing status-line settings in the profile are unchanged. |
| AUTO-AC-38 (Claude part) | claude-regular | the same turn | One content-free work record with model/effort, account reference, window references, source and freshness, and outcome. The subscription draw stays unverified. |
| AUTO-AC-31 and 35 | openai-plus | 3: parent, browser child, diagnosis child | Needs a browser MCP tool in the isolated Codex profile; the owner approves adding it. The parent delegates a real browser check through the run-bound bridge. The child returns an artifact exactly once. The parent consumes it and requests a separately selected unit. Both routes stay healthy. |
| AUTO-AC-32 (live delta) and 36 | openai-plus | 5 isolated tiny runs of one bucket, then 1 booked run | The qualified upper draw forms from five real window movements. The booking cites it. A sixth reading is compared with the bound. If the meter reports whole points, every sample is its movement plus two points. |
| AUTO-AC-21 | openai-plus, or OpenCode with local Qwen if the handoff path accepts OpenCode (check with a fixture first) | 2: an edit that then fails, and the continuation | A linked run on the same snapshot history with one writer, and the handoff record's fields. |
| AUTO-AC-33 | pro and plus | at least 3 tasks x 2 routes | Only with an explicit owner budget: the v1 rubric and tolerances are already frozen in `evaluation-v1.md`. |

Read-only between steps: quota and identity reads cost no turn and may repeat. Model turns may not.

## Packaged UI scenarios to run later (not run this session)

- `test/ui/scenario-auto-root.js`: after this slice, the decision cards must show `estimated_draw_exceeds_allowance` readably when a calibrated route is excluded (AUTO-AC-25).
- `test/ui/scenario-composer.js`: harness preference and manual pin (AUTO-AC-11/25).
- `test/ui/scenario-usage.js`: Auto Usage inspection, export and clear (AUTO-AC-39).
- `test/ui/scenario-swarm-allowance.js` and `scenario-swarm-status.js`: Swarm on the booking with the reading error now held back in each window (AUTO-AC-17, Swarm CONTRACT-01).
- New scenarios still to write: two VS Code windows reconnecting to one Auto decision (AUTO-AC-24); a continuation-disabled handoff waiting for the owner (AUTO-AC-23); the Gate S home composer's New agent and `@overseer` targets with Auto routing (AUTO-AC-25).
