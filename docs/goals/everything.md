# Goal: get everything done

Paste the block below into `/goal`.

```
Meet every acceptance criterion in Overseer (beelol/overseer) and merge every pull request once its agent has finished. Follow AGENTS.md.

"Everything" is docs/verification/tracker.md: every open criterion (AC-NN in docs/overseer-rfc.md, the TUI's T-NN, the Swarm and Auto criteria in their RFCs), who owns it (this goal, another agent, or the owner) and where its work is.

A criterion gets met one of four ways:
- another agent finishes it and its pull request is merged (step 3);
- an agent got stuck on it and this goal takes it over (step 2);
- nobody owns it and this goal builds it from zero (step 4);
- only the owner can do it and it goes on the owner-actions list (step 5).

EACH PASS
1. Refresh the tracker from the ledger (docs/verification/records.py), `gh pr list` and `git worktree list`: new criteria and new agents get a row; verified rows leave.
2. Other agents' rows: read each agent's recent commits and pull request. Check it works on its own criteria, its tests pass, it pushes regularly (never 3+ hours unpushed), it merges main, and it isn't looping. If not, comment on its pull request with a clear ask (AC-157). If it is stuck (no progress on its criteria for 3+ hours after an ask, or looping), tell the owner and take its criteria over: build on its pushed work in this goal's own branch, never pushing to its branch. Never push to or merge a pull request while its agent is still working.
3. When an agent has finished (its pull request is marked ready, or it has been quiet for 3+ hours and says it is done): merge main into a throwaway copy, fix conflicts so both sides keep working, run `scripts/test-all --jobs=3` plus the UI scenarios the change touches, then squash-merge it and record its criteria in the ledger (AC-146). Stacked pull requests go after their base. Never force-push.
4. This goal's own rows, in the tracker's order: build the feature, meet its Verify clause with evidence, record it in the ledger, push straight to main (no pull request needed). Before pushing extension changes, run at least one UI scenario that starts the extension. Rerun a timing check that failed under other agents' load alone before calling it a regression.
5. Owner-only rows: keep them on the README's owner-actions list (AC-160) and keep going on everything else. Avoid work that needs the owner: use the isolated test builds, never the owner's own VS Code.

HOW: criteria and Verify clauses in docs/overseer-rfc.md; designs in docs/rfcs/ and docs/design/; UI changes tested in the packaged VSIX with screenshots; paid turns only as AGENTS.md allows (ChatGPT: gpt-5.6-luna, low effort; Claude: haiku, light).

DONE WHEN the tracker only has rows waiting on the owner, every pull request is merged or closed with a reason (AC-161), `scripts/test-all` passes on main, and the ledger is current.
```
