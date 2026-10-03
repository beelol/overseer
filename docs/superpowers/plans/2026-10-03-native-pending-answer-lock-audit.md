# AC274 Slice2 source lock/authority audit

Source audit against published0439c584. No runtime wiring or Cargo execution in this checkpoint. Existing eleven reached missing API/gateway/receipt boundaries remain genuine baseline RED; the twelfth veto case remains a preserved collection setup failure with an unexecuted bare-version correction. Five held-boundary tests below are authored, not executed or qualified.

## Authority entrances

`daemon/src/server.rs::connection_loop` checks the owner Unix peer in `serve`, then calls dispatch on a blocking thread. This is the local-owner entrance. `gateway/remote.rs::handle` has authenticated `Ctx.device_id` from Noise, reads current scope per request, and passes only a display `phone:<name>` into `server::with_actor`. The display ACTOR is not authority. Internal callers of `server::dispatch` also exist in `overseer/session.rs` and daemon Auto metadata, so defaulting every dispatch to local-owner authority would authorize internal/model calls incorrectly.

Use a separate crate-private trusted context with scoped restoration: LocalOwner only at the accepted Unix connection, Device(device_id) only at the authenticated gateway entrance, and explicit ConfirmedOverseer/ConfirmedVoice context only after existing proposal/read-back confirmation. New typed dispatch with no trusted context refuses. Never derive authority from request parameters, `by`, surface labels, model-generated proposal contents or `server::actor()`. Existing legacy method context/classification stays intact. The typed method is Gateway Control and GateS Confirm; adding a class alone must not bypass the existing proposal confirmation lifecycle.

Owner-management mutations are exactly `gateway/local.rs` device_revoke/device_scope, calling `Store::device_revoke`/`device_set_scope` in `gateway/devices.rs`. No other production call sites currently change these fields. Both take the shared per-device gate before Store; revoke releases it after durable mutation and before session closure/notification. Final answer takes device gate → owning process gate → Store, rechecks full/unrevoked state there, drops Store before bounded control I/O and holds device/process gates until disposition. A revocation acknowledged while an answer is held wins; one arriving after a write begins waits for that bounded disposition. Remote once-cache invokes its closure without holding Store/inflight mutex, so it does not reverse this order.

## Process-generation boundary

Use a per-actual-process-run gate, never a displayed child gate. Store is always last. Gate-map mutexes are held only to clone the Arc and are dropped before gate acquisition. Preserve outer incumbent profile/workspace/work-unit/Stop-handoff task gates; answer never acquires those outer gates.

| Existing path | Required integration boundary / reentrancy decision |
| --- | --- |
| `daemon.rs::spawn_process` two `set_run_process` calls | Process gate before publication, held through owned supervisor spawn and committed process identity; launch qualification remains bounded outside Store. No path holding this gate may acquire profile/workspace/task gates. |
| `reattach_unrecorded_director` / `reattach_unrecorded_worker` | Process gate before the final Store guard/publication; private file/identity inspection can precede the gate, but exact expected generation is reread under it. Startup does not acquire SQL transaction first. |
| `apply_lines` | Process gate before Store/transaction and exact-generation check; native resolution/claim arbitration shares it. Drop before publishing events or invoking any downstream callback/control path. Any native machine reply uses the generation-frozen socket rather than `control_socket` rereading a replacement. |
| `mark_ended`, trusted `mark_failed_unstarted` | Public wrappers acquire process gate, then private checked settlement. Keep accepted exact observed-generation and separate same/exact-next + NULL-process launch-failure proof. Already-held callers use the private checked function, never reenter the gate. |
| `finalize` | Acquire process gate and verify exact generation before marker interpretation, auto-budget reporting and `handoff::park`; currently park precedes mark_ended. Call private checked settlement under that same gate. `park` only uses Store/Continuity bookkeeping and emits; it does not acquire task/profile/workspace gates or launch. |
| `interrupt_with_origin` | Handle process-less Continuity stop and recursive managed-child interrupts before taking this run's process gate; `stop_waiting` calls public mark_ended, so acquiring first would reenter. Then reread actual process and write Stop marker/contact its exact socket under the gate. Typed final validation refuses an already marked Stop. Preserve stable task gate from queue/Continuity and parent/child ordering. |
| same-process follow-up `start_turn_internal` | Existing profile/workspace gates precede process gate. Serialize actual native stdin send/turn rollover; do not put a process gate around `handoff::before_follow_up`, which can recurse into launch. Explicit native turn/context freshness must be checked, not fabricated from caller context. |
| typed final answer | Snapshot owning run/generation/socket under process + Store guards, pure-codec validation against private native descriptor/context, and immutable claim transaction. Release all guards for test holds; reacquire and recheck same attempt/generation/native resolution/device before writing. Never use the general fresh `control_socket(run)` helper after a held claim. |

`apply_norm` currently mutates only Store/TailState while the transaction is held. `mark_ended_checked` uses a savepoint, Store helpers, event publication and measurement recording; it does not acquire the outer task/profile/workspace gates. Tail loop invokes apply/finalize/mark independently without holding Store. These call-site properties must remain true in the final source diff; the audit is a design checkpoint, not proof of implemented locking.

## Durable claim and receipt

An immutable private attempt row keeps delivery_token, request_key, exact generation, response digest, trusted actor provenance and first disposition, never raw answer bytes. The pending row may point at that attempt; an authoritative definitely-unsent restoration increments revision and requires a new explicit action/attempt. Retain old attempt and phone once-cache disposition. Native resolution/retirement must cover claimed and awaiting-native rows as well as pending rows; it is not operation completion. A rival cannot replace a claimed token while fixture guards are released.

The shim LaunchFile gains a daemon-issued expected generation with a default for old files; absent qualification cannot accept typed writes. `request_reply` validates exact generation/token/SHA256 and writes a private durable prepared receipt before touching stdin. Mutex acquisition and nonblocking pipe write share one deadline. Restore pipe flags before legacy writers. A same token/digest request returns its existing receipt and never rewrites bytes; changed digest is receipt_conflict. Successful full write/flush becomes written, a pre-touch bounded refusal is not_written, and possible prefix/crash/ack loss stays uncertain. Daemon control connect/write/read has one bounded overall deadline while final authority is held. Receipt queries are private and bounded; digest alone never reconstructs/replays an answer.

## Confirmation freshness

Existing Voice `ReadBack` and `Answering` carry request ID but no revision; existing Overseer permission proposals capture request selector but must also freeze typed revision. Extend only the typed compatibility path to retain opaque key + revision through the existing confirmed read-back/proposal, using trusted confirmation origin. Structured questions/profile/form/external acceptance cannot inherit legacy bool approval. A missing revision on a stale typed read-back must refuse rather than read a newly restored revision. Local typed collection answers always submit exact revision. The existing legacy bool routes remain qualified separately.

## Newly authored held tests (unexecuted)

`daemon/tests/pending_requests.rs::slice2` adds startup-env-only AnswerGate plus an owned local answer worker, with release/reap during failure unwinding. Markers contain exactly phase + opaque request_key. The test helper rejects an answer that returns before its intended hold, so missing API cannot become a timeout classified as race proof.

- native_resolution_after_claim_before_send_prevents_reply: actual shim/tail resolved frame after SQL claim; zero response, independent second request then exact frozen reply.
- replacement_generation_after_claim_before_send_rejects_old_attempt: actual Stop, paused followup, explicit resume and changed generation; stale answer cannot reach successor; exact fresh response control.
- full_device_downgrade_before_claim_cannot_send: actual Noise full device, owner scope mutation acknowledged during validation hold, watch_only and zero bytes, then explicit local control.
- revoked_full_device_before_claim_cannot_send: real paired device/session closure, immutable private once-cache revoked refusal, zero bytes, healthy explicit local control.
- device_revocation_after_claim_before_send_prevents_held_reply: real durable claim then acknowledged revocation; authoritative presend restoration requires a fresh revision/local action, old phone first result immutable.

Other approved held fixtures (partial pipe, lost ack/query, crash, full-phone vs local arbitration, compatible voice, external refusal) remain required before Slice2 acceptance. These tests and later source are neither shippable nor a full/native/browser/three-surface claim. Compiler remains held by coordinator; no tests in this checkpoint were run.
