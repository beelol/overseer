# AC269 phone slice: source and lightweight fixture checkpoint

2026-10-03 11:27 UTC. Base: `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. Implementation branch: `codex/mods-phone`. This is the phone portion of Task 5; TUI and Gate S remain separate. No criterion or verification record is changed.

The read-only screen separates the installed library, owner bindings, desired plan and last recorded turn. Prepared/failed/uncertain outcomes are not reported as successful delivery; missing snapshots say no recorded delivery. Unsupported native/global/dynamic-local/child capability facts and inactive Less tool noise remain visible, without savings claims. All rendering uses existing phone components/theme tokens and plain selectable text, with no new assets, links or control outbox operations.

The explicitly adjacent Session change is public `subscribeMods(runId, listener)`. It notifies once per batch for global mods_changed or the matching envelope run_id on mods_applied. It forwards/retains no event body and works when timestamp values do not change. Screen refreshes are driven by that signal, entry/reconnect or explicit refresh; event bursts are coalesced using the existing review debounce/max-wait constants. There is no polling. Target/Mac keys, generations, commit-time key checks and lifecycle cleanup prevent late replies from filling another view; forgetting the Mac clears retained screen data. More offers inspection to full/watch, working/child targets and while the ordinary header is unavailable. Stop/cleanup/archive authority is unchanged.

Evidence:

- `red.log`: three absent public-signal fixtures and five absent menu/route fixtures fail for the intended missing behavior after correcting initial header fixture setup.
- `screen-red.log`: twelve behavior fixtures fail against an empty screen skeleton. The earlier missing-module failure was setup only and is not counted as behavior RED.
- `header-pending-red.log`: valid route with no ordinary header cannot reach More before the final menu correction.
- `signal-green.log`: original eight signal/menu fixtures pass.
- `regression.log`: final eight relevant files, **104/104**, including seventeen screen fixtures, six menu/route fixtures, three signal fixtures, existing Session/hold/learned, Conversation and Changes checks. The existing watch-only test now requires read-only Mods in More while still excluding Stop/Clean up/Archive; its old no-More assertion was superseded by the authorized inspection requirement.
- `target-probe.log`: a reply resolved during the new target's layout commit passes before the explicit commit-key correction too. This proves negative compatibility, not a reproduced runtime RED. The coordinator requested the direct state-key guard to remove dependence on passive-effect timing; the final regression set includes it.
- `types.log` and `lint.log`: whole-phone tsc --noEmit and scoped ESLint exit 0. git diff --check also passed.

All commands used nice -n20 and serial runInBand/Node checks, a branch-specific Jest cache, and the exact pinned lockfile/dependencies from the frozen integration. This sandbox reports `nice: setpriority: Operation not permitted`; requested priority could not be applied. No workaround or system-setting change was made.

**Unrun:** the authored actual encrypted full/watch gateway fixture `ac269_mods_phone_reads_and_all_local_management_refused` in daemon/tests/phone_methods.rs, which reads list/why and attempts every declared mac_only method with valid confirmations/revisions, including update via preview. It checks local library/binding/desired/applied state remains unchanged. Rust/compiler, package/VSCode/device rendered checks and full integration are reserved for coordinator slots; no runtime or rendered qualification is claimed. No dev daemon, owner profile, paid turn, credential or private audio access occurred.

PR69 owns the separate quiet-transcript change in extension/media/conversation.js and phone/model/src/conversation.ts; this branch leaves both untouched. Control-plane events remain observable by the Session signal independent of their transcript rendering.
