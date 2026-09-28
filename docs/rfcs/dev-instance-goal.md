GOAL: Overseer develops Overseer, in stages: dev instances beside the owner's production Overseer, one deliberate deploy path, and production never touching dev. Follow AGENTS.md.

READ: docs/rfcs/dev-instance.md (write it first), Gate T in docs/overseer-rfc.md, AGENTS.md, daemon/src/paths.rs, extension/src/daemon-client.js, test/ui/harness.js, phone/e2e/lab.mjs.

STAGES, in order, each its own pull request, each merged before the next starts:
1. Production guard. An installed production daemon, extension or TUI always uses the standard instance, even with dev variables leaked into its environment. Production refuses anything marked dev. A dev build refuses the production home. The phone app's side is a criterion for after PR #10.
2. Dev tooling. scripts/dev with up, down, status, list, logs, clean, code and tui. Named isolated instances (their own home, socket, VS Code profile and extensions, gateway port and mDNS identity). A simulator pointed at a chosen instance (the criterion after PR #10). The AGENTS.md section "Running a dev Overseer". A complete `scripts/dev --help`.
3. Deploy. The only path from dev to production. It builds from main and installs the daemon, the extension (into the owner's VS Code) and the notifier. It restarts the production daemon only when no runs are active, otherwise it waits or asks. It keeps data and logins, records the deployed version and can roll back. It is run by the owner, or by an agent the owner asked.

RULES
- Never touch the owner's production daemon, data, VS Code, logins or keychain while building. Test against temporary "production" homes.
- Criteria and records go to main; take the next free AC numbers right before pushing. records.py refuses a stale copy: pull first.
- Before 21:15: no VS Code launches, UI scenarios or scripts/test-all (another agent's measurements). Rust builds and targeted tests are fine.
- Timing tests that fail under load are rerun alone first.
- No paid model turns. Leave nothing running. Never force-push.
- Mark each PR ready only when its criteria are verified with evidence. The merge monitor (the session that sent you) merges each one after its own full run.

DONE WHEN:
- the RFC, this goal and Gate T's criteria are on main;
- three PRs are marked ready with every Gate T criterion verified (the phone-app criteria excepted, left not started with "after PR #10" as their blocker);
- `scripts/test-all` passes on each branch;
- your final report lists the criteria, the PRs, the tests run, and the exact deploy command for the owner.

Message me (SendMessage to "main") each time a PR is marked ready, so I can merge it before you start the next stage on top of main.

---

Additions from the owner (2026-09-27), after the goal above was set: the feature is called **the dev daemons feature** (developer versions of Overseer that collide neither with each other nor with the installed production extension); and **guided owner tests** (AC-215) are a stage of their own between the dev daemons tooling and the deploy, so there are four pull requests: production guard, dev daemons tooling, guided tests, deploy.
