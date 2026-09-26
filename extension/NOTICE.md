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

## Overseer UI (Gate J) bundled assets

All license texts are in `media/vendor/licenses/`.

| Asset | Files | Source | License |
| --- | --- | --- | --- |
| Codicons font and CSS | `media/vendor/codicons/` | `@vscode/codicons` 0.0.46-24 (Microsoft) | CC-BY-4.0 (font/icons), MIT (code) |
| Markdown parser | `media/vendor/marked.umd.js` | `marked` 14.0.0 | MIT |
| HTML sanitizer | `media/vendor/purify.min.js` | `dompurify` 3.4.14 | MPL-2.0 OR Apache-2.0 |
| Syntax highlighting | `media/vendor/highlight.min.js` | `@highlightjs/cdn-assets` 11.12.0 (common languages) | BSD-3-Clause |
| Claude, Claude Code, Anthropic, OpenCode and GitHub logos | `media/logos.js`, `media/logos/{claude,claudecode,anthropic,opencode,github}-*.svg` | Simple Icons 16.32.0 | CC0-1.0 |
| OpenAI and Codex logos | `media/logos.js`, `media/logos/{openai,codex}-*.svg` | LobeHub Icons (`@lobehub/icons-static-svg` 1.95.1) | MIT |

**About the logos (AC-65):**
- Each mark identifies the product it names: the harness or provider an agent or account uses, or GitHub for pull requests. This follows those brands' guidelines for referring to their products.
- Overseer is not affiliated with or endorsed by Anthropic, OpenAI, OpenCode or GitHub, and the marks remain their owners' trademarks.
- Overseer draws the marks in one color that follows the active theme. It does not recolor, distort or combine them with other marks.
- Every provider in Overseer has a licensed logo, so no neutral stand-in was needed. Generic programs use the codicon `terminal`.
