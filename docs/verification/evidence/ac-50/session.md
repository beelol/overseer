# AC-50 live session with the owner (2026-09-25 PDT / 2026-09-26 UTC, owner's Mac)

Hybrid check: the owner clicks in their own VS Code and approves the GitHub sign-in; the agent prepares the run
and reads GitHub with `gh` (read-only).

1. Agent asked which GitHub repository to use. Owner (quote): "hm can you make a new one under beelol.?"
   Agent created the private repository `beelol/overseer-pr-sandbox` (`gh repo create --private --add-readme`,
   default branch `master`) and cloned it fresh to `/tmp/ovs-ac50` (none of the owner's checkouts are involved).
2. Agent started one tiny live Codex run on the owner's daemon: `r-b3a97720b863`, account ChatGPT A
   (`p-f262c1bc4958`), model `gpt-5.6-luna`, title "Add Overseer PR check note", prompt: create `OVERSEER.md`
   with one line. It completed; the worktree has `?? OVERSEER.md` ("This pull request was opened by Overseer
   (AC-50 live check).") on branch `overseer/add-overseer-pr-check-note`.
3. **Bug found before opening the PR:** `workspace.pr_plan` reported `target: origin/master`. In a fresh clone the
   default branch is the remote-tracking `origin/master`, and a broken prefix strip kept the `origin/` — Open PR
   would have asked GitHub for base `origin/master`. Fixed in 4fa7166 (target `master`; local comparison uses
   `base_ref`), with regression test `ac50_pr_plan_targets_the_branch_name_in_a_fresh_clone` (fails before the
   fix with `origin/main`, passes after); full `cargo test` green. Rebuilt the VSIX, installed it, and restarted
   the owner's daemon with no runs active. The plan now reads
   `beelol/overseer-pr-sandbox: overseer/add-overseer-pr-check-note → master`, uncommitted `OVERSEER.md`.
4. Asked the owner to open the run and press **Open PR…**. Owner (quote): "Don't see where the "Add overseer" PR check node
   is, but pressing "Open PR" is not working." The extension log had no Open PR entry and no error.
   - **Bug:** from the Command Palette with no run selected, Open PR returned silently. It now offers a quick pick of
     worktree runs (2c2c7cf). The run sits under the repository row `ovs-ac50` in the Overseer view.
5. Owner (quote): "I press it. It clicks, but there's no feedback on anything. It should open it. Also, the open PR
   button should run that same command if it doesn't already" (it already does: the run panel's button runs
   `overseer.openPullRequest` for its run).
   - **Cause:** the owner's VS Code has **Do Not Disturb** on (`notifications.doNotDisturbMode = true`), which hides
     warning and info toasts — the "not signed in to GitHub" prompt, the "unavailable" reasons and the final
     "Pull request #N is open". **Fix** (0cdd312): Open PR answers with dialogs and logs each step; the UI scenario
     now runs with Do Not Disturb on (8/8 PASS). VSIX reinstalled with no runs active.
6. Owner reloaded and pressed Open PR… again. Extension log:
   `open PR for r-b3a97720b863: beelol/overseer-pr-sandbox overseer/add-overseer-pr-check-note → master`,
   `open PR: VS Code has no GitHub session with repo access` (so the sign-in dialog appeared and the owner approved
   VS Code's GitHub sign-in), then `pull request https://github.com/beelol/overseer-pr-sandbox/pull/1`.
   Owner (quote): "worked! amazing".
7. Agent checks (read-only `gh`, see `github-checks.txt`): PR #1 is OPEN, not merged, not draft; head
   `overseer/add-overseer-pr-check-note` = the worktree HEAD `45f52dd`; base `master`; title "Add Overseer PR check
   note"; author `beelol`; one file `OVERSEER.md` (+1); generated description (run, task, commits, files, "never merges
   automatically"). `master` is still `78db700 Initial commit`. The daemon recorded a `pull_request` event (URL and
   number only). Token-like strings (`gh?_…`, `x-access-token:`, `AUTHORIZATION: basic`) in Overseer's database, the
   daemon log and the extension log: 0, 0, 0; `extraheader` in the worktree's and the clone's git config: 0, 0.
8. Cleanup on the owner's explicit yes. Asked: close PR #1 and delete its branch (and the repository)? Owner
   (quote): "Yes clean up the pr". Agent: `gh pr close 1 --delete-branch` → PR #1 CLOSED, not merged; remote
   branches: `master` only; `master` still `78db700`. The private repository `beelol/overseer-pr-sandbox` is kept
   (repository deletion was not asked for).
9. Owner (quote): "Yes plus repo on the pr cleanup." Agent: `gh repo delete beelol/overseer-pr-sandbox --yes` was
   refused: GitHub requires the `delete_repo` scope, which this `gh` login does not have. The repository is still
   there (private, PR #1 closed, branch deleted). Owner action to finish: run
   `gh auth refresh -h github.com -s delete_repo` (browser approval), then the agent (or the owner) deletes it.
10. The owner granted `delete_repo` (`gh auth refresh -h github.com -s delete_repo`, approved in the browser).
    Agent: `gh repo delete beelol/overseer-pr-sandbox --yes` → the repository no longer resolves. Nothing of the
    AC-50 check remains on GitHub.
