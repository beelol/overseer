# Goal: get everything done

Paste the block below into `/goal`.

```
Get everything in Overseer (beelol/overseer) done, and merge every in-progress pull request once its agent has finished.

Every pass:
1. Check on the other agents: Codex Swarm (PR #3), Codex Auto (PR #2), Claude Audio Mode (PRs #5 and #6), Claude Continuity (Gate L) and Claude phone app (Gate N). Make sure each one is doing real work on its own criteria, its tests pass, and it pushes regularly. Auto has 100+ unpushed commits, so ask it to push. If something looks wrong, comment on its PR with a clear ask, and tell me if I need to decide something.
2. Merge any PR whose agent is done (marked ready, or quiet for 3+ hours and says it's done). Merge main into a copy first, fix conflicts so both features still work, run all the tests, then merge. Never touch an agent that's still working. Never force-push.
3. Then work on the next open criterion, pushing after each one, in this order:
   - PR #8 (Gate K follow-ups), once I've OK'd the review page, plus AC-154 (composer pills fill the row) and AC-155 (one-line search with a filter dropdown)
   - Gate Q: shared agent rules (AC-156), owner-actions list (AC-160)
   - Gate P: one test command, GitHub checks, a steady test suite, the first click always landing, live and performance reruns, ledger link check
   - Gate M: the bold "Overseer" theme and the immersive look first, then the rest
   - The logo (AC-142), once I add the files
   - The Claude parts of AC-81 and AC-45, once I sign in to Claude Code

Rules: follow AGENTS.md. Test every UI change in the packaged VSIX with screenshots, and put design work on a review page for my marks. Only ask me for things only I can do, and keep working on everything else meanwhile.

Done when every criterion is verified or only waiting on me, every PR is merged or closed with a reason, all tests pass on main, and the ledger is up to date.
```
