# Goal: get everything done (Gates P and Q, then the rest)

Paste the block below into `/goal`. It loops: each pass oversees the other agents, merges what they have finished, then advances the next open criterion, until everything is verified or waits only on the owner.

```
Work through Overseer's open criteria in beelol/overseer until every one is verified, or partial with a gap only the owner can close, and every in-progress pull request is merged after its agent finishes. Follow AGENTS.md (budget, ledger, push after each criterion, never force-push). If /usr/bin/git fails on the Xcode license, export DEVELOPER_DIR=/Library/Developer/CommandLineTools.

EACH PASS, first:
1. Oversee the agents (AC-157): Codex Swarm (#3, codex/swarm-mode), Codex Auto (#2, codex/automode-rfc; it holds 100+ unpushed commits), Claude Audio Mode (#5, #6), Claude Continuity (Gate L), Claude phone app (Gate N), and any new one. Check each one's recent commits: on its criteria, builds, tests pass, pushing, not looping, not changing others' areas. Comment on its pull request with concrete asks; tell the owner what needs a decision.
2. Merge finished work (AC-146): an agent is finished when its pull request is marked ready, or it has been quiet 3+ hours and says it is done. Merge main into a throwaway copy, resolve conflicts so both features keep working, run the full suite plus the scenarios it touches, then merge with a merge commit at the tested head (stacked PRs after their base) and record its criteria. Never touch an agent that is still working.

THEN advance the next criterion, in this order, pushing after each:
- Gate K follow-ups: PR #8 once the owner's round-3 marks are right; AC-154 composer choices fill the row; AC-155 one-line search with a filter menu.
- Gate Q: AC-156 shared rules (AGENTS.md is on main; confirm agents pick it up), AC-159 toolchain without Xcode's license, AC-160 owner actions list.
- Gate P: AC-147 scripts/test-all, AC-148 checks on every pull request, AC-149 steady UI suite, AC-150 first click always lands, AC-151 live reruns (budget), AC-152 performance, AC-153 ledger link check.
- Gate M (AC-158): the Overseer theme (AC-103) and the immersive editor area (AC-102), then AC-99 to AC-101 and AC-104 to AC-108, each with its own review page for the owner's marks.
- AC-142 logo, as soon as the owner's files are in docs/design/brand/.
- AC-81's Claude half and AC-45 once the owner signs in to Claude Code.

HOW: packaged-VSIX UI scenarios in an isolated profile for every UI criterion, screenshots in the themes each Verify clause names, a review page (artifact) for design work. Ask the owner only for what only they can do (AC-160 list) and keep working on the rest meanwhile. Leave no test windows, daemons, shims or runs going.

DONE WHEN every criterion is verified or waits only on the owner, every open pull request is merged or closed with its reason (AC-161), main is green on scripts/test-all, and the ledger is current.
```
