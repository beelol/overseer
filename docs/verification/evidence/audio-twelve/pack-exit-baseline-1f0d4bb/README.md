# Pack/source and owned-decoder baseline: 13 passes, 10 failures

Root session51683 is terminal at source
`1f0d4bb175dbce1667b04095f936f080d71fc278`. It ran all23 selected
`audio_packs` cases individually, once each:13 passed and10 failed. The wrapper
captures each exit rather than failing immediately; its terminal0 is not a suite
pass. Actual names, per-case exits and reached boundaries are in
[classification.json](classification.json); passing names are retained in
[pass-inventory.json](pass-inventory.json).

The package-only clean passed in5.423s, standalone build in20.766s and test
compilation without execution in21.892s. Both compiler artifacts report
fresh:false, with matching clone source, dep-info and executable hashes in
[receipt.json](receipt.json). Raw compiler JSON and `.d` files are preserved.
All case stdout/stderr streams are byte copies from the root batch. Cleanup
reports zero owned/zero other leftovers in0.38s. No case wrapper timed out;
the disable fixture's own bounded RPC receive assertion failed.

## Reached boundaries

- Thirteen actual passes cover inventory/phrases, complete folder/off/privacy/
  restart, manifest/path/revision/removed-folder/legacy rules, size/duration/
  malformed-media/FIFO limits, strict fields, enable-transition ordering, and
  held-live injected-memory refusal. Read the individual receipt for scope;
  these results do not qualify all Audio Mode behavior.
- `successful_decoder_exit_between_wait_and_memory_check_keeps_valid_selection`
  reached its owned matching-PID, exit0, unreaped and kill-zero controls, then
  failed on `Cannot supervise audio validation memory.` This is the intended
  decoder-exit RED. It is not proof of the intermittent auth cue's cause.
- Malformed-stdout and actually failed-decoder controls also refused at that
  earlier memory boundary. Their intended malformed-output/nonzero-exit guard
  assertions were **not reached**. The genuinely held live worker with injected
  unreadable footprint passed its memory-refusal assertions; this is injected
  refusal routing, not native OS memory-enforcement proof.
- Three actual shutdown/disable failures remain: an owned decoder survived
  daemon shutdown, closing accepted a folder source change, and disable did not
  promptly finish the held source-validation RPC. No shutdown correction is in
  this baseline.
- Three preview fixtures stopped at the explicit missing canonical preview
  queue; the resolved-permission fixture never reached its held admission gate.
  This pack branch lacks the semantic-worker merge. These are integration
  prerequisites, not reached source-switch/freshness assertion failures.

## Preservation and limitations

[SHA256.json](SHA256.json) pins the retained files and the original root receipt.
The published receipt omits only `baseline_pids`; raw process-inventory streams
are deliberately omitted. The original scratch receipt remains unchanged at
`/private/tmp/overseer-audio-pack-exit-baseline-20261003`.

No private audio, paid provider, native playback, owner profile or production
daemon was used. Built-in assets, native opened-FD playback/resource bounds,
whole-group cleanup, full integration and AC closure remain unqualified. Later
correction source `f56816d` was not executed by this baseline.
