# AC-147: one command runs every test

`scripts/test-all` on the Gate P branch (based on main at the time), 2026-09-27.

- `full-run.log`: every part with its counts. Rust 92 passed; unit 2 of 2; the extension check; the VSIX build; 34 packaged-UI fixture scenarios. Two scenarios failed on main and are reported as failures: `restore` still used the Gate J layout (the Gate K follow-ups pull request ports it) and `main` lost one tree click (the same pull request stops the tree redrawing under a click). Live scenarios and the load test are listed as not run: they need `--live` and `--perf`.
- `deliberate-failure.log`: a unit test that always fails, added for the run and removed after it, is reported as `FAILED` with its name, and the summary and exit code say so.

`npm test --prefix extension` runs the same unit tests (`test/unit/run.js`).
