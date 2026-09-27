# Swarm verification ledger

[The RFC](../../rfcs/swarm-mode.md) is authoritative for SWARM-01–64.
The [shared Auto–Swarm contract](../../rfcs/swarm-auto-contract.md) owns
CONTRACT-01–05, with one evidence record per criterion in this folder. Both RFCs
cite those records; neither may count a partial fixture as integrated proof.
`coverage.json` starts every item unverified and records the RFC hashes and base revision.
Create `SWARM-XX.md` when work begins on that criterion; each record must include revision,
fixture or live support level, inputs, exact commands, expected/actual result, evidence, and
any blocker. `partial`, `blocked`, and `implemented / unverified` never check the RFC box.

Historical baseline at `19edf57`: `cargo test --workspace --offline` passed 4 unit and 25
protocol tests when run with permission for the daemon to open its Unix socket. The
restricted sandbox prevented daemon startup and produced 25 false baseline failures.
Main has since merged AC-147's `scripts/test-all` and extension unit runner; the old
missing-test-script note no longer describes current main. Their existence does not verify
Swarm's normal launch or live harness support.

[Implementation plan](../../superpowers/plans/2026-09-26-swarm-mode.md) is maintained in
this isolated worktree. `codex/automode-rfc` exists as a separate branch; its routing RFC
was inspected for the target snapshot and admission boundary. No automode code was copied.

The [main integration review](main-integration-review.md) records how the newer agent UI,
Audio Mode, Continuity, and remote-control requirements affect Swarm without treating
another gate's evidence as a Swarm verification pass.
