# Audio decoder exit between child-status and memory checks

Auth causality remains unresolved: the nine-case batch missed the sign-in cue,
but its single isolated rerun passed with unchanged daemon bytes. This separate
witness tests a source-confirmed decoder race, not that missing-cue cause.

## Existing boundary

`daemon/src/audio/decode.rs` checks owned `Child::try_wait`, then separately
queries `proc_pidinfo(PROC_PIDTASKINFO)`. A short memory response calls
`kill(pid, 0)`; success is treated as a live worker with unreadable memory and
refused. An owned child can become an unreaped zombie between the two checks.
Apple's [kill implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c)
explicitly returns success for a positive zombie PID. Its
[process-info implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c)
only looks up zombies for selected flavors; task-memory info uses the ordinary
live-process lookup. These are primary source references inspected2026-10-03,
not runtime proof on this Mac.

`pack::ValidatedPack::open` propagates decode errors to `player::play_checked`.
The Audio worker logs only a generic playback failure. A decoder refusal can
therefore leave a live cue unplayed without a sink receipt. The original auth
fixture did not retain its daemon log/database; no such refusal was observed.

## Authored deterministic witness (UNRUN)

One actual daemon API fixture selects a complete synthetic short-WAV pack while
off. The existing WORKER gate keeps its exact owned first decoder alive. After
the parent observes `try_wait=None`, an environment-only observation hook:

1. Registers `EVFILT_PROC/NOTE_EXIT` for that owned child before releasing it.
2. Releases its matching worker gate and waits at most1s for the exit event.
3. Uses bounded `waitid(P_PID, WNOWAIT|WNOHANG)` readiness checks, without
   reaping, to retain the exited child's identity and record normal exit0.
4. Records the matching PID and real kill-zero result, then executes the
   unchanged memory/error path. No status or output is substituted.

The test distinguishes setup from behavior: it requires the actual matching
worker, normal successful exit, no reap and kill-zero0 before requiring valid
selection to succeed. Registration/exit/readiness failures are setup errors.
Kqueue is descriptor-owned; existing ReapedChild owns kill/reap on every return;
the fixture's release-on-Drop guard remains. Native playback is never invoked.
The original2s validation limit and all memory/output/media checks remain.

## Proposed correction, only after genuine baseline

On footprint-query failure, recheck the **owned Child**. If it now has terminal
status, preserve that status and finish ordinary bounded stdout/status/duration
validation. If it is still live, retain the memory-supervision refusal. Never
treat `kill(pid,0)` alone as proof of execution, success or whole-group cleanup.
Do not accept a nonzero exit or malformed/missing duration merely because it
became waitable. No retry, larger deadline, sandbox relaxation or detached worker.

## Negative controls required before closing the correction

- Existing `truncated_wav_and_non_decodable_mp3_refuse_without_committing_selection`
  exercises actual malformed media and worker failure, with unchanged selection.
  Its later assertions are authored but have not yet reached runtime qualification.
- There is no existing independently injected malformed worker-stdout fixture.
  Add a bounded synthetic worker-result control before claiming output parsing
  stayed qualified; do not rely solely on the implementation's parse condition.
- There is no existing genuine-live unreadable-memory fixture. A deterministic
  fault-injection control may cover refusal routing, clearly labeled as injected;
  it cannot qualify real OS memory enforcement. Preserve the live-child refusal
  and the provisional96MiB supervision limitation in the interim.

This checkpoint is test-only/source-only. No compiler, daemon, helper, player,
private audio, provider or UI ran. No RED/GREEN, auth fix, AC closure, native FD
playback, resource enforcement or cleanup qualification is claimed.
