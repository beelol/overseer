# Installed harness inventory, 2026-10-03

Read-only local version/help/schema inspection. Claude2.1.288 and Codex0.158.0 were inspected; no model turn, browser profile, credential access or native approval flow ran. These artifacts prove available CLI/schema shapes, not capability operation.

- [Claude version](claude-version.log) and [help](claude-help.log).
- [Codex version](codex-version.log), [app-server help](codex-app-help.log) and [schema summary](codex-schema-summary.json).
- [Inventory and implementation plan](../../../../superpowers/plans/2026-10-03-harness-pending-requests.md).

The installed command `codex app-server generate-json-schema --experimental --out /private/tmp/ac274-codex-0158-schema` generated440 JSON files. Sorted relative paths plus file bytes have SHA256 `ba6ceb14f4aaeb7e0713531cb1ed0b8829bdb735fb0d5580f0d4c6dc769e2a86`; the summary lists all11 server request methods. Full schemas remain scratch inspection material and must be regenerated/requalified if the installed version changes. The inspected Claude binary SHA256 was `bbe93063f7a0879a1021b2891e5c9354e5b3b98433e32efe6750f7710afed750`; no binary or embedded source is distributed here.

Inspected adapter source: PR52 commit2bb6b1e6130249db45e387906b6bd275e1405f61. Browser extension/OS/provider authorization, mock/native transport captures, all surface lifecycles, and browser isolation remain unverified. AC274 stays unchecked.
