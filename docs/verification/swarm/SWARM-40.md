# SWARM-40 — the run's allocation from a percentage allowance

Status: partial (2026-09-28, `claude/auto-swarm`). Everything in the clause is covered at fixture scope except the director's own turns in a run with more than one slot: they are not metered against the run's allocation until the director's `swarm/director` draw is qualified.

Criterion: with 60 percentage points of fresh unreserved/unprotected remaining weekly allowance and no override, derive at most 6 points for the run and a minimum 1.2-point finishing reserve. Apply a tighter short-window limit independently. Missing compatible worker estimates prevents default fan-out; no token conversion is fabricated. Planning, routing inference, messaging, reproduction and replacements share the original allocation.

| Clause | Test | State |
| --- | --- | --- |
| 60 points → at most 6 for the run, 1.2 reserve | `policy_uses_capability_allowed_pool_and_finishing_headroom`, `preview_uses_explicit_percentage_bounds_instead_of_builtin_values` (`swarm_policy.rs`: 60,000 milli-points → 6,000 and 1,200); `one_run_freezes_allocation_and_dedupes_replayed_admission` (admission freezes 6,000); the normal start's read-back shows the same arithmetic per window (`s0_variants_ask_once_fall_back_or_block_without_committing`) | covered |
| A tighter short window binds independently | `each_window_binds_and_provider_label_does_not_change_policy`, `short_window_and_unlike_unit_each_bind_admission_without_conversion` | covered |
| Missing compatible worker estimates prevent default fan-out; no token conversion | `unknown_stale_or_incompatible_units_do_not_enable_fanout`, `zero_upper_estimate_cannot_authorize_free_fanout`; route selection excludes a route whose draw is unknown (`upper_draw_unknown`); a Claude account with no usable reading reads back `serial` | covered |
| Reproduction and replacements share the original allocation | `workers_replacements_and_reproductions_share_one_frozen_allocation` (`swarm_admission.rs`, new; passed on first run): a worker, its replacement attempt and a reproduction book 4,500 of the 4,800 ordinary allowance on the one allocation row; a second reproduction is refused `finishing_reserve`; a later, larger observation does not enlarge the allocation | covered |
| Routing inference and messaging | route selection records `inference: not_used`; the broker makes no model calls, so neither draws allowance | covered (nothing to draw) |
| Planning shares the allocation | at one slot the director's own job is admitted against the run's allocation (`one_slot_director_executes_and_accepts_a_job_without_spawning_a_worker`); in a run with workers, the native director's turns run unbooked because its `swarm/director` draw has no qualified estimate, so they are not metered against the run | **gap** |

What would close the gap: a qualified `swarm/director` upper draw (a live calibration on the owner's Claude account, the RFC's D4 note: "a Claude account also cannot calibrate its draw from product readings today"), after which the director's booking can draw on the run's allocation. Class: needs a live account (b) and the owner's calibration decision (c, D4).

Run: `cargo test --offline -p overseerd --test swarm_admission -- --test-threads=1` (41 passed), `cargo test --offline -p overseerd --test swarm_policy -- --test-threads=1` (12 passed), 2026-09-28.
