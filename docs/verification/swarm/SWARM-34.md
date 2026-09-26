# SWARM-34 — bounded worker and director context

Status: partial. Fixture-only implementation at `641a9f0`; no live harness qualification.

Input: a 100-job category plan with 97 independent jobs and a parent with two dependent jobs. The parent produces an 80,000-byte UTF-8 artifact. One child is admitted to the same target as the parent; the other is admitted to a different target. The test requests a 4,096-byte director page and worker brief, then reads the artifact in chunks.

Expected: a bounded director summary and job-specific worker brief, without peer transcripts or inline artifact content. Only a permitted destination can retrieve the artifact, including content too large for the brief. The launch path must carry the brief to the supervised worker.

Actual: the summary reports all 100 jobs in aggregate and pages job labels within the byte limit. The same-target child brief includes its acceptance check and the accepted parent artifact reference; the other target receives no reference. The same-target child retrieves the artifact in bounded chunks. Cross-target, wrong-token and non-UTF-8-boundary requests fail. A scripted worker's saved prompt contains its assignment and artifact reference but not the artifact body. The brief and combined launch prompt each have a 32 KiB ceiling, and required context errors instead of silently truncating. No peer transcript is copied.

Verification: `cargo test --offline --test swarm_context` passed 1 test. `cargo test --workspace --offline` passed 110 tests (5 unit, 43 protocol, 62 Swarm). `git diff --check` passed. Evidence: `daemon/tests/swarm_context.rs`, `daemon/src/swarm/context.rs`, and `daemon/src/swarm/runtime.rs`.

Remaining: this is a fixture protocol, not a live director or native harness path. It conservatively permits dependency transfer only when both attempts use the exact same admitted target; it cannot express a user-approved cross-target destination grant or revoke one independently. It does not yet include owned resources, target constraints, quota allocation and full audit scope in the brief, or measure token cost. Context requests go through the daemon rather than a live director decision. Large artifact references that alone exceed the inline limit currently return an explicit error instead of a paged reference list. SWARM-34 stays unchecked.
