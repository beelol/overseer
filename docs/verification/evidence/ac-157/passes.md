# AC-157: oversight passes

## Pass of 2026-09-27, morning (Pacific)

| Agent | Branch, pull request | Last commit | Pushed | Behind main | AGENTS.md | Finding and ask |
|---|---|---|---|---|---|---|
| Codex Swarm | `codex/swarm-mode`, #3 | `8b4e589`, 07:04 | yes | 2 | yes | Working on its RFC's criteria, merges main. Asked to merge main after the Gate M merge ([comment](https://github.com/beelol/overseer/pull/3#issuecomment-5853378574)); it did. |
| Codex Auto | `codex/automode-rfc`, #2 | `c769383`, 2026-09-25 | 118 local commits unpushed (earlier pass) | 212 | no | Quiet for over a day; the owner moved it to its own Claude task. Asked to push and merge main ([comment](https://github.com/beelol/overseer/pull/2#issuecomment-5853450069)). |
| Claude Continuity | `claude/continuity-gate-l`, #9 | `707273e`, 07:03 | yes | 2 | yes | Merged main after the ask ([comment](https://github.com/beelol/overseer/pull/9#issuecomment-5853378314)); still early in Gate L (step 0 of 5 at the last read): watched; the everything goal takes Gate L over if it stalls. |
| Claude phone app | `claude/phone-remote-vscode-control-b48a34`, #10 | `bb6ffad`, 2026-09-26 23:53 | no: over 7 hours, about 368 uncommitted changes | 134 | yes | Flagged: commit and push, merge main, report the milestone ([comment](https://github.com/beelol/overseer/pull/10)); told about AC-178. |
| Claude Overseer itself (Gate S) | `claude/orchestrator-agent-control-rfc-8e2009` | `bb1083a`, 06:53 | yes | 7 | yes | Docs only; its design overlaps AC-107 (Talk to Overseer, on main): the two need to agree. |
| Claude Voice Mode (Gate R) | local worktree branch | — | design and criteria on main | — | — | Design stage; waits for the owner's answers in its RFC. |
| Claude Audio Mode (Gate O) | #5, #6 | merged | — | — | — | Finished: merged by its agent with the owner (`e0db692`, `ea6a6c2`); AC-143 to AC-145, T-23 and T-24 recorded. |

Machine load: the phone and Continuity agents ran VS Code scenarios at the same time as this goal's suite; timing checks that failed under that load were rerun alone before being called regressions (now a rule in AGENTS.md).
