# Controller boundaries —9/9 pass, no native runtime

Coordinator ran the exact frozen controller source `9fb19943df293767bd0ecae0d0e6175a8174c4c0` with `nice -n20 python3 -B -m unittest discover -s test/ci -p native_speech_diagnostic_test.py -v`. Authoritative terminal tool chunk4ae2ef reports exit0,9/9 passing in0.194s. [Receipt](receipt.json) preserves command, counts, test identities and SHA256 of the unchanged tested source files. The native module remains byte-identical to previously compiled3bbd089. The docs-only evidence follow-up did not rerun tests.

Seven cases inject the build/native child-process boundary: no real Cargo/library build/native invocation occurs. Two capture controls start only bounded synthetic Python children, proving output-cap termination and deadline kill/reap. No UI, model/provider, owner profile or account data is exercised. Files/source remain clean at the tested commit until this evidence-only update.

The raw original stdout/stderr was not supplied to this evidence writer; this is an exact coordinator terminal-result receipt, not an invented transcript. Hashes are captured after the reported run from the clean committed source; a separate pre-run hash receipt and actualNI process sample were not captured for this tiny run. Preserve those limits rather than reconstruct them.

The actual CI controller build/native pipeline, remote workflow, ignored native diagnostic and Rust callback-bound test remain unrun. No dispatch was performed and no further test ran. Passing Python controller boundaries do not qualify speech completion or explain the original85sample CI failure; that failure remains unresolved. No ordinary retry/assertion/poll/dispose behavior changed, no full-suite pass or criterion closure is claimed.
