# Goal: get everything done

Paste the block below into `/goal`.

```
Close every open criterion in Overseer (beelol/overseer) and merge every in-progress pull request once its agent has finished. Follow AGENTS.md.

"Everything" is docs/verification/tracker.md: every open criterion (AC-NN, plus the TUI's T-NN and the Swarm and Auto criteria in their own RFCs), who owns it (this goal, another agent, or the owner) and where its work is (main, a branch, a pull request).

EACH PASS
1. Refresh the tracker from the ledger (docs/verification/records.py, docs/verification/README.md), `gh pr list`, and `git worktree list`: new criteria and new agents get a row; verified rows leave.
2. Rows owned by another agent (Continuity, the phone app, Audio Mode, Swarm, Auto, or any new one): read the agent's recent commits and pull request. Check it is working on its own criteria, its tests pass, it pushes regularly and it isn't looping; if not, comment on its pull request with a clear ask, and tell the owner when a decision is needed (AC-157). Never push to or merge an agent's pull request while that agent is still working.
3. When an agent has finished (its pull request is marked ready, or it has been quiet for 3+ hours and says it is done): merge main into a throwaway copy, fix conflicts so both features keep working, run all the tests, then merge with a merge commit and record its criteria in the ledger (AC-146). Stacked pull requests go after their base. Never force-push.
4. Rows owned by this goal: do the next one, in the tracker's order (PR #8 and AC-154/155, then Gate Q, Gate P, Gate M with the theme and immersive look first, then the logo when its files arrive), and push after each criterion.
5. Rows owned by the owner: list them in the README's owner-actions list (AC-160) and keep going on everything else.

HOW: criteria in docs/overseer-rfc.md with their Verify clauses; designs in docs/rfcs/ and docs/design/; UI changes tested in the packaged VSIX with screenshots; design work on a review page for the owner's marks.

DONE WHEN the tracker only has rows waiting on the owner, every pull request is merged or closed with a reason (AC-161), all tests pass on main, and the ledger is current.
```
