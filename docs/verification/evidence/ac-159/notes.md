# AC-159: the toolchain works without Xcode's license

Run on 2026-09-27 with `fixtures/xcode-license-git/git` first on the PATH. That stand-in behaves like `/usr/bin/git` when Xcode's license has not been accepted (it prints Apple's license message and exits 69) unless `DEVELOPER_DIR` selects the Command Line Tools.

- `PATH=fixtures/xcode-license-git:$PATH scripts/test-all --only=sidebar`: `note: the system git would not run; using the Command Line Tools git`, then Rust 121 passed, unit 2 of 2, the source check, the link check (612 links, none broken), the VSIX build and the sidebar scenario: 6 of 6 passed.
- `PATH=fixtures/xcode-license-git:$PATH node test/ui/scenario-sidebar.js`: `note (ui harness): …`, SCENARIO PASSED.
- `PATH=fixtures/xcode-license-git:$PATH node extension/scripts/package.js`: `note (packager): …`, the VSIX packaged.
