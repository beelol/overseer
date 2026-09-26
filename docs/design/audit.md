# Presentation audit (AC-54)

What looked odd in the UI before Gate J, and what changed. "Before" is the Gate J baseline
(VSIX built from ce56432, evidence in
[audit-baseline](../verification/evidence/ui/audit-baseline/)); "after" is the new UI (evidence
in [audit](../verification/evidence/ui/audit/)). The audit scenario (`test/ui/scenario-audit.js`)
counts visible text per view and fails on horizontal overflow, text runs over 80 characters
outside code, and icon-only controls without a name.

## Visible text per view

Characters of visible text, same fixture state, 1280 px and 900 px, dark and light (the count is
the same at every size and theme). Code inside diffs is not counted.

| View | Before | After | Change |
| --- | ---: | ---: | ---: |
| Agents | 531 | 238 | −55% |
| Chat (run conversation) | 2,795 | 1,040 | −63% |
| Files | 127 | 59 | −54% |
| Review | 350 | 177 | −49% |
| New agent | 1,037 | 133 | −87% |
| Accounts | 381 | 185 | −51% |

Before, the new-agent form had 68 horizontally overflowing elements at 900 px. After: no
overflow, no long runs and no unnamed icon controls in any view, including the new dashboard and
grid.

## Odd-looking items found, and the fix

1. **Full paths everywhere.** Repository rows, the run header ("Workspace: /private/tmp/…"),
   the review header ("Worktree: …") and the new-task cards printed absolute paths. Now: the
   repository or branch name, `~` for the home folder and a middle ellipsis; the full path is in
   the tooltip.
2. **Run metadata as a sentence.** "completed — turn completed; exit 0 · claude claude-fixture
   0.0.0 (synthetic) · account … · run r-… · native fixture-session-1". Now: a status icon, the
   title, and one quiet line with the account and branch; ids and capabilities are behind the
   details button.
3. **A row of five text buttons** (Open Review, Raw output, Interrupt, Merge back…, Open PR…)
   that wrapped at 900 px. Now: icon buttons for the frequent actions (files, review) and a ⋯ menu
   for the rest; Stop sits in the composer while the agent works.
4. **Conversation and Event log as two big filled tabs.** Now: the chat is the view; the raw event
   log is one menu item away.
5. **Raw Markdown.** Tables with pipes and ` ```ts ` fences were shown as text. Now: rendered
   Markdown with tables, highlighted code and a Copy button on code blocks.
6. **Tool calls as JSON.** `Write {"content":"export class …` and
   `Edit {"file_path":"/private/tmp/…`. Now: one-line steps ("Created
   session-refresh-coordinator.ts +8 −0", "Ran npm test …") grouped under "6 steps" with a short
   summary, expandable.
7. **Every agent took two truncated rows.** A task row ("Migration dry-… ove…") and a run row
   ("generic", "claude claude (existi…"). Now: one row per agent with a status icon, the title
   and a relative time; the harness is a logo; native children nest under it.
8. **Pills competing for attention.** "1 active" on every repository and a yellow "needs you" pill
   deep in the tree. Now: a **Needs you** section at the top with a count, and a plain count on
   each repository.
9. **The New Task form as a page of cards.** An explanation paragraph, cards with full paths,
   harness cards with capability pills, and "Choose a harness." next to a disabled button. Now: a
   centered composer ("What should an agent do?") with repository, agent, model and workspace
   chips; problems appear inline with their fix; the full form stays one link away.
10. **Accounts with long labels.** "OpenAI / ChatGP…", "Anthropic / Claud…", "Devin
    unavailable:…". Now: provider logos (Simple Icons CC0, LobeHub MIT), short provider names, the
    plan and usage in the description, details in the tooltip.
11. **A follow-up textarea plus a separate button.** "Follow-up message to this run only" and
    "Send follow-up". Now: a composer pinned to the bottom with a send icon, attach, options, and
    Stop while running.
12. **Status bar text.** "Overseer 2 active, 1 waiting" with a pulse icon. Now: "Overseer 2
    active" and a bell with the Needs-you count.
13. **Truncated section headers.** "FILES Refresh …". Now: "2 changed" and a refresh icon.
14. **Mixed disclosure glyphs.** Text triangles (▾ ▸ ▼) next to codicon chevrons. Now:
    codicons in Overseer's views, and the same thin chevron in the review's file tree and file
    headers.
15. **Review header crowding.** "Files", "Latest run", the title, "2 changed files", a bare
    checkbox, "Unified" and "Refresh" in one row. Now: icon buttons for the file list and refresh,
    Save hidden until there is something to save, the checkbox labelled Follow, and the full
    worktree path in a tooltip.

## Still open (for the owner's review)

- In the review's narrow diff column, the hunk Accept/Revert buttons sit over the start of the
  code line.
- At 1280 px with the file list open, the chat header shortens the title and branch; the file
  list could close itself when the chat gets narrow.
