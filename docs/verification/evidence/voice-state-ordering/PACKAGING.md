# Qualification preparation: official new package

Root source-reviewed `42e4406` and executed the unchanged eleven host fixtures:
**11/11 PASS**, preserved exactly in `initial/host-green.log`. Prior baseline
remains **2 PASS / 9 RED**. This qualifies the focused host ordering correction;
extension-wide checks and a newly built packaged Voice scenario remain pending.
The intermittent original packaged failures remain unexplained.

## Provisioned without runtime

- Preserved the original copied VSIX under ignored
  `extension/artifacts/fe643/overseer-0.1.0.vsix`; SHA-256 remains
  `cc250107fe2b875b8d9aced1d8452f9c50e5906864a7784f63d420262e98ffbd`.
  The root extension directory currently has no VSIX. A future official package
  therefore cannot overwrite or accidentally select the historical artifact.
- Copied only ignored tooling dependencies from the frozen fe643 checkout:
  `extension/tooling/vsce/node_modules` and
  `extension/branch-diff/tooling/review/node_modules`. Their package and lockfiles
  match both checkouts exactly; hashes are recorded below.
  Locked vsce is 3.9.2. No install, test, source check, build or UI was executed
  while making these copies.

| Locked file (same in both checkouts) | SHA-256 |
| --- | --- |
| `extension/tooling/vsce/package.json` | `ae67d83aca1c0a1e3a412ed954ee840be27a17b04fe1c4590c5b108471beaea1` |
| `extension/tooling/vsce/package-lock.json` | `adae283b64343ead7e297fc035d043e9e8fe6e3777ab98f8f5234eb8381347a1` |
| `extension/branch-diff/tooling/review/package.json` | `baac6c3280fc8718927e31db30a69c67ea447670e42609e62bb3a1de4eb1bdae` |
| `extension/branch-diff/tooling/review/package-lock.json` | `1fe2ae63770ee56e6e4b0dfb3c0913967ffde6f6f31bc87df4e282c55fc01a23` |

Exact preparation commands already executed:

```sh
mkdir -p extension/artifacts/fe643
mv extension/overseer-0.1.0.vsix extension/artifacts/fe643/overseer-0.1.0.vsix
cp -R /private/tmp/overseer-closeout-observed-20261003/extension/tooling/vsce/node_modules extension/tooling/vsce/node_modules
cp -R /private/tmp/overseer-closeout-observed-20261003/extension/branch-diff/tooling/review/node_modules extension/branch-diff/tooling/review/node_modules
```

## Pending allocated execution

Use official `extension/scripts/package.js`; it has no skip/reuse switch and
always builds review assets, native helpers, Rust listener and release daemon.
No original-package overlay is used. Both Rust invocations inherit
`CARGO_BUILD_JOBS` and the absolute `CARGO_TARGET_DIR` through the process
environment. Swift helpers also inherit the outer nice priority. The package
stamps actual HEAD, plus `-dirty` if generated tracked UI receipts are present;
report the exact stamp instead of claiming a clean source tree.

Run all steps only after root explicitly allocates the runtime/compiler/UI:

```sh
cd /private/tmp/overseer-voice-state-ordering-20261003
export PATH=/Users/bilal/.local/share/mise/installs/node/24.20.0/bin:$PATH
export CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
export CARGO_TARGET_DIR=/private/tmp/overseer-queue-pause-target
nice -n 20 node test/unit/run.js
nice -n 20 npm run --silent check --prefix extension
nice -n 20 node --check extension/src/voice.js
# Retired target belongs to this agent; invalidate only the switched packages.
nice -n 20 cargo clean -p overseerd --release
nice -n 20 cargo clean -p overseer-listener --release
nice -n 20 node extension/scripts/package.js
nice -n 20 node test/ui/scenario-voice.js
```

Root must confirm the retired target is idle before allocation; never use its
active integration target. If it is unavailable, select a new exclusively owned
absolute target and record that setup change. No lock bypass. One UI scenario
only, after any full-run lock is released.

Record command source HEAD, real Cargo/rustc paths, actual nice priority,
package build output, `.d` paths referencing this checkout for daemon/listener,
release binary SHA-256 values and their matching packaged archive entries. The
new archive's `extension/src/voice.js` must equal the qualified source hash
`f59ff32d38393fdeb614c1d426f45ab8f4c857362bc5f67b9d5179aee16fc284`.
The official script emits `extension/overseer-0.1.0.vsix`; hash it separately and
verify `latestVsix()` selects it. Preserve archive provenance as a new build,
not fe643 binary reuse. Capture all extension unit/source outcomes honestly,
then actual packaged Voice checks, failure diagnostics if reached, and exact
owned process cleanup. No full-suite, ready-to-merge or intermittent-cause claim
follows from this focused qualification alone.
