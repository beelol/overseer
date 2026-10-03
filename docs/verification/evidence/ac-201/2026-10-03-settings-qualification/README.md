# AC201 setting-aware qualification checkpoint

Base: frozen integration `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. Authoring branch: `codex/settings-qualification`; isolated clone `/private/tmp/overseer-settings-qualification-20261003`.

Status: authored, UNRUN. No Cargo, Node tests, packaged UI, package build, full suite or paid turn has been executed for this checkpoint. `git diff --check` passed. Six changed JavaScript files passed `node --check`; these are syntax checks, not scenario/unit verification. The requested `nice -n 20` wrapper emitted `setpriority: Operation not permitted` in this sandbox; the short syntax processes still exited successfully. See source-check.log. There is no RED or GREEN claim and AC201 remains partial.

Changes are fixture/test/evidence only. AC189/190 first assert inherited disabled behavior, then explicitly owner-enable NEW positive fixture agents while keeping the original enabled cases and independent setting-override tests. The channel negative uses the actual bounded harness gate acknowledgement, not a guessed startup delay. No product switch/guard/authority was changed.

The current GateS scenario subset is home, talk, parity, home-overseer, oversight. Each now records the actual initial persisted defaults before any override plus actual per-run settings; available echo output proves native fixture MCP launch configuration. Oversight deliberately changes check-ins to off and records that transition; its later behavior is never labeled all-on. None of these receipts proves live native quality or savings.

Planned profiles are off/off and on/every:3. These cover named on/off values, not every Cartesian combination and not every packaged UI scenario. Separate exact-head clean-clone full regression remains the coordinator's gate. Each profile's artifacts must be preserved separately before the next run overwrites the scenario directory. See PLAN.md for effective controls, masked fixtures, gaps, commands and artifact boundaries.

After the compiler slot is released, first run the original frozen test binary or an unchanged frozen test copy once under inherited off to establish the known fixture expectation mismatch (not a security/product RED). Then run the two corrected AC189/190 cases before any broader matrix. All compiler/UI/full slots still belong to the coordinator. PR63 and the frozen integration source are unchanged.
