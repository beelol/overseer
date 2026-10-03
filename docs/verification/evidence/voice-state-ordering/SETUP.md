# Original-package diagnostic setup

Prepared diagnostic checkout:
`/private/tmp/overseer-voice-state-ordering-20261003`, source `0ad0f86` on
`codex/voice-state-ordering`, base `fe643229957cb3f443613aee453a009c28516fe7`.
No tests, builds or runtime were executed during this preparation.

The original ignored package was copied, without rebuilding, from
`/private/tmp/overseer-closeout-observed-20261003/extension/overseer-0.1.0.vsix`
to the diagnostic checkout's `extension/overseer-0.1.0.vsix`. It is the sole VSIX
there, so the unchanged `latestVsix()` selects exactly this artifact.

SHA-256 receipts:

| Item | SHA-256 |
| --- | --- |
| Original and copied VSIX | `cc250107fe2b875b8d9aced1d8452f9c50e5906864a7784f63d420262e98ffbd` |
| VSIX `extension/bin/overseerd-darwin-arm64` | `7e36da1036bc75e86929493140c7ca83acc1b430a4e60d60733697252f58a251` |
| VSIX listener `extension/bin/Overseer Listener.app/Contents/MacOS/overseer-listener` | `1c829f13505b19db24b1431f358abbdbde974d8bca11b0751aca26bcb900e017` |
| VSIX and checkout `extension/src/voice.js` | `9b3be67f036325b19208375b7e0267c8998f53a072a21887fe2c6b96604b716e` |
| Both checkouts' `fixtures/fake-harness/claude-fixture.js` | `f27cf285eba5e4b084f2614075dc51a18ac0192894a9c324e83ae77e42600eb4` |
| Both checkouts' `test/ui/harness.js` | `1bed94057465943986759f3e13a1561c1989858040af300431d109e4e2cf45df` |
| Both checkouts' `test/ui/cdp.js` | `893c193eacd25439f58a6230c38693264b463d32b73fc67a57073700c147da4a` |
| Both checkouts' `test/ui/quiet-launch.js` | `523c277398e5009bf43d8b6958759c7af6eb0592e976279ebebd170e8e90baae` |
| Existing Node 24.20.0 executable | `9d050fd455b56426e25d4d603c7c501cbb2630348e836cf221dcce748e90588a` |

The packaged daemon and listener hashes also match the corresponding retained
fe643 `extension/bin` files. The fixture helper is executable and byte-identical
to fe643. The harness, CDP driver, quiet launcher and fake Claude harness require
only Node built-ins. There is no `test/ui/package.json`, lockfile or
`node_modules` dependency to provision. CDP uses global WebSocket, available in
the existing Node 24.20.0. The fixture MCP server is the packaged daemon's Rust
`mcp` subcommand, not a missing Node SDK.

VS Code CLI is the existing
`/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code`.
The scenario installs the VSIX into its own temporary extensions directory and
profile. Its `Session.ctl` resolves the daemon from that installed package;
there is no Cargo-target daemon override. The scenario uses synthetic listener
input and the checked-in fake harness, with an isolated `OVERSEER_HOME` and mDNS
off. No production daemon, owner profile, microphone or paid model is selected.

After coordinator allocates runtime, the exact direct commands are:

```sh
cd /private/tmp/overseer-voice-state-ordering-20261003
export PATH=/Users/bilal/.local/share/mise/installs/node/24.20.0/bin:$PATH
nice -n 20 /Users/bilal/.local/share/mise/installs/node/24.20.0/bin/node test/unit/voice-state-ordering.js
nice -n 20 /Users/bilal/.local/share/mise/installs/node/24.20.0/bin/node test/ui/scenario-voice.js
```

Run directly, not through `scripts/test-all`: the full runner rebuilds the VSIX,
which would destroy this same-artifact comparison. Preserve the original test
failure and diagnostic. New scenario output is under this diagnostic checkout's
`docs/verification/evidence/ui/voice`; it does not overwrite frozen fe643
evidence. The scenario's existing `finally`/exit cleanup owns its windows,
daemon, shims and listener; coordinator must check the resulting owned cleanup.
