# AC-203: takeovers and hand-backs

## Gate N, the phone remote (2026-09-27)

- **Stalled:** pull request #10's last commit was `bb6ffad` (06:53 UTC). The monitor asked for a push at 14:07 UTC and got no reply and no commit by 17:08 UTC.
- **Taken over:** said so on pull request #10, branched `claude/phone-takeover` from `bb6ffad`, merged main (twelve conflicts, both sides kept), classed main's new daemon methods for the phone, made the phone's icons from the owner's brand files (AC-178), and fixed scenarios. Nothing was pushed to the agent's branch; its worktree was left as it was.
- **Handed back:** the agent resumed that evening and asked for the devices. The monitor stopped its own work, and the agent merged `claude/phone-takeover` into its branch (`c22ce427`). The tracker gave Gate N back to it (`a7791fdd`).

## Auto and Swarm (2026-09-27)

- **Stopped by the owner**, who gave both RFCs to the everything goal to build together.
- **Pushed state:** Auto `e77245c1`, Swarm `f701fddd`. Swarm's worktree was clean.
- **Uncommitted work kept:** Auto's worktree held an untested launch-booking wrapper in `daemon/src/account_booking.rs` and `daemon/src/store.rs`. It was saved as a patch without touching the worktree. It was applied on the new branch `claude/auto-swarm` (from `e77245c1`) and formatted. Its four booking tests, including the agent's red-first `launch_booking_holds_writer_slot_and_claims_effects_once`, and the 207 daemon unit tests ran before the commit `6abf8d71`.
- **Handoff notes:** summarised in the handover sections of `docs/rfcs/auto-mode.md` and `docs/rfcs/swarm-mode.md`. The tracker names the goal as the owner of both.
