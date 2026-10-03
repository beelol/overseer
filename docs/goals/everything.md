# Goal: get everything done

Paste the block below into `/goal`.

```
Meet every acceptance criterion on main in Overseer (beelol/overseer), keep watching the other agents and merge their work when it is ready. Follow AGENTS.md.

"Everything" is docs/verification/tracker.md: every open criterion (AC-NN in docs/overseer-rfc.md, T-NN, AUTO-AC-NN, SWARM-NN), its owner (this goal, another agent, the owner) and where its work is.

EACH PASS
1. Fetch main and re-read its README acceptance list and docs/overseer-rfc.md Verify clauses, including owner edits and newly added criteria (AC-274 included). Main is authoritative; a worktree copy or the handoff cannot freeze or narrow this goal. Preserve local work while reconciling changes. Refresh the tracker from the ledger, `gh pr list` and `git worktree list`. New criteria and agents get a row; verified rows leave.
2. Other agents: read each one's recent commits and pull request. Ask on the pull request, or message a local Claude session, when an agent does any of these: works outside its criteria, fails tests, leaves work unpushed for 3+ hours, loops (merging main and noting it, over and over), or polishes edge cases while its basic path is unproven (AC-157). Tell the owner when a decision is needed.
3. Stalled or stopped agents: take their criteria over (AC-203). Branch from their pushed work, keep uncommitted work as a patch and use it only once it builds and passes. Never push to their branch. Hand the work back if the agent resumes.
4. Merging (AC-146): once a pull request is marked ready, or its agent is quiet 3+ hours and says it is done:
   - merge main into a throwaway copy, keep both sides working;
   - run `nice -n 20 scripts/test-all --jobs=1` (one full run on the Mac at a time) and the UI scenarios it touches;
   - send failures back to its agent, or fix them if it is gone;
   - land a merge commit preserving both histories, then record its criteria (AGENTS.md/AC-146).
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
- Paid turns only as AGENTS.md allows (gpt-5.6-luna at low effort; no Claude model for now). Never touch the owner's checkouts, logins or daemon. Leave nothing running.
- Pace (the owner, after the Mac crashed on 2026-09-29): start with ONE builder sub-agent and ONE full test run (`--jobs=1`, `nice -n 20`, `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4`); add a second or third builder only while `uptime` stays low; always leave room for the owner's own apps (games) and a couple of other agent tasks. Keep `$TMPDIR/overseer-test-jobs-max` at 1 (a reboot clears it).
- Push often. Keep docs/goals/handoff.md on main current after every change, and tell the next agent to do the same.
- Tell the owner in a few lines: what landed, what is blocked, what they must do.

DONE WHEN the tracker only has rows waiting on the owner, every pull request is merged or closed with a reason (AC-161), `scripts/test-all` passes on main, and the ledger is current.
```
