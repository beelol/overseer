# Verification ledger

No product acceptance criterion has been implemented or verified in Overseer.
The RFC reconstruction and publication are documentation work, not product test evidence.

The authoritative checkboxes are in [the RFC](../overseer-rfc.md). Create an evidence
record named `AC-NN.md` when work starts; until then every AC is `not started`.
Do not duplicate checkboxes here.

## Record template

```markdown
# AC-NN — criterion title
Status: in progress | implemented / unverified | blocked | verified
Tested implementation commit:
Verification date and verifier:
OS / architecture / VS Code / harness versions:
Harness, provider and redacted account identities (if applicable):
Prerequisites and fixture:
Steps or exact reproducible commands:
Expected result:
Actual result:
Evidence paths (test logs, redacted transcripts, screenshots/recording):
Live vs fixture coverage:
Known limitations and remaining platform/account combinations:
Blocker, attempted alternatives and next action (if blocked):
```

Use repository-relative artifact links. Keep logs concise and redact secrets, personal
account information, and unrelated private source before committing. Do not store auth
homes, browser cookies, tokens or raw credential stores. Evidence may identify accounts
as A/B provided the live verifier establishes they are distinct subscription identities.

An implementation commit is the revision tested, not necessarily the later documentation
commit recording the result. Re-run affected checks when behavior changes, and reopen
criteria on regressions. Keep README's verified count synchronized in the same update.

## Initial prerequisites and unresolved feasibility

- Two distinct ChatGPT subscriptions available for live concurrent and isolation tests.
- Supported account sign-in access for Claude Code and OpenCode.
- A macOS and a Linux environment with a real VS Code UI for packaged-flow verification.
- Harness-native child event and deeper-delegation probes; capabilities not yet tested.
- Devin's account-only integration path remains unresolved; documented API access uses tokens.

These are prerequisites for future implementation testing, not failures observed in a
running Overseer product. No product exists yet.
