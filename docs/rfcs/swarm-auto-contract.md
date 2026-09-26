# Swarm ↔ Auto Mode integration contract (implementation boundary)

Status: proposed boundary in `codex/swarm-mode`, checked against the separate
`codex/automode-rfc` branch at `11b4cf9` on 2026-09-26. That implementation is not
merged here. Auto's selector, structured quota collection, and durable selected
launch intent exist on that branch; atomic allowance commitment and complete
crash reconciliation remain open. This contract defines their shared boundary,
not an assertion that either side already implements it end to end.

## Responsibilities

Auto Mode discovers/routes eligible harness + provider endpoint + account + model + effort
combinations; obtains health, auth, capability, quota and consumption observations; and
estimates work suitability. Swarm owns category planning, worker count, jobs, attempts,
coordination, category allocation and integration. The daemon owns **one** reservation
ledger and admission transaction for ordinary, Auto, and Swarm launches. It is the only
place a target becomes committed work. Swarm does not maintain a competing account
allowance balance.
No second quota collector, per-harness CLI scraper or user routing file is added by swarm.

## Target snapshot

A snapshot has a monotonically increasing version, `observed_ms`, expiry, provenance,
confidence and one or more `Target` records. Each target has stable target/harness/provider
endpoint/account/model/effort identifiers, a list of applicable shared quota-pool IDs,
capabilities (including message delivery/acknowledgement and native-child control), scoped
health/auth status, and eligibility reasons. A pool/window has native unit, fresh remaining
amount or unknown, reset if known, and scope (account, model, endpoint, or other binding
limit). Consumed tokens/usage remain separate observations. A local provider may have an
explicit `not_applicable` subscription quota; this never means infinite machine capacity.

Linked profiles are grouped by verified account/pool identity. If independence cannot be
proved, their capacity cannot be added. A changed identity invalidates prior route eligibility
and reservations remain until their processes are reconciled.

## Request and transaction

For each logical job, Swarm passes `run_id`, `parent_id`, `job_id`, plan revision,
requirements and capability floor, allowed targets/accounts, per-target exclusions,
`attempts_remaining`, the expected upper draw in each native unit, and
`remaining_allocation` for every applicable pool/window. That remaining allocation
is calculated from the category's frozen allocation, finishing reserve, and all
outstanding/confirmed work; it is a ceiling, not another report of provider balance.
The request includes the observed snapshot version and an idempotency key. A missing
or zero attempt budget cannot be repaired by routing to another harness.

`route(request)` returns an ordered eligible set with rejected alternatives and
scoped reasons. This is advisory: the daemon revalidates the selected target's
health, auth, account identity/generation, capabilities, permissions, snapshot
freshness, every binding quota window, category allocation, attempts, workspace
ownership, and the app-wide agent ceiling in one admission transaction. Its
effective limit is the tighter of Auto's account-window allowance and Swarm's
remaining category allocation for that window; every applicable window must fit.
It atomically records the allowance commitment, concurrency slot, workspace claim,
and durable launch intent before effects. An expired version fails and requests a
fresh snapshot. A changed pool identity invalidates the candidate. No model may
override a deterministic rejection. Repeating the same logical launch returns its
existing intent, never a second commitment.

An admitted run has a durable attempt/launch ID and reserved upper estimates in matching
native units. Actual usage reconciles into the same pools. A known pre-effect rejection
releases the commitment; uncertain launch, process, tool, or descendant effects retain it
until reconciliation proves settlement. Pool capacity is shared with ordinary Overseer
runs and other categories. External use can change observed headroom; it cannot be
reserved here and must be disclosed. Multiple windows each bind independently. A
reset permits a new observation; it does not erase an earlier attempt or create a
second category allocation.

Availability is scoped: a local harness failure, account auth failure, endpoint
outage, model-specific exhaustion, account quota rejection, and ordinary job failure
have different exclusion keys. A public status incident alone is advisory. Known
stale observations and never-known values are distinct: both require refresh before
fan-out, but a known exhaustion remains blocking until a newer authoritative
observation clears it. A confirmed pre-effect failure may try another eligible
target only while the logical job's attempt budget remains. An uncertain effect
pauses without reroute. Replanning does not reset the budget.

This contract does not claim an enforceable hard spend limit when adapters only expose
delayed observations. If no compatible upper estimate or fresh comparable allowance exists,
the default policy declines fan-out while preserving the authorized single-agent path.

## Existing API mapping and compatibility

Existing daemon `account.usage`, `account.list`, `harness.list`, and `profile.status`
are observations and UI discovery, not a reservation authority. Swarm's
`swarm.policy.preview` and `swarm.admit` currently consume caller-provided fixture
snapshots. Auto's `auto.dispatch` persists selected intent and can return
`launch_pending`, but does not yet commit allowance across all launch paths. The
adapter must translate Auto's account-generation, scoped quota/route observations
into a versioned snapshot and submit both Auto and Swarm launches to the same
transaction. It must not infer a balance from `account.usage` tokens or replace
Auto's collector with another parser.

Shared proof is recorded once for each boundary, then cited by both RFC ledgers.
These CONTRACT criteria remain unchecked until a single integrated implementation
passes the specified tests. Their records use the status vocabulary and evidence
fields from `docs/verification/README.md`.

| Shared contract criterion | Auto criterion | Swarm criterion |
| --- | --- | --- |
| [CONTRACT-01](../verification/swarm/CONTRACT-01.md): concurrent ordinary/Auto/Swarm launches and changed identity cannot double-commit a binding window or writer | AUTO-AC-17 | SWARM-08, SWARM-24 |
| [CONTRACT-02](../verification/swarm/CONTRACT-02.md): verified linked accounts share one pool; independent accounts do not; unresolved identity adds no capacity | AUTO-AC-04 | SWARM-09 |
| [CONTRACT-03](../verification/swarm/CONTRACT-03.md): failure exclusions respect account, endpoint, model, and harness scope while unrelated targets continue | AUTO-AC-19 | SWARM-14 |
| [CONTRACT-04](../verification/swarm/CONTRACT-04.md): confirmed pre-effect fallback has a durable logical-job attempt cap; uncertain effects do not reroute or spin | AUTO-AC-19, AUTO-AC-20 | SWARM-15 |
| [CONTRACT-05](../verification/swarm/CONTRACT-05.md): crash/reconnect retains one intent and commitment, reconciles process/workspace effects, and does not duplicate a launch | AUTO-AC-24 | SWARM-22, SWARM-24 |

- [ ] **CONTRACT-01:** Two concurrent launches from different callers, including
  ordinary and Swarm, compete for one known limiting account window and one writer.
  Exactly one commits. A profile/account generation change between route and launch
  blocks admission; replay returns the same intent. Repeat with short and long windows.
- [ ] **CONTRACT-02:** Two linked profiles/harnesses with proven shared pool identity
  consume one allowance; an independent account retains its own. Unresolved identity
  never becomes an additional allowance after quota failure or restart.
- [ ] **CONTRACT-03:** Inject harness-local, endpoint, account-auth, account-quota,
  model-window, and ordinary job failures separately. Only matching routes are
  excluded; an unrelated healthy route and running job continue. Distinguish stale
  known blocks from never-known data and public incident advisories.
- [ ] **CONTRACT-04:** A confirmed pre-effect failure may select another eligible
  route within the same logical job's remaining attempt budget. Two execution
  attempts are the maximum across targets, revisions and restart. An uncertain
  process, tool or external effect pauses without a second launch or planning spin.
- [ ] **CONTRACT-05:** Crash before commitment, after commitment, after external
  workspace effect, after child-row commit and after process start. Two reconnecting
  clients see one durable intent/commitment and one child, or a visible uncertain
  effect requiring reconciliation. Release only after confirmed settlement; no
  duplicate writer, process or model request occurs.

These are common tests with both ordinary and Swarm callers, not two independent
implementations or automatic checkmarks in either ledger. Current fixture tests
prove only portions of the Swarm side. `SWARM-24` stays partial until the actual
Auto producer and shared transaction pass the tests. A boundary change requires a
versioned contract update and compatibility test for active runs; it cannot
silently widen a saved permission or allocation.
