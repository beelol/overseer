# Exact root-directory allowance: sixteen runner checks pass

At e7a74019ea8804b8acde4d35ec6f06a5e4498239, root47074 passed all sixteen unchanged synthetic runner cases, each with its exact inventory. Fresh package clean/compiler receipts and three dependency-source hashes are attached. These checks cover successful exact-byte execution, timeout/output limits, environment, cancellation, staged-program identity and denial of protected/other-run/symlink files, TCP/Unix connections and fork/spawn, with unrestricted synthetic positive controls.

The controlled test-only probe at1ab71459b5b27292e8deb1b628ca35f44818bd90 changed only literal root-directory file-read-data and made the previously failing echo case pass. The resulting source change allows reading `/` itself, not descendants, and removes the temporary probe switch. This supports a startup-prerequisite inference; it is not a traced denied syscall or crash backtrace. No other policy was widened. Independent read-only review by queue_pause found no source blocker and inspected all16 receipts.

Qualified cleanup reports zero owned and one unrelated transient Git process (PID46331). A separate root ps check returned no such process (exit1, header only); it was never stopped. The probe cleanup was zero/zero. Do not report the qualified receipt as zero/zero.

Prior ten runtime failures, original dep-info wrapper setup failure and SIGABRT diagnostic remain in ../runtime-43fdb4. No installed transformer, native harness hook, install/recipe governance, token savings or AC271–273 closure is established. The runner remains unregistered. No paid provider, owner input, private audio or production daemon was used.
