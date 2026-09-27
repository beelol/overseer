# Side RFC: daily-driver orchestrator UI

Status: proposed by the owner on 2026-09-26. Acceptance criteria: AC-54 to AC-66 (Gate J), AC-67 to AC-82 (Gate K) and AC-99 to AC-108 (Gate M) in the
[main RFC](../overseer-rfc.md)).

## Why

The functional core is verified (AC-01 to AC-52). What is missing is the reason to open Overseer
instead of Claude Code or Codex directly. Today the views read like a debug console:
- full paths, run IDs and raw JSON are printed inline;
- the conversation is a log;
- buttons sit in one flat row;
- VS Code's default chrome makes the whole thing feel heavy.

The goal of Gate J is an orchestrator that is comfortable to live in all day.

## Principles

The owner's bar: ultra clear, easy to approach, and polished. In practice:

1. **Less text.** No sentence where a word works, and no word where an icon, a status dot or the layout already says it. Every icon-only control has a tooltip and an accessible name. Explanations live in tooltips, empty states and a details disclosure, not inline.
2. **Details on demand.** IDs, paths, capabilities and raw payloads are one hover or one click away, never on screen by default. Rare actions go into an overflow or ⌘K action menu instead of a row of buttons.
3. **Quiet until it needs you.** Neutral surfaces; color only for status and attention (running, waiting for you, failed, changed). The accent purple marks the one thing to do next.
4. **Generous padding, tight text.** Room around content, short lines and a steady vertical rhythm, rather than dense text with thin margins.
5. **One scale.** Spacing, type, radii, elevation and motion come from one token set used by every view.
6. **Keep the current icon style.** Codicons everywhere (the owner likes them), except real provider logos (AC-65).
7. **The chat is the product.** It should feel as good as the best chat apps.

## References to study (AC-66)

Study these and record what Overseer adopts from each, never their assets:

| Product | What to learn |
| --- | --- |
| Linear | Restraint, density done right, subtle borders, keyboard-first, a purple accent used sparingly |
| Raycast | Crisp list rows (icon + short label + quiet accessory), the ⌘K action panel instead of button rows |
| Claude.ai and ChatGPT | Chat column width, turn spacing, composer design, Markdown and code-block styling |
| Cursor and Zed (agent panel) | Tool calls as compact chips, inline diffs with accept/reject, Zed's tiled panes for the grid |
| Warp | Output as blocks; terminal-grid feel |
| Vercel / Geist | Typography, neutral grays, spacing scale |
| Codex app, Claude Code desktop, Conductor | Multi-agent lists, status at a glance, how runs are started |
| Apple HIG, GitHub Primer | Accessibility, contrast, focus states |

## What Overseer adopts (research, 2026-09-26)

These rules come from studying the products above: official docs, design posts and changelogs,
plus a few third-party measurements marked "approx.". The notes and sources are in
[docs/design/references.md](../design/references.md). Ideas only; no assets were taken.

1. **Chat column:** about 720 px (≈70 characters), centered, 24 px gutters (16 px when narrow). Sources: ChatGPT (40–48 rem), Claude.ai.
2. **Messages:** the user's in a right-aligned raised bubble (max 80% wide, 12 px radius, 10×14 px padding); the agent's as plain full-width text with no bubble and no avatar column, line height 1.6. Sources: Claude.ai, ChatGPT.
3. **Rhythm:** 24 px between turns, 8 px between blocks within a turn. Copy and other message actions fade in on hover (150 ms).
4. **Tool calls:** one muted row (icon + verb + target + result: `+12 −3`, `✓`, `exit 1`). Consecutive calls fold into "Ran 6 tools", collapsed by default. Sources: Zed, Warp, Claude Code desktop.
5. **Code blocks:** 13 px monospace, 8 px radius, a quiet header with the language; long blocks collapse. Copy appears on hover. Source: Zed.
6. **Composer:** a rounded card (12 px radius) with a hairline border and no shadow. Chips inside it for agent, account and model. Enter sends, Shift+Enter adds a new line, Stop replaces Send while running. Sources: Claude.ai, Zed, Claude Code desktop.
7. **Changes bar:** a quiet bar above the composer ("2 files +12 −1") that opens the review. Sources: Zed, Claude Code desktop.
8. **Agent rows:** one line, 28–32 px high: status icon, title, then muted meta on the right (provider logo, relative time). Sources: Raycast, Zed, Codex app.
9. **Status:** running = accent with gentle motion; waiting for you = amber; done = green check; failed = red. Rows that need the user sort first and are never dimmed. Sources: Codex app, Linear.
10. **Grouping:** runs grouped under a quiet repository header; finished runs archive out of sight. Source: Zed.
11. **Actions:** one primary action; the rest in a `…` / ⌘K menu with shortcuts shown. Sources: Raycast, Linear.
12. **Grid:** one tile per agent: status stripe, last lines of output, one-line reply. Sources: Cursor Agent Tabs, Zed parallel agents.
13. **Color:** Overseer Dark and Light are generated from three inputs (graphite base, purple accent, contrast) into a stepped scale. Steps 1–3 are backgrounds, 4–6 borders, 9–10 text. Sources: Linear (LCH), Vercel Geist (10-step scale).
14. **Radii:** 6 px for controls and rows, 12 px for cards, menus and the composer, full pills for chips. Borders are hairlines; shadows only on floating layers. Sources: Geist, Claude.
15. **Type:** 12/13/14/16/20 px, weights 400/500/600, one monospace size. Hierarchy comes from weight and color, not size jumps. Sources: Geist, Linear.
16. **Spacing:** a 4 px grid; 16–24 px section padding. A surface change replaces a separator line. Sources: Geist, Linear.
17. **Empty states:** icon + one line + one action ("Start an agent"). Source: Raycast.
18. **Motion:** 120–200 ms ease-out; no spinners on content. Source: Geist.
19. **Avoid:** colored icon backgrounds and decorative badges (Linear); repeating a fact in several places (Raycast); dimming rows that need the user (Codex app); code dumps that push prose apart (Zed).

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

**Text budget.** The audit counts visible text per view against today's UI and expects at least
40% less with no function lost. Typical moves: status words become dots; labels on common actions
become icons with tooltips; explanatory lines become empty states or tooltips; repeated headers disappear.

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

**Spacing:**
- one centered column of about 72 characters, with 24 px side gutters and 16 px between turns;
- 12 px padding inside bubbles and cards, and line height 1.5;
- code blocks run full column width with 12 px padding.

**Feel:**
- streaming without layout jumps;
- consecutive tool calls folded into one line ("3 edits, 2 commands");
- the composer keeps focus after sending;
- subtle motion only.

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

## Provider logos (AC-65)

Accounts, harness choices, agent rows, grid tiles and the composer show real provider logos:
Claude/Anthropic, OpenAI/Codex, OpenCode, and GitHub for pull requests.
- **Source:** only a license that allows bundling, for example Simple Icons (CC0) or LobeHub Icons (MIT), used as each brand's guidelines allow. Each logo is recorded in a third-party notices file shipped in the VSIX.
- **Theme:** monochrome variants in dark, light and high contrast, where color would clash.
- **No licensed logo:** a neutral codicon instead, with the gap recorded.

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
- **The real test (AC-64):** an hour of real work without leaving Overseer, after the owner's design review (AC-66).

## Gate K layout

Owner direction (2026-09-26): one agents list in VS Code's own side bar, and an editor area that
shows what matters for the selected agent.

| Area | Content |
| --- | --- |
| Side bar (Overseer view container) | **Needs you** first, then agents by repository with native children nested; provider logos as tree icons; search; hover actions (stop, archive, pin to grid); Accounts below. Replaces the dashboard's agent rail (AC-67 to AC-71). |
| Editor area, nothing to review | The chat, or the new-agent composer, alone in the middle (AC-72). |
| Editor area, agent has changes | The editable review on the left (about two thirds) and the chat on the right (about one third). Closing the review puts the chat back in the middle (AC-73). |
| Review | One scope picker (All changes, Staged, Unstaged, Untracked) beside the comparison base; follow or manual mode (AC-74, AC-75). The Workspace Dirty view goes away. |
| Grid | Opens in the editor area and closes back to the previous arrangement (AC-79). |

- Built with editor groups, which Overseer already manages. The secondary side bar was considered for the chat; extensions cannot reliably place views there, so it is not used.
- The side bar is a native tree: it cannot show chips or custom layouts, which a list does not need. Rich surfaces (chat, composer, grid, review) stay in the editor area.
- Dragging an agent out of the tree depends on what VS Code accepts as a drop; the fallback is **Open to the Side** and **Pin to Grid** (AC-71).

## Limits

- VS Code does not let an extension remove the title bar or activity bar outright. The dashboard hides what the workbench commands allow and leaves the rest.
- Themes cannot restyle native widgets beyond the color tokens VS Code exposes.

## Gate M: Overseer as the whole surface

Owner review of Gate K (2026-09-26), in the owner's words where it matters:

- Approved: the side bar hierarchy ("looks amazing"; no changes), the layout after a restart, the review inside the grid, and the overall direction ("really good progress").
- The review: "I should be able to go into the folders and still see the same changes, and I must be able to edit it", and clicking files should "open in the same thing", not the standard VS Code view. The reviewer should be Overseer's own.
- Duplication: the changed-files strip under the chat repeats the review; the Explorer should not be repeated inside Overseer ("just use the Overseer one that goes on the side instead of the Explorer"); the dirty checkout stays inside the review.
- The look: "I want the middle to look less like it's just VS Code… a full VS Code overhaul", and a third theme named **Overseer**: "darkish, but mostly focused on gradients… like it's a future app."
- The grid: drag agents onto the top, bottom or sides of a tile to grow it, up to a hard cap of 16 ("you just can't drag more in"); click one agent to track it with its review and follow mode; "I just want to make sure you can't easily get lost", meaning losing track of windows.
- A chat with Overseer itself at the bottom, on the default account, to nudge agents in a different direction (a separate criterion).

Design direction:

1. **Files live in the review.** The navigator becomes the worktree tree with changes marked and a *Changes only* filter; the right pane shows one file at a time in Overseer's editor (Monaco, already in the fork): a diff when the file changed, plain editable text when it did not. The chat keeps its per-turn edit chips (they jump to the hunk) and loses the Files pane and the changed-files strip.
2. **One frame.** Overseer's views share one header style (agent, what the view is, the few controls it needs) so VS Code's tab strip and breadcrumbs can be hidden while Overseer holds the editor area; the settings involved are applied in Overseer's mode only and restored exactly, as dashboard mode already does for the panel and side bars.
3. **The Overseer theme.** VS Code's workbench colors are flat, so the gradients (a slow aurora on the editor backdrop, lit edges on active tiles and the tracked agent, glass-like chat and review surfaces) are drawn by Overseer's webviews; the workbench colors of the theme are picked to sit under them. Overseer Dark and Light stay as they are.
4. **Grid by drag.** The grid becomes a split layout (like editor groups): dropping on a tile's edge splits that tile; the tree of splits is kept per window; 16 is the cap. Clicking a tile tracks it: the review for that agent opens beside the grid, following.
5. **Where am I.** One of each view per window, every header names its agent, and a small map lists what is open and jumps to it.
6. **Talk to Overseer.** A docked chat that runs an orchestrator turn on the default account with read access to the daemon's state and a small set of actions it may propose; every action waits for the owner's yes.
