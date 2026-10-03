# Gate S mutation governance: source-only fixture checkpoint

Base `60afc4bee5574403359361441e77d9e7687bd576`, isolated clone `/private/tmp/overseer-mods-governance-20261003`, branch `codex/mods-governance`. PR80 stays frozen at `974152c64f219acd70778962ebc8083b4bdf7fef`. The existing [approved Task6 plan](2026-10-03-mods-gate-s.md) and root-reviewed governed-binding fixture memo guide this slice. This checkpoint adds tests and this audit only: no application changes, Cargo, daemon, UI or paid turn was run. Root's combined full run owns resources.

## Source audit and execution boundary

- `daemon/src/server.rs:65–82`: thread-local ACTOR uses `None` for the local user. `with_actor` sets it for a device and resets it after dispatch. Native `overseer.tool` arrives on the same Unix request path; transport alone cannot distinguish native authority from an owner control. The separate pending-agent LocalOwner-context fix is not in this base and is not copied here.
- `daemon/src/overseer/mod.rs:228,289`: native tool calls pass through the common wrapper, then protected `propose`/`answer` take the publication guard, resolve the exact issued capability and capture a typed NativeOrigin. New Mods changes belong inside that proposal path, never direct native `mods.*` calls.
- `daemon/src/overseer/session.rs:934–976`: frozen capability SHA/run/turn/session/cause; `:978–1188`: daemon class, owner-requested Confirm, existing nonquiet Steer policy, level and settling. Native insertion at `:1200–1233` revalidates the exact capability, current session and originating turn/cause under Store lock. Preserve legitimate finding→Steer, and do not borrow an unrelated owner cause.
- `session.rs:1191` recursively redacts checked actions before public proposal storage; `:1364` shared answers claim once; `:1629` effect dispatch. A redacted public action cannot serve as canonical execution data. Store private immutable Mods parameters atomically with the validated proposal, bind them to its exact action identity, and retrieve them using daemon-held identity. Model-supplied private references, action indices, actor/class/role/env/approval fields refuse. No public card, reconnect reply or answer payload supplies new execution parameters.
- `daemon/src/mods/bindings.rs:565–691`: local set/unset check only ambient actor, assign `actor="owner"`, and deliberately allow owner unlock/edit/delete. Do not call these as governed authority by clearing or inheriting ACTOR. Extract the same normalization/pin/conflict/revision transaction with an explicit internal governed policy; a valid proposal grant may change installed text, but never supplies owner-only locks. Revalidate revision/current lock/pin in the effect transaction, attribute the actual proposal, and preserve all local-owner and gateway protections.
- `library.rs:498` removes bindings along with a version; governed removal must reject locked references. Install/update require an immutable reviewed preview, a governed revision precondition and separate Confirm approval. Preview is not installation; update executes through existing `mods.install`, leaves pins untouched and never enables. Their later effect fixtures remain required; this first checkpoint tests classification/No only.

A future typed request-context integration must restore nested caller state safely and distinguish native capability from local-owner transport. The governed policy must remain explicit even if that infrastructure later lands: strings such as `by`, `surface`, `source`, `actor` and a socket location are attribution or presentation, not authority grants. Current finding actions keep their allowed Steer behavior; this is not a blanket refusal of native proposals.

## Authored contracts

`daemon/tests/mods_governance.rs` contains twelve feature cases (plus the four existing common helper tests when compiled):

1. Owner-reviewed bind/unbind in all five exact scopes, pins and ordinary/Overseer separation; unchanged roles, raw runs, turn admissions, tokens, profiles, guardrails and repository permission policy.
2. Owner Steer/Auto settling and cancellation, preserving the existing window.
3. Strict top-level/nested action fields, exact scope/pin identity, unknown/private-reference/authority fields, and the temporary one-operation batching limit.
4. New and existing enabled/disabled owner locks deny governed edits/unbind; manual local-owner unlock permits a newly reviewed operation.
5. Concurrent owner edit/lock makes approval stale, preserves the newer binding and refuses replay rather than silently rebasing.
6. Removed pinned version makes approval stale with no replacement binding.
7. Synthetic credential-shaped canonical Git path and escaped exact-model filter are redacted in public cards/messages/events but preserved in actual private execution; the real repository resolver selects the original pin. No invented model route is used for the filter.
8. Two proposals keep distinct frozen commands; No does nothing; restart preserves the other proposal; replay cannot execute the declined action.
9. Concurrent answers on the shared daemon boundary with existing VS Code/TUI surface labels yield exactly one mutation/revision/event; labels do not claim physical UI/device qualification or supply new parameters.
10. Genuine finding-driven harness/MCP calls keep nonquiet Steer open at Steer and may apply at Auto; library Confirm and owner-lock edits refuse. Lock denial is attempted at the current revision before the legitimate Auto bind, so a stale revision cannot mask ambient LocalOwner inheritance.
11. Genuine native finding held across Fresh cannot publish a command into the replacement default-owner session. Cleanup waits for the old actual run, not the new blank session.
12. Owner library preview/install/update/remove remain Confirm at Auto; No leaves Mods rows/revision/event count unchanged. These are classification contracts, not execution qualification of library operations.

Fixtures use actual isolated daemon APIs and temporary repositories/bundles. SQL reads observe durable evidence; no fabricated run, cause, proposal, private command or revision is inserted. Native finding cases reuse the actual existing gated harness/MCP fixture; most owner cases use the local-owner proposal/answer boundary. Denials compare raw Mods rows/revision/change-event counts; strict proposal refusals also compare proposal rows. Public projection and independent raw binding effects establish the canonical execution requirement without inventing a private table schema.

## Baseline and later gates

The current implementation does not recognize `action:"mod"`. Positive prerequisites are deliberate: the eventual baseline must classify missing-action assertion failures separately from fixture setup failures and from negative cases already denied by the old generic parser. Formatting parse and diff checks are source checks only, not compile/test evidence. No RED or GREEN is claimed at this checkpoint.

Before application edits: root reviews this checkpoint; an exclusive bounded slot rebuilds the standalone daemon and no-run executable, verifies source/.d/hash identity and observes the baseline. Preserve each exact failure and stop on setup/artifact errors. Private immutable storage/guarded transactions then follow genuine failures, with reviewed green and incumbent owner/native/voice/text/gateway checks. Confirm library effects (preview/install/update/remove), lock-race isolation at the internal mutation boundary, voice/text compatibility and actual cross-surface/device confirmation remain later fixtures; full AC-269 governance is not complete. No transformer/code-program installation, public library changes, private audio, paid runs or production configuration belongs here.
