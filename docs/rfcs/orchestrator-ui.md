# Side RFC: daily-driver orchestrator UI

Status: proposed by the owner on 2026-09-26. Acceptance criteria: AC-54 to AC-64 (Gate J in the
[main RFC](../overseer-rfc.md)).

## Why

The functional core is verified (AC-01 to AC-52). What is missing is the reason to open Overseer
instead of Claude Code or Codex directly. Today the views read like a debug console:
- full paths, run IDs and raw JSON are printed inline;
- the conversation is a log;
- buttons sit in one flat row;
- VS Code's default chrome makes the whole thing feel heavy.

The goal of Gate J is an orchestrator that is comfortable to live in all day.

## Layout

The Overseer dashboard (AC-57) has three columns and one alternative mode.

| Area | Content |
| --- | --- |
| Left rail | Agents grouped by repository, a **Needs you** section on top (AC-61), search and archive (AC-63). Narrow; names, status dots and badges only. |
| Middle | The selected agent's chat (AC-55). With no agent selected: the new-agent composer (AC-59). |
| Right | The selected agent's review and files (AC-42, AC-51). Collapsible. |
| Grid mode | Replaces middle and right with tiled agents (AC-58). The left rail stays. |

- The dashboard is a window configuration, not a separate app. One command enters it and one exits it.
- Exiting restores the previous layout exactly.
- The dashboard can open in its own window with no folder.

## Visual rules (AC-54)

**Long values are never printed raw.**
- Paths: `~` for the home folder and a middle ellipsis (`~/…/worktrees/demo/fix-login`).
- Branch names: the task name when it matches.
- IDs (run, session, tool use): hidden behind a details disclosure.
- Prompts: first line only.
- JSON: summarised ("Edit `hello.md`: replace 1 line").
- The full value is always in a tooltip and one click from **Copy**.

**Actions have a hierarchy.**
- One primary action per surface (for example Send, Allow).
- Two or three secondary actions as quiet buttons.
- Everything else in a `…` overflow menu.
- A disabled action says why on hover.

**One scale.**
- Four spacing steps: 4, 8, 12 and 16 px.
- Three text sizes: body, small and title.
- One radius: 6 px for controls and 10 px for cards.
- Codicons only.

## Chat (AC-55)

- **Your messages:** right-aligned bubbles.
- **Agent replies:** Markdown rendered with syntax-highlighted code blocks (with copy), streaming.
- **Tool calls:** one-line chips (`Edited hello.md +1 −0`, `Ran npm test ✓ 3 s`) that expand to their inputs and results.
- **Permission requests:** cards with a readable summary and **Allow** / **Deny**.
- **Native children:** nested, collapsible threads under the tool call that started them.
- **Composer:** pinned to the bottom and grows with its content. Enter sends, Shift+Enter adds a new line, and Stop replaces Send while the agent runs.
- **Scrolling:** follows new output unless you scrolled up. **Jump to latest** brings you back.

## Grid (AC-58)

- Tiles like a terminal multiplexer: 1, 2, 2×2, 3×2 or 3×3, chosen from the number of agents shown.
- A setting caps the count (default 6, at most 9).
- Each tile shows name, account, status, the last few conversation items, a one-line composer and inline Allow/Deny.
- A tile can be pinned, so a finished agent stays on the grid.
- Enter or a click opens the tile in the normal chat and review. Arrow keys move between tiles.

## Themes (AC-56)

**Overseer Dark** and **Overseer Light** are optional color themes contributed by the extension:
- purple accents over silver and graphite "metal" neutrals;
- quiet borders and flat tabs;
- fewer separators, so VS Code feels lighter.

Overseer's own views keep using VS Code theme tokens, so they also look right in any other theme,
including high contrast. Starting palette, tuned for WCAG AA text contrast:

| Token | Dark | Light |
| --- | --- | --- |
| Background | `#15141B` | `#F6F5FA` |
| Surface (side bar, cards) | `#1C1B24` | `#FFFFFF` |
| Raised surface | `#24222E` | `#EFEEF5` |
| Border | `#2E2C3A` | `#DDDBE7` |
| Text | `#E7E5EF` | `#1C1A26` |
| Muted text | `#A09CB2` | `#5F5A72` |
| Silver (icons, rules) | `#C8C6D4` | `#8D89A0` |
| Accent (purple) | `#9B7BFF` | `#6B47E0` |
| Accent hover | `#B39CFF` | `#5A37CC` |
| Added / removed (diff) | `#2F9E6E` / `#D0566B` at low alpha | same hues, darker |

## Daily-driver gaps (AC-60 to AC-64)

What still sends people back to the native CLIs:
- **Parity (AC-60):** model, effort and permission mode; attachments and pasted images; @-mentions; steering a running agent; resuming a session.
- **Attention (AC-61):** one "Needs you" list and full keyboard control.
- **Usage and limits (AC-62):** so you pick the right account.
- **A tidy history (AC-63):** archive and search.
- **The real test (AC-64):** an hour of real work without leaving Overseer.

## Limits

- VS Code does not let an extension remove the title bar or activity bar outright. The dashboard hides what the workbench commands allow and leaves the rest.
- Themes cannot restyle native widgets beyond the color tokens VS Code exposes.
