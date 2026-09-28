# AC-212: reruns of the checks that failed under load

Branch `claude/prod-guard` at `93ea210b` (main merged in). Other agents were building and running scenarios on the same machine, so the load average was between 80 and 130 during the full runs.

| Check | Full run | Rerun alone | Note |
| --- | --- | --- | --- |
| protocol `ac45_one_notice_per_quit…` | failed | passed (load 29) | |
| protocol `ac51_worktree_tree…` | failed | passed | |
| overseer `ac192_conflicts_between_agents_in_flight` | failed | passed | |
| tui control `t19_a_new_waiting_agent_rings…` | failed | passed | |
| tui audio `t24_the_real_binary_rings…` | failed (test-all) | passed | |
| tui look `t10_nine_busy_agents_stay_responsive` | failed | failed at load 29 (event lag p95 269 ms); passed at load 4.5 (04:52) | `origin/main` failed it the same way at load 29 (322 ms), and main's own CI fails it too |
| UI composer, grid, center, audit, review-width | failed | passed | |
| UI review | failed | passed | `origin/main` failed it alone at load 71 (other checks) |
| UI gallery | failed | failed at load 70, passed twice at load 17 (main also passed twice then) | a focus race puts a palette command into the chat composer under load |
| UI arrangement (first edit within 500 ms) | failed | failed alone at load 20 (615 ms); passed twice at load 4.5 (04:50), as did main | AC-149's list of load-sensitive checks |

Every other suite and scenario passed in the full run (`test-all-jobs2.txt`, `cargo-workspace.txt`); the criterion's own tests are in `targeted.txt`.
