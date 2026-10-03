# AC-200: structured and escaped credential redaction

Synthetic-only regression evidence for the narrow redaction fix based on main `bc19baa5`. All model harnesses are disabled except the existing traffic test's fake Claude program. No paid calls, owner credentials, UI, full suite or production daemon was used. AC-200 remains partial; no historical database migration or historical session scrub is claimed.

The [exact Rust probe](probe.rs) and [output](output.txt) confirmed the old escaped-quote parse failure and original-action fallback. The probe includes the saved [pre-fix source](redact-baseline.rs) at `7a7182e2`; integration tests reproduce against this branch's main baseline.

Red evidence:

- [Integration red](integration-red.log): three intended credential-leak failures after successful real stdin-reading fixture turns, not setup failures.
- [Plaintext unit red](unit-red.log): escaped quoted value leaks its suffix.
- [Key unit red](key-unit-red.log) and [accepted proposal key red](key-integration-red.log): reviewer found the reused helper left credential-shaped object keys unchanged.
- [Legacy card red](legacy-card-red.log): restore only the old card read line to prove that test catches the independent read boundary; no history rewrite.

Final green evidence:

- [Units](unit-green.log): 4/4, including complete escaped-value removal, nested values, safe control fields and deterministic key collision ordering.
- [New integration file](integration-green.log): 5 new regressions and 4 shared helper tests, 9/9 total. Inspect stored proposals, before/after cards and session replies, successful delivered turn input, repeat-confirmation refusal and unchanged safe context.
- [Existing credential traffic](traffic-green.log), [proposal/card governance](cards-green.log), [redirect/queue](queue-green.log): one existing integration test each.

Commands from the repository root, using two build/test workers and low CPU priority:

```sh
export CARGO_TARGET_DIR=/private/tmp/overseer-closeout-pr49-target
nice -n 20 cargo test -p overseerd --bin overseerd redact::tests -j 2 -- --test-threads=2 --nocapture
nice -n 20 cargo test -p overseerd --test overseer_redaction -j 2 -- --test-threads=2 --nocapture
nice -n 20 cargo test -p overseerd --test overseer -j 2 -- ac200_no_credential_in_overseer_s_traffic --test-threads=2 --nocapture
nice -n 20 cargo test -p overseerd --test overseer -j 2 -- ac185_actions_have_classes_and_cards --test-threads=2 --nocapture
nice -n 20 cargo test -p overseerd --test overseer -j 2 -- ac188_redirect_and_the_queue --test-threads=2 --nocapture
```

Before-fix logs include expected failed assertions. Compiler warnings are pre-existing unused/dead-code warnings. Final validation and integration/full-suite qualification remain separate gates; the new file is automatically included by ordinary workspace tests. Evidence contains only planted synthetic secrets.
