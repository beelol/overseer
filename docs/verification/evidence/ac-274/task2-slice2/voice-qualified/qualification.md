# Corrected simulated Voice qualification

Coordinator command 37877 passed exactly `slice2::voice_checked_readback_keeps_typed_revision_and_exact_native_decline` **1/1** at `3fe336f3ce3e904b2a6b5475006dd930d87fcf15`. The unchanged assertions reached the captured opaque request key/current revision, one exact frozen native decline, confirmed_voice provenance and answered-awaiting-native state. This is a simulated/scripted listener plus synthetic output, not a real microphone, native speech, installed-provider or browser qualification.

The prior [six-case surface phase](../surfaces-qualified/qualification.md) remains five reference-phone passes and one missing-listener setup failure before assertions. The separate test-only correction supplies a strict same-source listener build with explicit listener path, fake synthesis and disposable cue log. It does not change product authority or weaken any assertion. No failed-build/stale-executable fallback.

## Actual artifact and command proof

- Listener package clean 0.517s; standalone listener build 8.441s, fresh:false. Actual executable SHA256 `95c0ef697640d1782416f1842507c17ee4576aec39580edaab997e16decd6003` and source `.d` are retained.
- Pending integration no-run 1.487s, fresh:false. Actual executable SHA256 `7a65b746c797abc4973a490ae7cd31cb201d8f03611a66217a13eddf0db64f11`, test source SHA256 `b6c0efd22661f3acac5bdb0558b14e16caeac765c96fa9f2bbdefaf6d833463c` and `.d` retained.
- Standalone daemon was unchanged and reused from 9040, SHA256 `1c542a4dbc9adf765da1c540526bd2d70909a295f3bbff521b296e82c7b205f8`. No new daemon build is claimed; the earlier fresh:false record remains in [core qualification](../core-qualified/qualification.md).
- Exact case command 2.914s, one inventory match; raw stdout/stderr and bounded-capture exit/signal/timeout/output-cap/reap metadata retained. Coordinator reports NI 20 asserted before child launch; a later ps found it already terminal, so no observed live PID is fabricated. One build worker/test thread in the coordinator-allocated target.
- Actual cleanup JSON `{"ours":[],"others":[]}` and cleanup 0.463s retained. Baseline process-ID inventory omitted. Shared verify target has subsequently been allocated elsewhere; these saved receipts do not authorize future executable reuse without fresh source/hash proof.

Only relevant original compiler-artifact lines are preserved, not full Cargo JSON. Existing raw streams are copied unchanged and hashed; no logs are fabricated. This publishing editor launched no runtime/build/native/UI/provider calls. Scope remains partial AC274: real browser continuation, external authority observation and all rendered typed VSCode/TUI/phone capability controls require separate implementation and qualification.
