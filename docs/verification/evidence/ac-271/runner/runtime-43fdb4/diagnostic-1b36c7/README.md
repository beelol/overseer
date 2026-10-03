# Bounded synthetic startup diagnostic

At source1b36c762b495ae7c15302dd140a49e60dac7f7c5 the coordinator added an opt-in cfg(test)-only diagnostic of the already bounded child error buffer and exit status. Package clean, fresh exact test build and the single echo assertion ran. The child exited with SIGABRT6 and an empty error buffer; the expected bytes assertion still failed. No sandbox policy changed, and this does not identify the denied operation. Both command receipts and source/compiler provenance are retained. Cleanup reports zero owned/zero other leftovers.

Next investigation should isolate loader startup requirements with a single narrowly scoped test-only policy change or a test-owned crash/denial trace. Do not broaden the production policy merely to make this test pass. No owner inputs or downloaded transformer were involved.
