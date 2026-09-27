# Main integration review — 2026-09-26, refreshed for Gate S

Compared Swarm draft PR #3 with `origin/main` at `759c080` after merging that revision
into `codex/swarm-mode`. This is a compatibility assessment, not acceptance evidence.
No SWARM or CONTRACT box changes status because of this review.

| Main addition | Current state on main | Swarm decision and required joined check |
| --- | --- | --- |
| Gate K/M agent sidebar, composer, chat, review and grid (AC-99–113) | Merged UI; some owner-review criteria remain partial | Put category/Swarm initiation in the existing launch flow and show director, jobs and workers through existing agent views. The grid's 16 visible tiles do not cap the 32-worker qualification. Exercise S0 and SWARM-01/23/27/28/39/56/63 through the packaged UI. |
| Audio Mode (AC-143–145) | Merged daemon and TUI cue paths | Derive cues from durable Swarm state. A cue cannot serve as a worker result or exit receipt. Check a stop/failure/needs-attention transition with multiple workers; do not create one alert per progress message. |
| Gate P/Q rules and test runner (AC-146–161) | `AGENTS.md` and `scripts/test-all` merged; several monitor and review criteria remain open | Keep draft PR #3 in flight, push criterion work promptly, and use the combined checks before marking it ready. Swarm evidence remains in the separate SWARM ledger; main's AC records do not verify it. |
| Continuity (AC-83–98, AC-138–140) | RFC/criteria on main, separate implementation in flight | Consume the daemon's network and local-memory authority when available. A handoff stays within the approved Swarm pool, permission mode, two-attempt job cap, allocation and deadline. Distinguish waiting from exit/settlement; test provider failure, offline, memory pressure and restart with one intent. Never add a second connectivity or quota collector. |
| Overseer chat, phone and voice controls (AC-107, Gate N/R) | Control-surface requirements on main; Gate N's latest commit changes phone presentation and pairing/notification defaults, not Swarm admission | Route confirmed user actions through the same daemon Swarm control/revision checks. An unsent voice proposal cannot replay itself, and no surface may bypass Stop, permissions, or the account pool. Qualify each surface only when its own gate is implemented. |
| Gate S: Overseer above agents (AC-180–202, especially AC-195/196/198/200) | New RFC and criteria on main; no Gate S implementation yet | Keep one Swarm director as the only job decision-maker. Overseer sees the director summary, uses Swarm controls or a sourced director advisory, and never directly steers a worker. One daemon broker and claims/conflicts ledger must cover Swarm workers and ordinary agents. Starting a swarm or raising its limit needs one owner confirmation even at Overseer's Auto level; S0 still needs no settings form. Gate S agents/watchers share the slot and account admission limits; Overseer's own coordinating run remains reachable and metered. Joined AC-195 and SWARM-07/20/24/27/39/44/60 checks remain partial until both features are integrated. |
| Gate S chat model setting and activation fix (AC-107) | `b46b512` adds the setting and scenario; `ff9349c` fixes activation | The Swarm director is a distinct run. Do not reuse the extension-local chat session as a director or depend on it for Stop, deadline or admission. When Gate S moves the conversation into the daemon, surface the same Swarm state there. |

The resulting RFC/contract clarification is in `docs/rfcs/swarm-mode.md` and
`docs/rfcs/swarm-auto-contract.md`; S0/S5 in `docs/rfcs/swarm-mode-scenarios.md`
now include the joined control cases. The approved SWARM-01–64 outcomes are retained
and their integration checks are more explicit. No Gate S or Swarm criterion is
verified by this document review.
