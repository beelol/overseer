# Design references (Gate J research, 2026-09-26)

Read-only web research for the [orchestrator UI RFC](../rfcs/orchestrator-ui.md). Ideas only;
no assets were taken. Numbers not confirmed by an official source are marked "approx.".

## Linear

- **Themes from three inputs:** base, accent and contrast, computed in LCH. This replaced about 98 hand-picked variables per theme, and the contrast input yields high-contrast themes for free.
- **Selection:** a selected surface regenerates its colors from its own background, so text and borders stay legible on it.
- **Quiet chrome:** the 2025 refresh dimmed the side bar, used fewer and smaller icons, and softened borders.
- **Type does the hierarchy:** weight and color rather than size jumps.
- **⌘K command menu:** context-first, with shortcuts shown.
- **Avoid:** colored icon backgrounds and decorative badges.

## Raycast

- **Row anatomy:** icon, title, muted subtitle, then accessories on the right (tag, text, relative date, icon), each with a tooltip.
- **Action panel:** the primary action is Enter, the second is ⌘Enter, the rest are in ⌘K with sections.
- **Empty view:** icon, title, one line and an optional action.
- **Avoid:** repeating a fact in the subtitle, the accessories and the detail pane.

## Claude.ai and ChatGPT

- **Messages:** the user in a right-aligned bubble; the assistant as full-width plain text with no avatar column, and the composer under the same column.
- **Column width:** ChatGPT approx. 40–48 rem; Claude.ai approx. 720–768 px.
- **Claude's published guidelines for embedded apps:** text at 12/14/16/20 px, radii 4–12 px, 0.5 px hairlines, very soft shadows, two weights, and skeletons instead of spinners.
- **Bubble and composer (approx.):** bubble padding about 10×16 px with a 16 px radius; the composer is a rounded card with a hairline border and no shadow.
- **Code blocks:** 13–14 px monospace; ChatGPT's 12.5 px draws complaints.

## Cursor and Zed

- **Zed tool calls:** collapsed by default. Code blocks can collapse to one line showing language and line count, with a hover toolbar for copy, wrap and collapse.
- **Zed changes:** a bar above the composer ("files changed, +/−") opens one review with Keep/Reject per hunk.
- **Zed threads:** grouped by project; each row shows title, status and agent; ⌃Tab switches threads.
- **Cursor:** file mentions appear as pills in the prompt, and Agent Tabs tile several chats side by side or in a grid.
- **Avoid:** long code dumps pushing the prose apart.

## Warp

- Everything is a block (commands, output, agent steps), and each can be selected or collapsed as a unit.
- Agent commands collapse into a summary row.
- Agent mode has its own tint so it can't be confused with user commands.

## Vercel Geist

- **Color scale:** 10 steps: backgrounds (100–300), borders (400–600), fills (700–800), then secondary and primary text (900–1000).
- **Radii:** 6 px for controls, 12 px for menus and cards, 16 px for full screens.
- **Type and spacing:** weights 400/500/600 only, on a 4 px base unit.

## Multi-agent orchestrators

- **Conductor:** one git worktree per task, a workspace side bar, a diff viewer, and checks toward merge readiness.
- **Codex app:** projects with threads under them. Each status has a small icon and color: done = green check, needs input = question, needs approval = shield, failed = red. Threads waiting on the user keep full-strength titles.
- **Claude Code desktop:**
  - the side bar filters by status and project;
  - environment, folder, model and permission mode sit in the prompt box;
  - a `+12 −1` chip opens the diff;
  - transcripts cycle between Normal, Thinking and Verbose;
  - panes can be dragged into splits.

## Sources

- https://linear.app/now/how-we-redesigned-the-linear-ui
- https://linear.app/now/behind-the-latest-design-refresh
- https://developers.raycast.com/api-reference/user-interface/list
- https://developers.raycast.com/api-reference/user-interface/action-panel
- https://claude.com/docs/connectors/building/mcp-apps/design-guidelines
- https://zed.dev/docs/ai/agent-panel
- https://zed.dev/docs/ai/parallel-agents
- https://cursor.com/changelog/2-0 and https://cursor.com/changelog/3-0
- https://docs.warp.dev/agents/local-agents/interacting-with-agents/terminal-and-agent-modes/
- https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment
- https://vercel.com/geist/colors, https://vercel.com/geist/typography, https://vercel.com/geist/materials
- https://www.conductor.build/docs/guides/parallel-agents/run-multiple-claude-code-sessions
- https://code.claude.com/docs/en/desktop
