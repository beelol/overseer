# Swarm implementation milestone 16 — effective budget percentages

The policy preview and fixture admission transaction now use the run's effective allocation and finishing-reserve percentages. Default requests still use 10% and 20%; saved application/category/run overrides follow the existing precedence and are frozen at first admission. Policy validation limits these percentage fields to 1–100.

With a 60,000-point window and an explicit 20%/30% run policy, the fixture reserves a 12,000-point allocation and a 3,600-point finishing reserve. It admits an 8,000-point worker and declines the next 500-point worker outside the remaining worker headroom. The focused test failed before the arithmetic was connected and passed afterward.

Evidence: `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_settings.rs`. SWARM-39 and SWARM-10 remain partial pending live launch, revocation, actual usage reconciliation and finishing-work draw.
