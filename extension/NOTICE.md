# Third-party notices

## Branch Diff (Local)

The review UI in `branch-diff/` is derived from Branch Diff (Local),
https://github.com/beelol/branch-diff at commit `fbc6eb807fd41d8fd1a004977e1aa637a4f7c900`,
itself a fork of Artem Kotov's Branch Diff View. MIT License; the original notice is kept
in `branch-diff/LICENSE` (Copyright (c) 2026 Artem Kotov; Copyright (c) 2026 Bilal Itani).

Overseer modifications: `review/panel.js` (Overseer-selected worktree and comparison base,
comparison/base control, Follow wiring), `review/browser.js` and `review/browser.css`
(Follow checkbox, pause/resume, agent-edit reveal, comparison label), `review/comparison.js`
and `review/editing.js` (configuration namespace `overseer.review`, target-based inputs).

## Monaco Editor and bundled dependencies

`branch-diff/dist/THIRD-PARTY-NOTICES.txt` carries the licenses of monaco-editor 0.56.0
(MIT) and its bundled dompurify (MPL-2.0 OR Apache-2.0) and marked (MIT).

## Rust daemon

`bin/overseerd-*` is built from `daemon/` in this repository. Its crate dependencies and
licenses are listed in `docs/verification/AC-03.md`.
