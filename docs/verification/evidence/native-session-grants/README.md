# Native session grant safety evidence

Implementation commit=1129e35eebb2e26a8d631666a009a6418457e61c. Base: finished PR52, 2bb6b1e6130249db45e387906b6bd275e1405f61. This is a bounded AC274 safety correction, not AC274 verification; criteria/ledger edits remain coordinator-owned on main.

The old daemon reused a display label as permission authority. A second Codex command (or even a repeat native ask), changed Claude full-rule type/behavior/destination, changed daemon process/native session ownership, and legacy label-only records could therefore acquire an old Always answer. Claude's installed 2.1.288 can_use_tool schema also provides optional suppress_always_allow_rule, which vetoes the persistent choice even when suggestions exist; the old adapter discarded that veto.

Replay now requires an exact durable typed qualified grant. A canonical SHA256 covers the native descriptor and daemon-owned run/native session/process generation. The new event contains only nonsecret classification and digest. Complete recognized Claude addRules/allow/session tool-wide rules can replay; patterned/unknown rules remain native-only. Codex acceptForSession is still forwarded exactly, but host replay remains false until its actual native cache scope/identity is qualified. Native suppression hides Always, blocks replay, and refuses a stale Always answer before claiming attention or sending stdin. Allow once and Deny remain available.

## Recorded RED

- `cargo test -p overseerd --test answer_waiting ac274_ -- --nocapture`: 11 failed at intended behavior assertions; no setup failure. [daemon-red.log](daemon-red.log)
- `cargo test -p overseerd --bin overseerd ac274_native_suppression_hides_the_claude_always_offer -- --nocapture`: 1 failed because a native-suppressed request still offered Always. [adapter-red.log](adapter-red.log)

The source behavior was unchanged for these runs. An initial --lib invocation was invalid because overseerd has no library target; it was corrected to --bin and is not counted as a behavioral failure.

## Focused GREEN

Every Rust command used `CARGO_TARGET_DIR=/private/tmp/overseer-native-session-grants-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20`. One builder, no full suite or UI, no paid harness turn.

- `cargo test -p overseerd --test answer_waiting -- --nocapture`: 18/18, including 11 new AC274 regressions and existing AC241/helper controls. [daemon-green.log](daemon-green.log)
- `cargo test -p overseerd --bin overseerd always_allow_tests -- --nocapture`: 7/7. Covers complete native rule validation, canonical object identity/conservative arrays, daemon-owned context boundaries, hash-only public projection, Codex native cache authority, suppression, and unchanged native answer payloads. [unit-green.log](unit-green.log)
- `cargo test -p overseerd --test protocol_shapes --test menubar -- --nocapture`: protocol 8/8 and menubar 6/6, including strict real grant-event/negative-shape checks and existing patterned-rule forwarding controls. General protocol check covered 36 methods and 122 events of 22 kinds. [protocol-menubar-green.log](protocol-menubar-green.log)
- Direct fixture-only probes: 5/5 command/full-rule probes plus 2/2 native-veto probes. [fixture-smoke.log](fixture-smoke.log), [native-veto-fixture-smoke.log](native-veto-fixture-smoke.log)
- Node syntax checks, `node protocol/gen-ts.mjs --check`, and `git diff --check` passed. Scoped process scan found no remaining own daemon/shim/fixture processes. Coordinator independent source review found no blocker before commit.

## Limits and remaining work

The changed-native-session regression mutates only the disposable daemon's authoritative stored session while the synthetic harness waits at a bounded gate; it proves host ownership matching, not a live harness migration. The new-process test uses a real daemon follow-up and observes process_generation=2. Legacy compatibility removes the typed grant from the disposable event while retaining its old label. These fixtures make no claim about installed native grant caching.

Installed Claude schema evidence was read from version 2.1.288 at factory offset 180696534, field offset 180698356 (retained privately at /private/tmp/ac274-claude-suppress-always-evidence.json). This establishes the native veto field contract only; no native/model/browser turn or owner profile was used.

Full integration verification remains pending. Broader schema-correct request-family routing, concurrent pending lifecycle, default_to_no policy, browser/extension/external authorization, native Codex cache and patterned Claude rule qualification remain explicit AC274 gaps. No VS Code/TUI/phone browser or rendered approval-flow evidence is claimed. The coordinator must preserve PR53 queue protocol additions when integrating this PR52-based branch. AC274 stays unchecked.

## Log hashes

- `daemon-red.log`: `ee25da9a6c8e8dbafdd9da81d95bba06d05f672a69b6e23927ea419b12ee581e`
- `adapter-red.log`: `4c4f50be66924b6ffabc20b37dcca8f7843e2c8d36196af685795353de7d82ff`
- `daemon-green.log`: `993a580016c1036070091107c8f516191d3a01bc347030074a48c65c1ad96404`
- `unit-green.log`: `e06441d8976e7b3ad4383c8d9bed827edfdd3bd38edebcde27cbe24e4c6736ce`
- `protocol-menubar-green.log`: `c3b13fa5e7f1815ed2bb29bfff437c493a5d3cb2b6445ba98ebb933ba64dd8a2`
- `fixture-smoke.log`: `3aa2ab93d57b96194d3b95276f0067fe0cf00a52ffd15fddbe9daf110ba32774`
- `native-veto-fixture-smoke.log`: `3b7aa9a02df6ce674bb6952624e09fe17b8fd93230a345357d9c79a7e756173a`
