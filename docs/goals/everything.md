# Goal: get everything done

Paste the block below into `/goal`.

```
Meet every acceptance criterion in Overseer (beelol/overseer), keep watching the other agents and merge their work when it is ready. Follow AGENTS.md.

"Everything" is docs/verification/tracker.md: every open criterion (AC-NN in docs/overseer-rfc.md, T-NN, AUTO-AC-NN, SWARM-NN), its owner (this goal, another agent, the owner) and where its work is.

EACH PASS
1. Refresh the tracker from the ledger, `gh pr list` and `git worktree list`. New criteria and agents get a row; verified rows leave.
2. Other agents: read each one's recent commits and pull request. Ask on the pull request, or message a local Claude session, when an agent does any of these: works outside its criteria, fails tests, leaves work unpushed for 3+ hours, loops (merging main and noting it, over and over), or polishes edge cases while its basic path is unproven (AC-157). Tell the owner when a decision is needed.
3. Stalled or stopped agents: take their criteria over (AC-203). Branch from their pushed work, keep uncommitted work as a patch and use it only once it builds and passes. Never push to their branch. Hand the work back if the agent resumes.
4. Merging (AC-146): once a pull request is marked ready, or its agent is quiet 3+ hours and says it is done:
   - merge main into a throwaway copy, keep both sides working;
   - run `scripts/test-all --jobs=3` and the UI scenarios it touches;
   - send failures back to its agent, or fix them if it is gone;
   - squash-merge and record its criteria.
   Stacked pull requests merge after their base. Never force-push.
5. Partial merges (AC-204): when a slice is finished and tested, and main gains from it or another agent is blocked on it, merge that slice without waiting for the rest. Each unfinished part stays partial with its gap, or becomes a new criterion with a Verify clause and a tracker row. Unfinished features stay behind a setting or fixture gate.
6. This goal's own rows, in the tracker's order:
   - Auto and Swarm together on `claude/auto-swarm`. Auto owns the one account booking; Swarm uses it. Order: the booking with run binding and recovery, then Swarm on it, then one launch transaction, then a normal swarm run (S0), then the rest.
   - Then the other open rows.
   Build the feature, meet its Verify clause with evidence, record it and push. Before pushing extension changes, run one UI scenario.
7. Owner-only rows go on the README's owner-actions list (AC-160). Keep going on everything else; use the isolated test builds, never the owner's VS Code.

RULES
- Ledger: pull right before each records.py edit; records.py refuses a stale copy (AC-153).
- New criteria: fetch main first and take the next free number.
- A timing test that fails under load is rerun alone before calling it a regression. Agree quiet windows with agents running measurements.
- Paid turns only as AGENTS.md allows (ChatGPT: gpt-5.6-luna, low effort; Claude: haiku, light). Never touch the owner's checkouts, logins or daemon. Leave nothing running.
- Tell the owner in a few lines: what landed, what is blocked, what they must do.

DONE WHEN the tracker only has rows waiting on the owner, every pull request is merged or closed with a reason (AC-161), `scripts/test-all` passes on main, and the ledger is current.
```
