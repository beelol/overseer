# AC-40 fresh-reader README verification

- Date: 2026-09-25 00:53 PDT
- Reader: fresh agent with no prior context; worked only from a new clone and its README.md
- Clone: `git clone --branch claude/overseer-macos-app-2d3d20 https://github.com/beelol/overseer.git /tmp/ovs-fresh-reader`
- Clone HEAD: `adb2dc99a9254437f4d0212a77d89817f56154ce`
- Machine: macOS 26.6.2 arm64; rustc/cargo 1.89.0, node v24.20.0, git 2.50.1, VS Code `code` 1.139.0
- No submodules (Branch Diff is vendored under `extension/branch-diff`)

## Steps ("Build and install (macOS)")

| # | Command (run from clone root) | Result |
|---|---|---|
| 1 | `npm ci --prefix extension/branch-diff/tooling/review --ignore-scripts` | PASS: 6 packages, 0 vulnerabilities |
| 2 | `npm ci --prefix extension/tooling/vsce --ignore-scripts` | PASS: 0 vulnerabilities |
| 3 | `node extension/scripts/package.js` | PASS (exit 0): `Packaged: /private/tmp/ovs-fresh-reader/extension/overseer-0.1.0.vsix (107 files, 3.5 MB)`; `extension/bin/overseerd-darwin-arm64` created; 7 dead-code warnings from rustc, no errors |
| 4 | `code --user-data-dir /tmp/ovs-fresh-reader-profile --extensions-dir /tmp/ovs-fresh-reader-ext --install-extension extension/overseer-0.1.0.vsix` (isolated profile instead of README's bare `code --install-extension`) | PASS: `Extension 'overseer-0.1.0.vsix' was successfully installed.` (a Node `url.parse()` DEP0169 warning from VS Code itself) |
| 5 | `code --user-data-dir /tmp/ovs-fresh-reader-profile --extensions-dir /tmp/ovs-fresh-reader-ext --list-extensions --show-versions` | PASS: `beelol.overseer@0.1.0` |
| 6 | `cargo test` | PASS (exit 0): unit tests `4 passed; 0 failed`; `tests/protocol.rs` `25 passed; 0 failed` (29 total) |

Not run (out of scope for this check): reload VS Code / activity-bar icon, `node test/ui/scenario-main.js`
(opens a VS Code window), and any agent/harness task or login.

## Daemon smoke check ("Recovery" section), isolated state

```
OVERSEER_HOME=$(mktemp -d /tmp/ovs-fresh-reader-home.XXXX)   # isolated, not ~/Library/Application Support/Overseer
target/release/overseerd version          -> overseerd 0.1.0 (protocol 1)
target/release/overseerd serve &          -> log: "listening on $OVERSEER_HOME/run/overseerd.sock", "reconcile: []"
target/release/overseerd ctl hello        -> {"result":{"protocol":1,"version":"0.1.0","socket":"$OVERSEER_HOME/run/overseerd.sock",...}}
target/release/overseerd ctl state        -> {"result":{"cursor":1,"daemon":{"parser_version":"2026-09-24.1","version":"0.1.0",...},
                                              "profiles":[system-claude, system-codex, system-opencode (existing login)],
                                              "runs":[],"tasks":[],"turns":{},"workspaces":[]}}
target/release/overseerd ctl daemon.shutdown -> {"result":{"ok":true}}; log "shutdown requested"; no `overseerd serve` process remains;
                                              a later `ctl state` fails with "Connection refused" (expected)
```

Result: PASS. State dir contained `overseer.sqlite`, `overseerd.log`, `overseerd.lock`, `run/`, `runs/`.

## README problems found

1. Status line inconsistency: "Verified 34 / 41" implies 7 unverified, but the "Unverified:" list names only 6
   (AC-11, 12, 13, 14, 19, 41). AC-08 is missing from that list although it is an open item in Follow-ups.
2. Recovery section says `overseerd ctl state` but never says where `overseerd` is: it is not on PATH after
   install. A reader must find `target/release/overseerd` (or `extension/bin/overseerd-darwin-arm64`, or the copy
   inside the installed extension) themselves.
3. The daemon is only started "on demand" by the extension; the README does not mention `overseerd serve`
   (only discoverable from the binary's usage line) for running `ctl` without VS Code. Minor for normal use.
4. Build step 1 `git clone ... && cd overseer` clones the default branch; fine for `main`, but nothing says which
   branch carries this milestone. Minor.
5. Unclear but harmless: `package.js` prints 7 rustc dead-code warnings; the README does not say warnings are expected.

No step was wrong or failed; all links in the README resolve to existing files.

## Verdict

Yes. A fresh reader can build, package, install (isolated profile) and run the checks from the README alone
on macOS arm64. Daemon control needed one inference (binary location / `serve`), noted above.
