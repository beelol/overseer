# UI readiness corrections — 2026-10-03

Continuity previously clicked the predecessor row while its successor was still being rendered. The test now waits for the actual Local model successor and both successor notes before making the existing assertions. The original failure is retained in ../ac-91-readiness/before.log; source correctionc71ab4f3.

The zero-friction shortcut check previously captured editor readiness before the Manual edit status rendered. Its failing screenshot already showed Manual edit and the next real file-save check passed. Correction590caf20 includes the asserted status in the existing typed and spoken readiness predicates. The15s timeout, action count, focused editor/title checks, real save and Follow-return assertions remain. This measures the complete rendered result rather than an intermediate state. Independent source reviews accepted both corrections.

Both scenarios passed once in serial on 2026-10-03 in the disposable integration clone. Test source head09cf6715 includes both corrections and quiet-launch3716a1bc. The packaged build was explicitly the ORIGINAL715ee85b VSIX; its hash is retained. These runs qualify the test corrections against their failing baseline, not the newly integrated product or a full suite. The final full run must rebuild its own package.

Each directory contains the scenario result, full log, stdout/cleanup log and representative screenshots. No paid turn or production daemon was used. The scenario cleanup exited before the next one began.
