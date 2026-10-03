# Continuity scenario readiness correction

The full fixture run at `715ee85bb858d0d4c170de355aaed8316f8be416` failed the successor-chat assertion on 2026-10-03. [The recorded run](before.log) shows that the sole row before the click still named Codex and said waiting for a connection. The chat then showed the predecessor’s transition notes, while the following sidebar check observed the completed fold.

The test previously treated one matching title as proof that the sidebar had refreshed to the successor. That condition also matches the old, single predecessor row. The correction waits for the local-model row with a predecessor fold before clicking, then waits for actual successor notes before checking their full contents. It preserves the existing daemon status and exact chat assertions.

Syntax and whitespace checks passed. The required isolated packaged scenario has not run yet: the full suite still owns the machine-wide UI lock. This is not a passing UI or AC verification record.

Follow-up: the corrected scenario passed against the original packaged baseline; see ../ui-readiness/README.md for exact source/build provenance and limits. Combined full verification remains pending.
