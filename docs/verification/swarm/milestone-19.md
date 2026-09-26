# Milestone 19 — interrupted integration reconciliation

Revision: `fca22c0`. This is partial SWARM-18/19/35 evidence, not acceptance-criterion completion.

An accepted fixture patch now has a durable integration intent before the integration worktree changes. The intent binds the artifact digest, prior commit and expected Git tree. A daemon interruption after staging the patch resumes by verifying that exact staged tree; an interruption after Git commit resumes by verifying the commit parent, tree, subject and clean worktree. SQLite acknowledgement and dependent release occur together. A changed source HEAD after the Git commit does not prevent reconciliation of an existing intent. Unexpected worktree edits fail closed and retain the pending intent for manual reconciliation.

The recovery fixture was red against the previous implementation. Eight focused integration fixtures passed after the change, including two restart windows and an unexpected-edit rejection. `cargo test --workspace --offline -q` passed 168 Rust tests; `git diff --check` passed. No live provider, paid account, or user repository was used.

Ruling: retain fixture-only integration. Combined verification, director-led conflict handling, normal adapter authority and a live end-to-end scenario remain open, so no RFC acceptance box changes.
