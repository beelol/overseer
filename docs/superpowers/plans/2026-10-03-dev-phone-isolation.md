# Dev Phone Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This is a plan only; implementation waits for the coordinator's builder/native slots. No extra owner planning approval is required.

**Goal:** Pin simulator builds to one dev daemon and prevent a release phone from consuming a dev pairing code or saving a dev pairing (AC210/213).

**Architecture:** Carry a standard-only or exact-dev instance policy in the authenticated first Noise payload, and enforce it on the gateway before pairing/session mutations. Validate the authenticated gateway reply again before saving phone keys or starting a session. Native launch/discovery capabilities enforce build identity; the existing dev launcher and phone lab provide isolated dual-instance verification.

**Tech Stack:** Existing Rust/Tokio/Snow gateway, TypeScript phone-core/session/platform interfaces, Expo 57 native modules (Swift/Kotlin), Node dev tooling, existing Maestro simulator runner. No additional crypto stack or general Gate S method exposure.

**Spec:** `docs/overseer-rfc.md` AC210/213; `docs/rfcs/dev-instance.md` phone and isolation sections; `docs/rfcs/phone-remote-protocol.md`. Inspected `origin/main` at `37ca03319a22bbf14ca1c5a702f0ab1fc6b8c00c`; gateway and phone code is the same relevant baseline as the coordinator's cited 715 source. PR53 remains frozen at `f02665a9` and is not this implementation branch.

## Global Constraints

- AC210 command: `scripts/dev phone --name x --platform ios|android`; iOS `127.0.0.1:<port>`, Android `10.0.2.2:<port>`; exact dev identity `dev-<name>`; no accidental pairing with standard.
- AC213 release discovery is only `_overseer._tcp`; dev advertisements are only `_overseer-dev._tcp`, with `inst=dev-<name>`, and only with explicit `--mdns`.
- Derive daemon identity from existing `paths::instance()`; startup guard reconciles the on-disk binary marker with its environment before the gateway starts. Do not trust a claimed phone surface, discovery TXT or leaked environment as daemon identity.
- Standard means the lab's standard daemon under temporary HOME/data/socket. Never use, connect to, stop, inspect credentials of, or pair with the owner's installed daemon/phone.
- Preserve Full/Watch authorization, existing pairing confirmation, revoked-device behavior, counter replay checks, exactly-once requests and outbox/cursor replay. No default automatic acceptance of a pairing.
- Fixture-only, no paid Claude or other paid turns. All builds/tests nice20, absolute isolated target and coordinator slots; no full run or native scenario while the machine lock/quiet window is held. Native checks are opt-in ON SCREEN and skipped is not passed.
- No production deployment, TestFlight upload, owner device installation, new package generator, main ledger/criterion edit or unrelated phone128/246 implementation. Preserve `/private/tmp/overseer-phone-overseer-plan.md`.

## Exact Verify obligations and current evidence

**AC210 Verify:** app pairs with dev A while a standard gateway is also on; paired `fp` is A's, agent list is A's; standard pairing codes/devices unchanged; two dev ports/fingerprints differ. Native iOS and Android pin flows will cover this. A command dry-run or JS fake cannot substitute.

**AC213 Verify:** actual gateway test proves dev advertisement only on the dev service and instance in authenticated handshake; native release e2e lists/pairs standard with dev also on and refuses typed dev address clearly. The current `scripts/dev` phone function (599) exits2; `gateway/mod.rs` advertise (313) always uses the standard type; the final encrypted reply (547) lacks instance. `session.ts` GatewayHello lacks it. Pairing currently sets `p.used` and emits confirmation before that reply, then inserts a device: adding a reply-only check is too late.

Both `discoveryBrowser.ios.ts` and `.android.ts` currently report unsupported, recorded as AC120 partial. Consequently release listing cannot be honestly verified until native discovery is implemented. The pairing/pin guard can be an independently reviewable first slice, with AC213 explicitly partial until Task4 + native evidence. Android emulator cannot see the host LAN's multicast; its pin/typed-address checks remain required, while actual discovery must be proven on iOS simulator or a separately authorized Android test device/network. Do not fake native discovery to close the criterion.

## Review Focus

1. Typed address or old gateway bypasses service filtering: refuse before consuming code/inserting device; fail closed against legacy gateway.
2. Dev A is down and stored/manual/Bonjour data points to standard or dev B: no fallback; exact pin/fingerprint required on pair and reconnect.
3. Release app receives dev launch arguments or leaked environment: native compiled release gate ignores them; standard policy stays enforced.
4. Code is open while mismatch/refusal happens: code/PSK availability, pending requests and devices stay unchanged; original intended client can still pair.
5. Native rebuild/relaunch or changed dev port/key: no stale standard cache/keys/outbox shown or replayed into another instance; explicit pin mismatch is visible.

---

### Task 1: Gateway identity before mutation, including legacy refusal

**Files:** Modify `daemon/src/gateway/mod.rs`, `daemon/src/gateway/noise.rs`, `docs/rfcs/phone-remote-protocol.md`; Test `daemon/tests/gateway.rs`, `daemon/tests/dev_instance.rs`, existing `daemon/tests/common/phone.rs` handshake helper.

**Interfaces (proposed, not existing):** first authenticated device hello adds mandatory `instance: string | null` for the instance-aware frame; null means standard-only, string means exact dev. Normal and revoked/refusal gateway hellos carry authoritative `instance: string | null`. Use a typed Rust policy parser/matcher, not boolean surface inference. Refusal returns an authenticated typed `instance_mismatch` error without pretending a device was paired. First-frame parser must preserve the negotiated framing version.

- [ ] Add red tests: standard client to dev gateway; dev-A policy to standard/B; valid A; pairing code remains usable; no `pairing_request`, device row, `p.used`, consumed secret or last-counter mutation on mismatch. Include revoked and Watch paired-key cases without changing their authorization.
- [ ] Run exact new gateway tests and capture the actual pre-fix failure; also retain existing pairing/replay tests.
- [ ] Gate **after successful Noise first-payload decryption and before** `device_accept_counter`, `p.used`, pending confirmation, device insertion or app-key save. Compare with `paths::instance()`; send authenticated instance/error then close on mismatch. In revoked path report actual instance too.
- [ ] Fail closed to gateways that cannot enforce this before mutation: propose first handshake framing version2 for new clients (transport chunk framing and daemon RPC protocol version remain separate). Old gateway rejects frame2 in existing `first_frame` before mutation. New gateway accepts legacy frame1 only on standard; dev refuses it before mutation. Do not downgrade/retry frame1. Update test helper/benchmark inputs and parser checks. Coordinator review must confirm this bounded compatibility contract; alternative must prove the same old-gateway no-mutation property, not just a late client rejection.
- [ ] Green commands: targeted `cargo test -p overseerd --test gateway ac213_` and first-frame parser checks, then existing pairing/replay/revocation subset. Commit this independently reviewable safety boundary.

### Task 2: Phone core fails closed before saving a pairing or session

**Files:** Modify `phone/core/src/session.ts`, `client-types.ts`, `client.ts`, `errors.ts`, `pairing-store.ts` only if persisted identity is required; Tests `phone/core/test/session.test.ts`, `client-pair-once.test.ts`, `client-resume.test.ts`, `types.test.ts`, `mock-gateway.ts`. Preserve the separate PR53 follow-up result union.

**Interfaces (proposed):** `ClientOptions.instancePolicy` defaults to standard-only; exact-dev policy also contains the launcher fingerprint and explicit address. `DeviceHello.instance` and typed `GatewayHello.instance` mirror Task1; a typed handshake refusal is distinct from successful device/scope hello. Report `instance_mismatch` distinctly through existing ConnectFailure/PairingError paths.

- [ ] Red tests: authenticated dev reply on standard-only, wrong instance, malformed/missing policy/identity, frame1-only gateway, correct A, correct standard and impostor key. Assert no `Pairing.save`, secret write, paired event, session online, outbox send or cursor advance on refusal; generated temporary private keys are wiped. Assert refusal survives reconnect and is visible, without deleting an unrelated stored pairing.
- [ ] Validate the launch pin's expected fingerprint against pairing-code public key before opening any pairing socket. Filter pairing/reconnect candidates to the explicit pin; neither code address order, platform loopback defaults nor manual extras may fall back to standard/B. Enforce authenticated hello identity before `LiveSession` construction and before `client.ts` saves Pairing. Normal release default is standard-only and requires the instance-aware first frame; no compatibility downgrade.
- [ ] Persist dev state in a distinct instance namespace using existing `ClientOptions.namespace` and scoped app stores; isolate session cache, cursor and outbox as well as core keys. An instance or gateway-key/pin change must select a fresh namespace and drop/quarantine old pending outbox data **before constructing the client**; never just update its address. A regenerated gateway key requires a visible mismatch and deliberate new pairing, not silent key replacement. Add a test that an old pending control request is never sent to the new pin.
- [ ] Run core strict typecheck + targeted session/pair/reconnect tests, then core suite. Commit only this boundary and tests.

### Task 3: Native build identity and simulator pin capability

**Files:** Modify `phone/src/platform/capabilities/launch.ts`, `native/launchShared.ts`, `native/launch.ios.ts`, `native/launch.android.ts`, `native/index.ts`, `fake/launch.ts`, `phone/src/session/create.ts`, `phone/app/_layout.tsx`, `phone/src/config.ts`; tests `platform/__tests__/native.test.ts`, `fakes.test.tsx`, `session/__tests__/session.test.ts`. Create a small hand-authored local Expo module `phone/modules/overseer-instance/` with `package.json`, `expo-module.config.json`, `ios/OverseerInstance.podspec`, `ios/OverseerInstanceModule.swift`, `android/build.gradle`, `android/src/main/java/expo/modules/overseerinstance/OverseerInstanceModule.kt`, and a typed JS adapter. Configure local autolinking explicitly via existing Expo autolinking `nativeModulesDir` convention; no file generator or edits to disposable prebuild output.

**Interfaces (proposed):** `LaunchInfo.instancePin` is null or immutable `{ instance, host, port, fingerprint }`. Native `getInstancePin()` reads iOS process arguments / Android intent extras only in a compiled developer simulator build. A release module always returns null regardless of arguments; shared screens never branch on platform or trust JS `__DEV__` as the authorization boundary.

- [ ] Red capability tests: valid exact pin, invalid name/port/fingerprint, partial arguments, non-simulator and release build, argument injection; release remains standard-only. Native verification must exercise compiled release too, since Jest cannot prove the compile-time guard.
- [ ] Implement native pin parsing and expose it through launch capability; app root injects it before constructing session or reading stored pairing/cache. Configure dev discovery type/filter and its default port from that pin. Visible connection copy names expected dev instance when unavailable; do not retry another daemon.
- [ ] Run phone lint/typecheck + named capability/session tests. Native compilation waits for assigned slots. Commit.

### Task 4: Honest dev/standard service discovery

**Files:** Modify `daemon/src/gateway/mod.rs` advertiser; existing `phone/src/platform/capabilities/discovery.ts`, `discoveryCore.ts`, `native/discoveryBrowser.ios.ts`, `.android.ts`, `phone/app.config.ts`; tests `daemon/tests/gateway.rs`, `phone/src/platform/__tests__/fakes.test.tsx`, `native.test.ts`. Extend Task3's small local module with native browser methods/events rather than adding a second native dependency.

**Interfaces (proposed):** `DiscoveryConfig.instanceFilter: string | null` selects standard or exact dev. Native browser reports existing `DiscoveredGateway` records; core filters `inst` exactly, filters on update/removal as well as initial find, and never treats TXT as authenticated identity. Production service type is fixed even if dev config is injected into a release build.

- [ ] Gateway red tests observe `_overseer._tcp` and `_overseer-dev._tcp` with unique fp: default dev advertises neither, explicit mdns dev appears only under dev with exact inst, standard only under standard, off/clean withdraw both. Existing `advertised()` macOS dns-sd helper needs a service-type parameter; no textual source grep as evidence.
- [ ] Phone red tests reject dev/missing/wrong-inst records for the chosen service/pin, duplicate service names with other keys and record changes; support/denied permissions show their true state. Implement actual iOS service browser and Android NSD behind the existing capability, with cancellation/cleanup. Release plist only permits standard service; dev native build permits dev service. App manifest/local-network copy uses existing token/theme/platform rules.
- [ ] Run targeted gateway mdns and platform tests. Before marking AC213 complete, capture actual native standard/dev service list and typed-refusal flow. If native browsing remains unsupported, commit only safety/pinning with AC213 partial and defer this clearly identified task; do not substitute fake-record filtering for native evidence.

### Task 5: Replace scripts/dev phone stub with the actual pinned launcher

**Files:** Modify `scripts/dev`, `phone/scripts/lib/run-shared.mjs`, `run-ios.mjs`, `run-android.mjs`, `phone/e2e/device.mjs`; test `test/dev/run.js` and add focused launcher command tests alongside current dev tests.

**Interfaces:** Existing `instance()`, `state()`, instance metadata `repo/port/socket/home`, existing native runner build/install/launch commands. Add explicit runner flags for instance/address/fp and `scripts/dev phone --dry-run` returning exact command/pin JSON. These are proposed additions; runners currently accept only --release/--no-bundler.

- [ ] Red tests: stub exit2 replaced; missing/down instance and malformed platform refused before launching; dry-run emits IOS host/Android host and exact metadata port, own checkout, instance/fp; no default47810 or owner daemon path. A stopped A with standard on refuses rather than opens standard. Stub command tests must not require Xcode/Android.
- [ ] Before enabling its gateway or launching, call hello over that instance's exact socket and compare instance; query/enable phone access on **that** dev only and read its actual fp. Build from metadata repo with existing native scripts, launch compiled dev app using native arguments/intent extras, pin exact address. No automatic pair_start/confirmation in the ordinary launcher; lab scenario owns its explicit disposable pairing flow.
- [ ] Extend existing launch helpers without overwriting the owner's installed physical app or default Mac daemon. Rebuild identifies source SHA/app build; cleanup records owned simulator/Metro processes. Update stub/help text. Run dev command tests; commit.

### Task 6: Dual-instance native evidence and integration

**Files:** Extend `phone/e2e/scenarios.mjs`, `run.mjs`, `lab.mjs`, `device.mjs`, add `phone/e2e/flows/dev-instance.yaml` and `release-instance.yaml`; create opt-in `test/ui/scenario-phone-dev-instance.js` with first line `// ON SCREEN`. Coordinator owns main ledger/criteria.

- [ ] Isolated standard lab uses temporary HOME + unmarked executable (copy binary without dev marker if needed); dev A/B use named dev homes/sockets/markers. Strip inherited instance/home/socket variables deliberately. Start standard/A/B together with fixture agents and own ports; snapshot standard devices, pairing code identity/availability, clients/state. No owner's default address/47810 access.
- [ ] Native developer iOS/Android: launch through scripts/dev phone for A, enter A lab code/confirm on A, prove paired fp=A and displayed agents onlyA; standard snapshots unchanged. A/B ports/fps differ. Switch explicit pin toB in fresh instance namespace, stopA, reconnect tests must never show standard/B forA. Record exact build identity, arguments and gateway metadata.
- [ ] Native release iOS discovery: standard+dev mdns on, actual list contains standard and excludes dev; pair standard. In isolated fresh app test state, type dev address and valid dev code; authenticated refusal says dev cannot be used by release. Before/after dev pending/code/device snapshots prove refusal before mutation. Android release typed-address refusal and pin safety also run; Android multicast listing requires an actually supported network/device and is never inferred from emulator fake data.
- [ ] Inject dev launch arguments into native release and verify it remains standard-only. Refuse old frame1 gateway before its pairing mutations; show wrong dev fp/missing identity failures separately. Preserve screenshots of release list, refusal, developer pin/agent list, structured assertions and cleanup status under `docs/verification/evidence/ac-210-213/` with exact implementation SHA. No screenshots alone for database invariants.
- [ ] Coordinator grants separate native slots, then run existing e2e runner with explicit new scenario names and `--skip-measure` per platform; build current source (no stale --skip-build). Opt-in joint test only via `scripts/test-all --jobs=1 --only=phone-dev-instance`; default full suite never requires Xcode/SDK/emulators. Missing platform/discovery is skipped with gap, not passed. Clean owned daemons/Metro/apps/windows/shims in finally blocks.
- [ ] Fresh source review then coordinator's one throwaway-merge full suite. Proposal for main records: AC210 verified only after actual pinned pairing/list/fp and untouched-standard proof; AC213 verified only after real dev advertisement/handshake and native release list/standard pair/typed dev refusal. Otherwise partial with precise native/discovery gap. Push criterion work without force, keep draft until required gates; deploy remains separate owner-authorized work.

## Self-review / bounded execution recommendation

Exact Verify clauses map to Task6, with early no-mutation protection in Tasks1/2, immutable compiled pin in Tasks3/5 and actual discovery in Task4. All five Review Focus conditions have explicit tests. Existing APIs/paths were inspected; all additions are marked proposed. No claimed test pass, native proof, owner/live evidence or source implementation occurred during planning. The key bounded first PR is Tasks1/2 (gateway/release pairing guard) plus independent focused tests; AC210 launcher/native-pin follows, and native discovery is an explicit dependency for full AC213 completion. This plan does not weaken criteria or ask the owner for a repeated approval, and does not displace the frozen queue branch or the preserved AC128/246 plan.
