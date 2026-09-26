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
