# Host correction source checkpoint

Root executed unchanged production source with the expanded `a0a55c4` fixtures:
**2 PASS, 9 RED**, preserved in `initial/host-expanded-baseline.log`. Both controls
(explicit re-enable, Off during title lookup) passed. All nine failure assertions
were reached, including the stale title lookup's null dereference. No setup
failure is used as product evidence.

The candidate changes only `extension/src/voice.js`:

- A connection epoch invalidates all pre-disconnect requests and title lookups;
  a refresh ticket prevents older success/error from overwriting newer work.
- Three current live projections retain state/reason, target and targeted
  agents, with arrival revisions. A complete get supplies static model,
  availability and listener fields. Live projections newer than its request are
  merged; with no cached complete snapshot yet, pre-snapshot projections are
  also retained. The presence of `this.voice` itself denotes a complete cached
  snapshot; there is no extra flag that could disagree with it.
- Snapshot commit/render/publication is synchronous before roster I/O. A title
  lookup captures its snapshot object, target, target revision and epoch; it can
  update and publish only while all remain current. Enrichment publication is
  synchronous with that final guard, avoiding a second awaited publication
  window. It uses the object's current live state.
- Disconnect clears the snapshot/projections/badges. Failed refreshes do not
  clear healthy newer cached state; the disconnected event owns clearing.

Tests were not changed or weakened during implementation. The candidate is
**UNRUN**: only source inspection and `git diff --check` were performed by the
author. Root owns the next focused host and packaged qualification. This is not
a readiness, merge or full-suite claim. It remains independent of the two
intermittent original packaged re-enable failures; the same-package diagnostic
pass reached no timeout capture, so their cause is still unproved.

This addresses host snapshot/state/title ordering only, not a new server-side
sequence guarantee or a redesign of asynchronous spoken-request collection.
