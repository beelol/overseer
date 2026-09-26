# Swarm verification ledger

[The RFC](../../rfcs/swarm-mode.md) is authoritative for SWARM-01–64.
`coverage.json` starts every item unverified and records the RFC hashes and base revision.
Create `SWARM-XX.md` when work begins on that criterion; each record must include revision,
fixture or live support level, inputs, exact commands, expected/actual result, evidence, and
any blocker. `partial`, `blocked`, and `implemented / unverified` never check the RFC box.

Baseline at `19edf57`: `cargo test --workspace --offline` passed 4 unit and 25 protocol tests
when run with permission for the daemon to open its Unix socket. The restricted sandbox
prevented daemon startup and produced 25 false baseline failures. The current main branch's
`extension/package.json` points `npm test` at `extension/test/run.js`, which is absent.
That pre-existing test-script issue is not a swarm verification pass.

[Implementation plan](../../superpowers/plans/2026-09-26-swarm-mode.md) is maintained in
this isolated worktree. `codex/automode-rfc` exists as a separate branch; its routing RFC
was inspected for the target snapshot and admission boundary. No automode code was copied.
