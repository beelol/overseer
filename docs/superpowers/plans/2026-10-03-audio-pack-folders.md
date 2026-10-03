# Audio pack folders implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Tasks use test-first source checkpoints and an explicitly allocated runtime slot.

**Goal:** Select Built-in or From folder through one twelve-key manifest resolver, with safe in-place playback and local VS Code/TUI controls.

**Architecture:** A focused pack module validates opened descriptors; a source module owns atomic persistent settings and a path-free public projection; one bounded player consumes the opened validated file. The semantic classifier agent owns canonical Line metadata, event identity, coalescing, urgency and freshness. Existing single-daemon playback and Voice arbitration remain shared.

**Tech stack:** Rust daemon/TUI, serde JSON, Unix descriptor APIs; existing VS Code QuickPick/folder dialog and Node tests. Keep dependencies pinned; no new dependency or generation tool selected in this plan.

**Spec:** `docs/rfcs/audio-lines.md`, AC-287/288 in `docs/overseer-rfc.md`, `docs/audits/2026-10-03-audio-twelve-trigger-inventory.md`.

## Global constraints

- Base `79a334cd0a1b334cae80e4654ceafa244f8677e8`, normally merge published main `990d784a00e948c217cee79839c78bfe2a9f9b6d`. Branch `codex/audio-pack-folders`; no main ledger/criterion edits.
- Exactly twelve canonical keys; schema 1 `audio-pack.json` with id, label and lines. Manifest <=64 KiB, each mapped WAV/MP3 <=8 MiB and <=15 seconds; no eager whole-pack loading, fallback or extra exposed cues.
- Private folders are read in place, never copied/cached/uploaded/committed. No private POD access, native audio generation, owner profile or paid call. Synthetic PCM fixture files prove routing/validation only, never spoken content.
- Runtime/build/UI checks wait for root allocation; no production implementation before genuine observed RED. Full suite/rendered/audition evidence remains required before readiness.
- **Asset gap:** existing Reactor synth cues do not prove the currently written twelve spoken lines. Root is clarifying Built-in content with the owner. Do not relabel assets, manufacture completion or generate replacements meanwhile; valid private/synthetic folder work proceeds independently. Built-in availability must be truthful until its approved distributable assets and required content/audition checks exist.

## Review focus

- Open/check/use race: a leaf, parent or selected root replacement must never hand an outside/special-file path to the player (Task 1).
- Selection failure or competing clients: validated source commits atomically and does not enable Audio Mode or silently adopt a rival revision (Task 2).
- Queued/in-flight work: a source switch affects queued resolution, lets an existing clip finish; disable/exit cancels and reaps the player and invalidates automatic cues (Tasks 2/3, semantic integration).
- Missing legacy/private folder: preserve its private selection and show unavailable; no default fallback or path in remote projection/errors (Task 2).
- UI disconnection/cancellation: stale replies cannot become a new selection; cancelling local picker/form changes no setting and restricted clients cannot select a Mac folder (Task 4).

## Shared interface and ownership

`queue_pause` owns `daemon/src/audio/lines.rs`: `Line::ALL`, `Line::parse(&str)->Option<Line>`, `key()->&'static str`, `phrase()->&'static str`, `urgent()->bool`; plus semantic identity/subscriber/Cue/arbiter freshness and Voice nonspoken feedback.

This branch owns `audio/pack.rs`, `audio/source.rs`, `audio/player.rs`, settings/preview/resolution hunks in `audio.rs`, local controls and protocol/classification. Shared `audio.rs` edits are coordinated by hunk; no independent full rewrite.

- `source::snapshot(&Arc<Daemon>)->Result<SourceSnapshot>` gives private selected source identity and availability without materializing assets while off.
- `player::play(&Arc<Daemon>, Line, preview: bool)->Result<()>` resolves the **current** source after semantic freshness and arbiter waits. It never consumes a Cue's old source snapshot.
- `player::cancel(&Arc<Daemon>)` is idempotent and reaps the active player on disable/exit. Source setter calls the semantic runtime's agreed enablement epoch/cancel/drain helper; source changes do not invalidate otherwise-live queued cues.
- Existing explicit synthetic sink retains `pack-id:canonical-key`, so both agents' tests can count key suffixes. It executes validation/resolution before recording, never skips private-file safety. Default logs contain no source paths/media bytes. Additional race/control hooks stay test-only.

## Task 1: one safe manifest/media resolver (AC-287)

**Files:** create `daemon/src/audio/pack.rs`; tests `daemon/tests/audio_packs.rs` and module unit tests; later approved assets under `daemon/assets/audio/default/`.

**Interfaces:** `Pack::open(root:&Path)->Result<Pack>` validates schema/all twelve mappings; `Pack::open_line(Line)->Result<OpenedLine>` returns an owned regular descriptor and duration/format, never a pathname subsequently reopened by the player. Private pack metadata stays private.

- [ ] Author daemon-boundary baseline fixtures first: exact canonical key inventory; twelve valid synthetic WAV mappings; missing/unknown keys, schema/type and unsafe mapping refusal; no copied audio in daemon data; unchanged prior selection on failure. Observe intended assertion RED separately from setup/compiler failures in allocated slot.
- [ ] Add focused descriptor/type/format tests before implementing their behavior: manifest64KiB boundary, sparse8MiB+1 file, 15-second boundary, malformed WAV/MP3, absolute/traversal paths, leaf/parent/root replacement and symlink/FIFO cases. Deterministic pause hooks are test-only and bounded.
- [ ] Implement anchored directory descriptors and component-wise no-follow/nonblocking open, regular-type/size checks, bounded manifest read and validated relative paths. Retain the exact opened regular file through bounded format/decode validation and player handoff. No `canonicalize` then pathname reopen; no FIFO-blocking open.
- [ ] Select a bounded WAV/MP3 validation implementation after inspecting existing platform facilities; must reject malformed/unsupported content, not merely inspect RIFF bytes. Keep memory <= one8MiB file plus fixed decoder buffers, operation deadline and child cleanup explicit. Do not add an unreviewed decoder/probe dependency.
- [ ] Built-in embedding/materialization uses only approved distributable folder bytes, owner-only versioned internal storage and the same Pack loader; no hard-coded key/file mapping. With assets unqualified, keep status unavailable and tests honest.
- [ ] Run focused file/unit tests, review and commit/push this slice. Full spoken-content/actual packaged asset audit remains an explicit separate gap.

## Task 2: atomic source settings, migration and privacy (AC-288)

**Files:** `daemon/src/audio/source.rs`, settings/preview hunks `daemon/src/audio.rs`, dispatch `daemon/src/server.rs`, `protocol/protocol.json` plus generated `phone/protocol/protocol.generated.ts`, class entry `daemon/src/overseer/control.rs`; tests `audio_packs.rs`, `phone_methods.rs`, protocol/classification tests.

**API:** retain `audio.get` as read and `audio.set` for enablement; add Mac-only `audio.source.set {source:'builtin'|'folder',path?:string,expected_revision:number}`. Folder path is required only for folder, forbidden for builtin. Unknown/authority fields refuse. `audio.preview {key}` still explicit while off. Public `audio.get` returns enabled, revision, available, source `{kind,label,available,reason}`, and twelve line `{key,phrase}` records; never folder path or private provenance/hash metadata.

- [ ] Baseline tests cover source selection while off, revision conflict, invalid selection rollback, folder removal and restart, private error/projection privacy, old stored track migration. Real store meta is valid legacy fixture setup; no fabricated runs.
- [ ] Validate outside Store lock; commit source/path and validation identity in one transaction after rechecking expected revision. Folder selection never enables. Enabling an unavailable source refuses; disabling always succeeds and cancels playback. Concurrent source/toggle operations use latest revision without overwriting unrelated settings.
- [ ] Persist selected source even when it later becomes unavailable; bounded revalidation on get/preview/play reports a safe reason, never falls back. Old reactor/system settings migrate to Built-in; old commander path remains a private folder requiring complete new manifest. Preserve the prior enabled bit without turning an off setting on. Invalid migrated source stays unavailable.
- [ ] Old track/voice mutations refuse with static migration guidance; retain `audio.voices` only if needed for independent Voice Mode compatibility. `audio.import_commander` may be a deprecated full-manifest validation alias, with no copy and no legacy three-file acceptance; UI no longer calls it.
- [ ] Register new method Mac-only in generated classification and NEVER in model/Overseer action routing. Gateway refusal covers observe/control/admin devices; audio.get remote projection contains no path. Regenerate with `node protocol/gen-ts.mjs` and check source/classification tests in allocation.
- [ ] Emit only safe `audio_changed` public settings and refresh both local clients. No private settings in event replay. Review/commit/push.

## Task 3: bounded opened-file playback and semantic integration

**Files:** `daemon/src/audio/player.rs`, coordinated play path `audio.rs`; tests `audio_packs.rs` plus existing audio/Voice fixtures jointly reconciled with classifier branch.

- [ ] RED controls: held player+switch source, queued preview/current source, disable during player, player error/hang, source removal/replacement before play. Sink observes validated selected bytes/identity through synthetic-only control; never claim actual afplay proof from sink output.
- [ ] Feed opened regular descriptor to the player; do not close it then pass its original path. Bound launch/wait (clip<=15s plus fixed grace), cancellation and reap; avoid holding Store/process locks across I/O. One daemon player remains shared with Voice arbiter, no per-client players.
- [ ] Semantic worker rechecks eligibility/enablement after arbiter wait; source resolves at playback start. Keep clip/source change and disable behavior distinct. Drop stale automatic cues, explicit preview stays explicit, no retry loop.
- [ ] Run descriptor handoff/player tests and joint old audio/Voice regressions only in root slot. Native opened-descriptor afplay behavior needs actual isolated qualification; platform fallback must not reopen unsafe paths. Commit/push after source review.

## Task 4: existing VS Code and TUI source controls

**Files:** extract `extension/src/audio-mode.js` from command in `extension/src/extension.js`; `test/unit/audio-mode.js`; existing `tui/src/app.rs`, `tui/src/ui.rs`, `tui/tests/audio.rs`; packaged scenario `test/scenarios/audio.js` (verify actual filename before edits).

- [ ] Node RED: menu has exactly Built-in/From folder plus separate on/off and twelve previews; picker cancellation sends no mutation; invalid/stale/disconnected result preserves state; hostile pack labels are rendered as text; restricted-workspace mutation refused.
- [ ] TUI RED: retain `S` Audio entry, Compose/incumbent controls; `1` Built-in, `2` From folder path form, Esc cancels, Tab cycles twelve phrases and `p` previews; expected revision pinned to shown state; disconnected/unknown settings refuse mutation, safe label/path rendering, no automatic voice/import/track toggles.
- [ ] Implement native QuickPick/openDialog and existing themed TUI panel/forms, pack label/availability, explicit refresh via safe event. No new UI shell and no revealing raw keys in place of human phrases. TUI path form is bounded/local; no remote folder picker.
- [ ] Run Node/TUI focused tests and existing source checks in allocated slots; prepared packaged scenario tests all three themes, narrow controls, cancellation/invalid folder, separate enablement, twelve previews, source-loss and two-client daemon-owned playback. Actual rendered/terminal/native audition/full suite stay unrun until executed. Commit/push draft checkpoint.

## Initial baseline checkpoint

Author `daemon/tests/audio_packs.rs` with eight API-boundary tests and valid synthetic PCM WAV helper only. Do not import private assets or add runtime modules just to compile a wished-for API. Freeze/push authored source before baseline allocation; count helpers separately. Correct setup failures only after diagnosis; no passing claim until actual results. Then implement one reviewed Task1/2 green slice at a time; native asset/player/surface qualification remains visible rather than claiming all AC287/288 complete.
