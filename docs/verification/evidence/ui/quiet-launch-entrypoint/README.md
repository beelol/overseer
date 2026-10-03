# Quiet launcher entrypoint correction — 2026-10-03

Source baseline: main `9dc165d0`. Verifier: Codex. This checkpoint contains source/syntax tests only; no packaged UI, app launch, build, model, microphone or production activity. The full-run machine UI lock was held during this work.

The baseline notify-agents log at `/Users/bilal/.codex/worktrees/verify-voice/overseer/docs/verification/evidence/ui/notify-agents/scenario.log` reports “VS Code window constructor not found” at 08:04:14.938 UTC, then never observes `vscode_focused:false`. Its isolated daemon log `/private/tmp/ovs-ui-TMWE97/overseer-home/overseerd.log` lines 7–11 suppresses all notifications because VS Code is focused. The empty notification log follows that real focus state; it is not evidence of a notification delivery failure.

The installed VS Code 1.140.0 bootstrap `out/main.js` contains no BrowserWindow constructor and imports `./mainImpl.js`. The unchanged constructor matcher finds `new Jn.BrowserWindow(et)` in that imported file. [installed-entrypoints.json](installed-entrypoints.json) records actual byte lengths, hashes and the zero-based breakpoint location without copying or executing installed source.

The same missing breakpoint explains the baseline popout focus failure: its scenario log reports initial window-created at 167 ms, app-active at 227 ms and window-focused at 270 ms, before floating windows at 8,691 and 19,057 ms. The latter use the existing hidden-window patch and show no additional activation events. This is evidence of initial-launch activation; no floating-window product change is included.

The correction reads only the two known packaged entrypoints, at most 16 MiB each, and registers exact by-URL breakpoints before the paused bootstrap resumes. It supports the old inline constructor and new imported constructor; an unknown layout is refused before an app is launched. Source is read as text and never evaluated. Existing showInactive/floating-window handling and actual notification focus assertions remain unchanged.

[red.log](red.log) shows the five new source tests failing before the helper exists. [green.log](green.log) shows 5/5 passing: old main constructor coordinates, imported mainImpl pending URL registration (spaces/raw/file URLs), refusing unknown import layouts, no source execution, and bounded size discovery. `node --check test/ui/quiet-launch.js`, `node --check test/unit/quiet-launch.js`, and `git diff --check` passed.

Independent source review accepted the exact-URL pre-import breakpoints, preserved options mutation/inactive handling, unchanged actual focus assertions, and bounded old/new entrypoint tests before commit. Isolated `notify-agents` and `popout` packaged proofs will run serially only after the coordinator releases the UI slot, against final integration of PRs #52/#53/#54. Until then this is not a claimed packaged UI pass, AC verification or full-suite result.
