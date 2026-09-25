# Verification ledger

No product acceptance criterion has been implemented or verified in Overseer.
The RFC reconstruction and publication are documentation work, not product test evidence.

The authoritative checkboxes are in [the RFC](../overseer-rfc.md). Create an evidence
record named `AC-NN.md` when work starts; until then an AC is `not started`, except explicitly deferred AC-41 (Linux), which
is `blocked` by the unavailable environment. All checkboxes remain open.
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

- The owner has two OpenAI accounts; agent access/login and distinct subscription identity verification remain unproven. Use tiny hello-world-style prompts only.
- Codex and Claude Code require live account-login integration. OpenCode AC-14 permits mock responses or a very small Qwen Coder via Ollama, clearly labeled.
- Verify the real VS Code UI on macOS. No Linux environment is available: AC-41 is deferred/blocked and stays unchecked.
- Native child probes remain untested. Keep limited harnesses usable while retaining the unverified child AC.
- Skip Devin if no account-login path is available; do not introduce keys or personal access tokens.
- The implementer may check ACs with reproducible evidence; a separate review follows later.
- Sustained/volume testing uses fixtures, not paid prompts. Do not launch any tests while planning; explicit start confirmation is required.

These are prerequisites for future testing, not failures observed in a running product.
No product exists yet. All 41 criteria remain unchecked.
