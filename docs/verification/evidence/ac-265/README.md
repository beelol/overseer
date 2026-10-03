# AC-265 fixture evidence

All checks use isolated data/profile folders, synthetic harnesses and speech input. No paid turns or owner credentials.

- `daemon-green.log`: seven AC-265 queue tests and one actual Continuity handoff test. Covers typed redirect + spoken addition before Stop, ten seconds no send, restart persistence, no model resume action, voice Stop/Send queued/Clear, FIFO completion timestamps, empty-resume batching and stale predecessor controls after handoff.
- `tui-green.log`: grid and Q controls against the isolated real daemon; actual queued redirect before Stop. Text/SVG screenshots are in `../tui/queue-pause/`.
- `ui-green.log`: all seven packaged UI checks, including actual typed Alt+Enter redirect (`redirect: true`) + spoken addition while running before Stop. Screenshots and structured result are in `../ui/queue-pause/`.
- `parity-green.log` and `parity-result.json`: existing unchanged composer parity, five checks passed; its screenshots are retained here without replacing baseline evidence.
- `unit-green.log` and `source-check.log`: all 27 extension unit files and source check passed.
- `daemon-red.log`, `tui-red.log`, `ui-grid-red.log`, `grid-before-fix.png`: actual missing behavior before implementation/fix. The grid failure showed host availability excluded a stopped agent's paused queue.

The synthetic slow harness has optional `FIXTURE_INTERRUPT_DELAY_MS=1500` only in the acceptance checks, so the real pending redirect remains observable before explicit Stop. Default delay is zero; unchanged parity confirms normal Stop-and-send and addition batching.

Full `scripts/test-all` is coordinated separately before this draft becomes ready for review. No full-suite result is claimed here.
