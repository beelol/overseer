# Goal: get everything done

Paste the block below into `/goal`.

```
Get everything in Overseer (github.com/beelol/overseer, local clone /Users/bilal/projects/overseer) done, and merge every in-progress pull request once its agent has finished. Follow AGENTS.md at the repo root.

WHERE THINGS ARE
- Criteria: docs/overseer-rfc.md (each AC-NN has a Verify clause). Ledger: docs/verification/records.py (regenerate: set -o pipefail; python3 docs/verification/records.py b5693b8).
- Designs: docs/rfcs/orchestrator-ui.md (Gates J, K, M), offline-mode.md (Gate L), phone-remote.md (Gate N), audio-mode.md (Gate O), tui.md; Swarm and Auto designs are on their branches: docs/rfcs/swarm-mode.md (codex/swarm-mode), docs/rfcs/auto-mode.md (codex/automode-rfc). Brand: docs/design/brand.md.
- Tests: cargo test --workspace; node test/unit/*.js; node extension/scripts/package.js then node test/ui/scenario-<name>.js.
- Design review page for the owner's marks: https://claude.ai/artifact/7ohJ5qNE7Wdqt95n1ecavv (marks: ArtifactData, collection "marks").
- The other agents and their work: Codex Swarm = PR #3, branch codex/swarm-mode, worktree /private/tmp/overseer-swarm-mode. Codex Auto = PR #2, branch codex/automode-rfc, worktree ~/.codex/worktrees/automode-rfc/overseer (100+ commits not pushed). Claude Audio Mode = PRs #5 and #6 (session "Overseer Audio Mode PRs merge-ready"). Claude Continuity = Gate L, branch claude/continuity-gate-l. Claude phone app = Gate N (session "Phone remote control for VS Code agents"). Find others with gh pr list and git worktree list.

EVERY PASS
1. Check on the other agents: each one's recent commits are on its own criteria, its tests pass, it pushes regularly, it isn't looping. Comment on its PR with a clear ask if not; tell the owner if a decision is needed (AC-157).
2. Merge any PR whose agent is done (marked ready, or quiet 3+ hours and says it's done): merge main into a throwaway copy, fix conflicts so both features still work, run all the tests, then merge with a merge commit. Never touch an agent that is still working. Never force-push (AC-146).
3. Then do the next open criterion, pushing after each, in this order:
   - PR #8 (Gate K follow-ups) once the owner OKs the review page; AC-154 (composer pills fill the row); AC-155 (one-line search, filters in a dropdown)
   - Gate Q: AC-156 shared rules, AC-160 owner-actions list
   - Gate P: AC-147 one test command, AC-148 GitHub checks, AC-149 steady UI suite, AC-150 first click lands, AC-151 live reruns, AC-152 performance, AC-153 ledger link check
   - Gate M (AC-158): the "Overseer" theme (AC-103) and immersive look (AC-102) first, then AC-99 to AC-108
   - AC-142 logo once the owner puts the files in docs/design/brand/
   - The Claude parts of AC-81 and AC-45 once the owner signs in to Claude Code

HOW: UI changes tested in the packaged VSIX with screenshots; design work goes on a review page for the owner's marks. Only ask the owner for what only they can do; keep working on everything else meanwhile.

DONE WHEN every criterion is verified or only waiting on the owner, every PR is merged or closed with a reason (AC-161), all tests pass on main, and the ledger is current.
```
