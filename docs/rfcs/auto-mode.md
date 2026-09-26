# RFC: Auto Mode — health- and usage-aware agent selection

Status: **draft for discussion; no implementation authorized by this RFC**.
Requested by the owner on 2026-09-25. Baseline: `2c2c7cf`.
Acceptance criteria: **0 / 30 verified**, tracked independently below as `AUTO-AC-NN`.

## Outcome

A user chooses **Auto** once and Overseer chooses a usable agent route from the user's allowed accounts and providers. If the preferred service is unavailable or its account has exhausted its allowance, Overseer selects another eligible route and explains why. The user should not have to know which harness can reach which provider or manually inspect several usage screens.

A route is **harness + provider endpoint + account + model + capabilities**. The harness is a means of running the task, not the primary routing decision. OpenCode can expose several provider routes; changing from Claude Code to OpenCode while still using the same unavailable upstream is not a successful fallback.

“Right agent” in this version means an allowed route that meets the task's capability and model requirements, is usable given current evidence, and ranks highest under explicit user preferences. It does not mean an inferred universal quality ranking.

Example: the user prefers OpenAI through Codex and allows Claude as a fallback. With healthy services and sufficient account allowance, choose Codex. If OpenAI is unavailable, choose an eligible Claude route. If Anthropic fails during a Claude Code task, consider Codex or an OpenCode route on a different available provider. These are policy examples, not provider-specific branches in the selector.

## Scope and coexistence

This PR adds this RFC only. It does not change the daemon, extension, account credentials, packaging, original RFC, or release acceptance ledger. Implementation follows a reviewed revision of this document in separate PRs, coordinated with the VS Code production-readiness work.

The design builds on [account governance](account-governance.md), [credential isolation](claude-credentials.md), the [existing RFC](../overseer-rfc.md), and [adapter capabilities](../compatibility.md). Preserve account/subscription login only and no API-key fallback. Auto Mode is disabled by default. Existing manual tasks retain their selected route and behavior.

Current account governance restricts OpenCode to local providers. The routing model must accommodate multiple providers, but cloud OpenCode routes stay ineligible until their account integration is separately supported and verified. Auto Mode does not implement that login work or assume that a ChatGPT/Claude subscription authorizes another harness to use it. A verified local endpoint is a valid route without a subscription quota; this is explicitly `not_applicable`, not an invented unlimited balance.

In scope: initial selection, availability and remaining-allowance observations, deterministic routing, bounded fallback, safe continuation, explanations, and evidence. Out of scope: model-quality benchmarking, races between agents, automatic merges, credential migration, purchases, credit redemption, paid overage activation, and implementing a new harness. Native child routing remains under its harness; do not silently reroute a child independently of its parent.

## Approaches considered

| Approach | Benefit | Limitation | Decision |
| --- | --- | --- | --- |
| Fixed harness fallback chain | Small initial implementation | Confuses harness with provider; expands into special cases | Reject |
| Provider/account observations plus a deterministic policy engine | Auditable, testable, extensible; unknown evidence remains explicit | Requires adapters and freshness rules | Recommend |
| Model decides the route from raw outputs | Can interpret unfamiliar displays | Adds cost, nondeterminism, privacy exposure, and cannot reveal missing facts | Restrict to optional evidence extraction; never the routing authority |

Provider-specific knowledge belongs in adapters, capability declarations, and configuration. The selector consumes a common contract and must not contain `if Claude ... else Codex ...` policy logic.

## What is known, and what still needs validation

Research checked 2026-09-25; documentation is a discovery lead, not proof of support in an installed version. No paid/model probes were run for this RFC.

| Surface | Evidence | Proposed acquisition and limit |
| --- | --- | --- |
| Codex | Existing adapter receives `account/rateLimits/updated`; [official app-server docs](https://learn.chatgpt.com/docs/app-server) document `account/rateLimits/read`, usage windows, and `rateLimitsByLimitId` | Prefer the account-scoped structured read and update stream. Feature-detect the installed schema and retain every applicable window/bucket. Validate reads under the selected isolated account; existing event evidence does not prove proactive reads. |
| Claude Code | [Official usage docs](https://code.claude.com/docs/en/costs) describe `/usage`, plan bars, and last-known data on fetch failure. [Prior local survey](../verification/AC-01.md) identifies `rate_limit_event` as a candidate | Test structured events first, then a supported read-only usage command/display capture. `/usage` is interactive; do not assume `claude usage --json` exists or that prompt mode runs slash commands without inference. Cache age must survive parsing. |
| OpenCode | [Official CLI docs](https://opencode.ai/docs/cli/) describe `opencode stats` as session token/cost statistics; [local survey](../verification/AC-01.md) did not find an account-quota API | Treat stats as consumed usage only. Each configured provider/account needs its own quota adapter or an explicit unknown result. Never subtract session tokens from an invented subscription token budget. |
| Public service status | Provider-specific feed discovery is still required | Map status components to actual endpoints/products. A green public page does not prove that a particular account/model works; an unrelated incident does not block every route. |

### XCB assessment

The linked project is [hraness/xcb](https://github.com/hraness/xcb), already assessed at `bd360151d29287147e77451c3edbc2b66405c812` in [AC-03](../verification/AC-03.md). Its current [route documentation](https://github.com/hraness/xcb/blob/main/docs/route.md) describes filtering eligible credentialed accounts, avoiding known quota-blocked windows, ranking routes, and custody during execution. These concepts are directly relevant. Its current docs and the earlier pinned assessment may describe different revisions.

Recommendation: use it as a design reference; do not add a dependency in this RFC. Before adopting code or a subprocess integration, pin the inspected revision, inspect route/quota/custody implementations and tests, reconfirm license/notices, and test compatibility with Overseer's account ownership, supervisors, and workspace lock. An independent account-custody system could conflict with those existing owners. Reject any integration that requires moving credentials, broadening authentication scope, or delegating durable task ownership. Record a reuse-or-no-reuse decision; resemblance alone is insufficient.

## Observation contract

The daemon owns collection, decisions, and durable history. The extension displays those decisions. Conceptual types below are contract requirements, not a proposed rewrite of existing storage.

| Record | Required fields / meaning |
| --- | --- |
| Route | Stable route ID; harness and transport/version; provider endpoint/product; account identity fingerprint; model; capabilities; enabled/allowed flags; quota-pool IDs where known |
| Observation | Kind, scope, normalized value, source, capture time, original observation time if cached, expiry, adapter/version, evidence reference, confidence (`reported`, `parsed`, `inferred`) |
| Health | `healthy`, `degraded`, `unavailable`, or `unknown`; separate scopes for local harness, endpoint/product/model, account authentication, and local connectivity |
| Quota window | Pool/bucket ID, account/model scope, unit, remaining or used fraction when supplied, limit when supplied, reset time when supplied, and `available`, `low`, `exhausted`, `unknown`, or `not_applicable` |
| Decision | Request ID, policy revision, candidate snapshot, exclusions, ordered eligible routes, selected route, reasons, evidence IDs/ages, reservation, and attempt count |

Keep rate-limit throttling, exhausted allowance, expired authentication, network failure, and ordinary task/tool failure distinct. One failed account does not establish a provider outage. A 429 needs the provider's structured error/reset/retry evidence before it becomes a quota conclusion. Missing and malformed values are unknown, never zero or unlimited.

Several routes may share one account quota pool: `codex` and `codex-app` do not create two allowances. OpenCode routes sharing the same upstream/pool inherit the relevant exclusion. If cross-harness pool identity cannot be established, mark it unresolved and conservatively avoid treating the alternate harness as new capacity after an account-quota failure.

## Collecting evidence

Use this order per adapter:

1. Supported structured local protocol/API, CLI JSON, or native rate-limit events.
2. Deterministic parsing of supported CLI output or a narrowly scoped local status artifact with a known schema/version.
3. Optional small-model extraction from already collected, redacted usage text, only with explicit user opt-in.
4. Unknown, with the reason and the action that could improve it.

Text parsing is acceptable; arbitrary grepping of a whole transcript is not an authoritative balance. Parsers must handle supported output versions, ANSI formatting, percent-used versus percent-left, multiple windows, cached timestamps, and reset time zones. Version/format drift, impossible values, missing units, or conflicting labels produce unknown. Never execute content extracted from a display or log.

Model extraction is **off by default** and separable from core delivery. Consent names the destination provider/account/model, data sent, and a user-approved hard call/token or spend cap. Use the smallest validated model from the allowed set; no hard-coded “tiniest” model claim. Send only a bounded, redacted status excerpt, never prompts, source files, or credentials. The response must cite exact input spans for values and pass deterministic schema/range/unit checks. It can extract “20% left” from text; it cannot infer remaining allowance from token consumption alone. Ambiguous output remains unknown; inferred evidence cannot override an authoritative block. No recursive fallback to other models when extraction fails or its route is unavailable. Revocation prevents further calls.

### Health, freshness, and collection bounds

Proposed configurable defaults: 60-second health/quota freshness, 5-second timeout per collector, at most four concurrent collectors, and a 10-second overall decision deadline. Batch duplicate refreshes for a shared account/pool. Collect on demand before launch, consume native updates during runs, and refresh on relevant failures. Do not continuously spend model tokens to check health.

Expired observations become unknown and trigger one bounded refresh. A reset timestamp permits rechecking, not assuming the balance has replenished. Future timestamps, reversed windows, or unreasonable clock skew are rejected. Fresh explicit account exhaustion/authentication failure blocks its scope regardless of a public green status. Fresh route success can outweigh a broad public incident advisory; a matching endpoint failure plus a matching incident excludes the affected route. A status feed alone is advisory/degraded until route-specific evidence establishes unavailability. This prevents a status-page outage from disabling working accounts.

No paid synthetic request is allowed just to probe a service unless the user enables a separate bounded probe policy. The actual requested task may be the first request under unknown health. A local network outage is surfaced as connectivity failure and stops repeated remote fallback attempts; an eligible local route can still be considered.

## Selection policy

On enabling Auto, the user sees and can edit the allowlist, route preferences, permitted model/capability set, reserve thresholds, unknown-data behavior, and continuation permission. Choosing Auto authorizes routing within that visible set; it does not authorize login changes, new providers, API keys, purchases, or relaxed permissions.

Proposed default policy:

1. Enumerate supported installed routes, with a working account binding (or an explicitly local endpoint). Exclude disallowed routes, unsupported authentication, missing required capabilities/models, known failures, exhausted pools, and routes at/below a user reserve. Proposed reserve is 10% for reported percentage windows and zero remaining for native-unit balances unless configured otherwise. If any applicable window is exhausted, the route is exhausted even if another window has space.
2. Prefer fresh healthy evidence over degraded over unknown health. Within each health tier prefer known allowance above reserve (or verified not-applicable local quota) over unknown allowance. Unknown routes remain eligible as a last resort by default with a visible warning; strict mode blocks them. Inferred quota cannot qualify for the known-allowance tier or strict mode.
3. Apply the user's ordered provider/model preferences, then preferred harness for that provider. The initial suggested preference can put OpenAI/Codex first, as requested; the user can change it. This ranking is data, not source-level provider logic.
4. Within an otherwise equivalent quota pool/model class, prefer greater known headroom across applicable windows; do not compare raw tokens, dollars, and subscription percentages across providers as equivalent capacity. Break remaining ties by stable route ID.
5. Atomically reserve dispatch capacity, recheck policy/account/evidence, and persist the selected decision before launching through the existing supervisor. Honor configured concurrency per quota pool; default one Auto dispatch at a time per pool, while accounting for known manual runs. This is a local reservation, not a guarantee about usage on other machines. Release on launch failure or settled termination and recover reservations after daemon restart.

Explicit provider, account, model, or harness pins are hard constraints. If none qualify, launch nothing and show exclusions, next known reset/retry time, and actionable options (wait, refresh, sign in, edit policy, or choose manually). Do not silently relax a pin or loop indefinitely.

## Fallback and continuity

Before a task can have effects, a confirmed launch/provider failure can trigger reselection within the same policy. Proposed limit: three distinct route attempts per dispatch, never retry the same blocked pool through another harness. Respect `Retry-After`; otherwise use a 60-second cooldown for transient endpoint failures. Quota blocks persist until their reset followed by refresh, or fresh authoritative evidence clears them; auth failures require refreshed identity/status. Passing a cooldown permits one shared recovery check, not a stampede. Successful running routes stay selected even if a preferred provider recovers.

Once a run has started, absence of an observed tool call does not prove absence of effects. Continue automatically only after a supported no-effects result or a safe checkpoint: the previous supervisor and writers are confirmed stopped, pending tools/children are settled, workspace ownership is released, and any external action has a known outcome. If that cannot be established, pause with an explanation. Never rerun an uncertain shell command or external action merely because its response was lost.

At a safe checkpoint, if the user enabled continuation, create a new linked run on the same task/workspace with a new harness-native session. Preserve files, snapshots, original goal, user corrections, completed/remaining work, tests, and any unresolved actions in an explicit handoff record. Transfer only context allowed for the new provider. If the handoff cannot fit or required context is unavailable, pause; do not silently discard it. Native session IDs and tool messages are not portable. Label context loss and retain the earlier transcript. The new run inherits equal or stricter sandbox/approval requirements; pending approvals are not transferable and are never auto-approved. If continuation is disabled, offer the prepared switch for user action.

Ordinary test failures, tool exits, refusals, user interruption, and permission denial are not service outages and do not cause fallback. Disabling Auto stops future selection/reselection but does not kill a running agent. On daemon restart, reconcile real supervisor state before deciding anything; never relaunch a possibly active run.

## Integration boundary and UI

Implement collection and selection behind a small daemon routing interface after the production branch stabilizes. Existing adapter events feed normalized observations. Account governance remains the identity/credential owner; the supervisor remains the process owner; existing workspace ownership remains the single-writer authority. Additive durable decision records and protocol capability negotiation must let older clients continue manual use. Do not choose a new migration number or refactor shared run/account types in this RFC PR.

New Task offers Auto beside manual selection. Preview the selected harness/provider/account/model and a short reason such as “Codex · work account — Claude allowance exhausted; resets at 14:00.” Show unknown or stale data explicitly. The task history records each switch, trigger, source age, and context limitation. The explanation view shows rejected alternatives and lets the user pin a route or edit policy. Monitoring and bounded decisions continue when VS Code closes; UI disconnection must not duplicate execution.

## Acceptance criteria

These are implementation requirements, not claims about this documentation PR. All start unchecked. Core release requires AUTO-AC-01 through AUTO-AC-29. AUTO-AC-30 is conditional: either verify the optional extractor or explicitly ship it unavailable, with no model calls, and record it as deferred (not verified). Never count a deferred criterion as passing.

### Evidence and boundaries

- [ ] **AUTO-AC-01 — Isolated delivery.** Auto ships disabled by default and manual routing remains unchanged. **Verify:** diff review plus manual create/follow-up/interrupt regression with Auto disabled; confirm release AC IDs and account-governance work are untouched by routing-only changes.
- [ ] **AUTO-AC-02 — Capability survey.** Record installed versions, routes, supported quota/health sources, and unsupported combinations for Codex, Claude Code, and OpenCode. **Verify:** redacted reproducible read-only probes per available harness; missing accounts/access remain explicit blockers for claimed support.
- [ ] **AUTO-AC-03 — Provider-neutral selector.** Routing depends on normalized data and policy. **Verify:** synthetic provider and harness names work without selector changes; source review finds no provider-specific fallback branches in the selector.
- [ ] **AUTO-AC-04 — Distinct routes and shared pools.** Account/model/endpoint scopes and shared quota are preserved. **Verify:** exhaust one account across two harnesses; both are excluded while an independent allowed account remains eligible; test unresolved identity conservatism.
- [ ] **AUTO-AC-05 — Codex remaining allowance.** Structured account-scoped reads/updates expose all applicable limits independently of consumed tokens. **Verify:** tiny live isolated-account read plus fixtures for multiple buckets/windows, absent fields, exhausted secondary window, and unsupported schema; no fabricated total token allowance.
- [ ] **AUTO-AC-06 — Claude acquisition.** A supported collector reports allowance or explicit unknown without launching an inference prompt to obtain a status display. **Verify:** versioned structured/text fixtures and one live read-only capture with an available account; cached values retain original age; expired login is account-scoped.
- [ ] **AUTO-AC-07 — OpenCode provider separation.** Each supported provider route has its own evidence and authentication eligibility. **Verify:** real OpenCode with two controlled local endpoints; failing one leaves the other eligible; stats-only data stays quota-unknown; unsupported cloud login is excluded.
- [ ] **AUTO-AC-08 — Parser honesty.** Deterministic collectors fail safely on format drift. **Verify:** fixtures for ANSI text, used/remaining inversion, localization, malformed/negative/out-of-range values, reset time zones, and missing units; unknown replaces invented balances.
- [ ] **AUTO-AC-09 — Scoped health.** Public incidents, route errors, authentication, throttling, and local connectivity remain distinct. **Verify:** decision fixtures for green feed plus exhausted account, unrelated incident, broad incident plus route success, matching endpoint failure, 429 without quota proof, and offline host with local alternative.
- [ ] **AUTO-AC-10 — Freshness and reset.** Expiry/reset schedules a bounded refresh instead of assuming health/capacity. **Verify:** fake clock tests at expiry/reset, cached source timestamps, clock skew, out-of-order updates, and collector timeout.

### Decisions and resource bounds

- [ ] **AUTO-AC-11 — User policy and pins.** Only allowed routes satisfying account/provider/model/harness pins and capabilities can run. **Verify:** missing approval transport, unavailable pinned model, disabled provider, API-key-only route, and permission downgrade are excluded with reasons.
- [ ] **AUTO-AC-12 — Preferred agent.** With equivalent usable evidence the configured provider/model/harness order determines selection. **Verify:** OpenAI/Codex wins with that preference; editing preferences makes Claude win with identical observations and no code change.
- [ ] **AUTO-AC-13 — Quota reserve and windows.** Any exhausted or reserved applicable window excludes a route; unlike units are not ranked as equivalent capacity. **Verify:** boundaries below/at/above 10%, short/weekly/model-specific windows, zero credits, and token-versus-percentage fixtures.
- [ ] **AUTO-AC-14 — Unknown policy.** Unknown health/quota is explicit and ranked according to policy. **Verify:** known usable candidate beats an equivalent unknown candidate; default allows last-resort unknown with warning; strict mode refuses; inferred evidence does not satisfy strict mode.
- [ ] **AUTO-AC-15 — Deterministic explanation.** Identical normalized inputs and policy yield identical route/reasons. **Verify:** repeat and reorder candidate inputs; persisted decision includes selection, exclusions, evidence ages, and policy version without secrets.
- [ ] **AUTO-AC-16 — All routes unavailable.** No eligible route means no launch. **Verify:** exhaust/disable all candidates; show reasons and known reset/retry times; refresh/manual controls work; no busy loop or automatic spending appears.
- [ ] **AUTO-AC-17 — Concurrent admission.** Shared-pool dispatch and workspace ownership are atomic. **Verify:** simultaneous requests at concurrency one yield one admitted launch, including a competing manual run; policy/account change before launch invalidates reservation; failure releases it; external usage is not promised to be reserved.
- [ ] **AUTO-AC-18 — Bounded collectors.** Collection obeys timeout, concurrency, deduplication, and no-paid-probe defaults. **Verify:** 100 synthetic candidates with stalled collectors reach a decision/pause within the 10-second deadline plus 1-second test tolerance, no more than four collectors run, and duplicate pool reads coalesce; measure UI responsiveness.

### Fallback and continuity

- [ ] **AUTO-AC-19 — Pre-effect fallback.** Confirmed safe launch failure selects another eligible route. **Verify:** preferred route rejects before work; eligible alternate runs; an OpenCode route on the same failed upstream does not evade its exclusion.
- [ ] **AUTO-AC-20 — Cooldown and attempt bound.** Transient failures do not oscillate or multiply requests. **Verify:** fake clock tests for Retry-After, 60-second default, at most three distinct attempts, shared recovery check, quota reset refresh, and provider recovery without interrupting an active run.
- [ ] **AUTO-AC-21 — Safe handoff.** Automatic continuation preserves work and transfers only at a confirmed checkpoint. **Verify:** controlled real harness processes fail after a file edit, settle, and continue in a new linked run with the same file/snapshot history and one writer; handoff includes goal, corrections, progress, tests, and limitations.
- [ ] **AUTO-AC-22 — Uncertain effects stop.** Unknown process/tool/external-action outcome cannot trigger replay. **Verify:** lost connection after a mock external side effect, live child process, pending approval, and unconfirmed supervisor stop all pause without a duplicate command or second writer.
- [ ] **AUTO-AC-23 — User intent survives.** Interrupt, denial, ordinary task failure, and disabling Auto do not trigger unwanted fallback. **Verify:** each event leaves the task stopped/failed or the current run continuing as appropriate; continuation-disabled mode waits for user action; sandbox and provider-sharing constraints survive handoff.
- [ ] **AUTO-AC-24 — Durable recovery.** Restart/reconnect preserves decisions without duplicate work. **Verify:** crash before launch, after reservation, after spawn, and during handoff; reconcile actual supervisors and recover stale reservations; reconnect two UI clients with one launch and one decision history.

### Delivery evidence

- [ ] **AUTO-AC-25 — Understandable UI.** Users can select Auto, inspect its choice, change policy, and see why it switched or paused. **Verify:** packaged VS Code screenshots and actual flows for healthy, exhausted, unknown, and all-unavailable states; decision persists after window closure/reopen.
- [ ] **AUTO-AC-26 — Credential/privacy boundary.** Observations and handoffs expose no credentials and never change login or spending settings. **Verify:** redaction fixtures, bounded retained evidence, linked-account identity change invalidation, and inspection of logs/events/database; no credential export, key fallback, purchase, reset redemption, or permission auto-approval.
- [ ] **AUTO-AC-27 — XCB disposition.** Reuse is an explicit evidence-backed decision. **Verify:** pinned revision, relevant files/tests, license and custody compatibility assessment; if adopted, integration tests and notices; if rejected, concrete reasons and no accidental runtime dependency.
- [ ] **AUTO-AC-28 — End-to-end support matrix.** Claimed supported routes work through the actual daemon/adapters. **Verify:** one tiny live account-authenticated Auto task for each claimed Codex/Claude route and controlled OpenCode runs; inject outage/quota fixtures for fallback rather than exhaust paid accounts; distinguish live, mock, fixture, unsupported, and blocked coverage. macOS release evidence required; Linux remains explicitly unverified until exercised.
- [ ] **AUTO-AC-29 — Reproducible goal ledger.** Every passing criterion has revision-specific evidence and failures reopen it. **Verify:** independently follow the ledger for all core criteria; original release checklist stays separate; no documentation-only or fixture result masquerades as required live evidence.
- [ ] **AUTO-AC-30 — Optional model extraction.** If shipped, extraction is opt-in, bounded, source-grounded, and subordinate to deterministic policy. **Verify:** consent off/revoked yields zero calls; opted-in ambiguous/redacted/malicious excerpts, invalid schema, fabricated spans, timeout, unavailable extractor, and exhausted cap yield unknown with no retries beyond the cap; authoritative blocks remain blocks. Record the validated model/version and extraction evaluation evidence.

## Verification loop and implementation sequencing

Create a separate `docs/verification/auto-mode/` ledger when implementation begins; do not extend the original `records.py` or renumber `AC-01…AC-53` for this milestone. Each AUTO-AC record includes status (`not started`, `in progress`, `implemented / unverified`, `blocked`, `verified`, or conditional `deferred`), tested commit, environment/harness/provider versions, policy/fixture inputs, exact steps, expected and actual result, evidence links, coverage type, and blocker/next action. Only verified receives `[x]`. This RFC is the authoritative checklist until a separately reviewed generator is introduced.

Suggested order: (1) survey/contract and XCB decision, (2) collectors and pure selector with fixtures, (3) admission and bounded safe fallback through existing supervisors, (4) UI and tiny live verification, (5) optional extraction if deterministic gaps justify it. These are slices for a later implementation plan, not tasks dispatched by this RFC.

For a future goal loop: pick an unmet core criterion, implement the smallest related slice, run its Verify scenario and affected regressions, attach evidence at the tested commit, and update its status. Finish only when all 29 core criteria are verified and the optional extractor is verified or explicitly deferred. Missing logins/platforms remain visible blockers or unsupported scope; do not solve them by repeatedly spending tokens. No implementation goal or recurring automation is started by drafting this RFC.

## Review decisions

The following are proposed defaults to refine while reviewing, not claims of owner approval:

- Prefer OpenAI/Codex when evidence tiers are equal; users can reorder routes.
- Reserve 10% of each reported percentage window; permit unknown evidence only as last resort, with a strict alternative.
- Allow initial automatic selection/fallback within the configured allowlist; require a separate persistent opt-in for automatic continuation after work begins.
- Keep model extraction optional and off; first establish whether deterministic evidence is insufficient on supported harness versions.
- Use the stated freshness, timeout, cooldown, and attempt limits as measurable starting values; change them through policy and update corresponding tests.
