# RFC: Overseer-managed Claude credentials

Status: proposed (owner request, 2026-09-25). Tracked by
[AC-53](../overseer-rfc.md#gate-i--future-owner-requests-2026-09-25) (fixed Claude accounts).
Nothing here has been run against a real Claude login yet; the owner asked to design it first.

## Problem

Codex keeps each account's login in a file (`<account>/codex/auth.json`), so Overseer gets
isolation for free: one folder per account, `CODEX_HOME` points at it, and nothing else is
shared. Verified live with two ChatGPT accounts (AC-12, AC-13).

Claude Code on macOS keeps its OAuth login in the **macOS Keychain**, not in its config folder.
Overseer points each Claude account at its own `CLAUDE_CONFIG_DIR`, but that alone does not prove
the Keychain entries are separate. If two config folders shared one Keychain item, signing a
test account in or out could replace or remove the desktop Claude login, and with it the login
of agents that are running. That risk is why fixed Claude accounts (AC-53) are not tested yet.

## Goal

Each Overseer Claude account owns its credentials the way a Codex account does: separate from
the desktop login, removable without side effects, and never stored in Overseer's database,
events or logs. Account login only: no API keys (the RFC's standing rule).

## Options

1. **Confirm Claude's own per-folder Keychain entries.** Recent Claude Code versions appear to
   name the Keychain item after the config folder when `CLAUDE_CONFIG_DIR` is set. If that holds
   for the installed version, no new storage is needed; Overseer only has to check it.
   - Check (read-only, attributes only, never the secret): list generic-password items whose
     service starts with `Claude Code-credentials` before and after a test sign-in; each
     account must add its own item and leave the desktop item unchanged.
   - Overseer's `profile.status` would report the item name (not the secret) so the Accounts view
     can show "credentials: own Keychain entry" or warn "shares the desktop login's entry".

2. **Overseer-managed credential store ("mock keychain").** Overseer keeps each account's Claude
   login in storage it controls, and hands it to Claude only for that account's runs.
   - Where: an Overseer-owned Keychain item per account (service `Overseer — <account id>`), or a
     0600 file in the account folder, mirroring Codex's `auth.json`.
   - How Claude reads it: either Claude's own file-based credential mode, if the installed version
     can be pointed at a file (Linux builds use `<config>/.credentials.json`), or an environment
     variable Claude accepts for an OAuth session.
   - The long-lived token that `claude setup-token` issues for a subscription is an account login,
     not an API key, but it is a long-lived personal token. The standing "no personal access
     tokens" rule needs an owner decision before Overseer stores one.
   - Sign-in stays Claude's own flow (`claude auth login`); Overseer only moves or references the
     result. Sign-out deletes Overseer's copy and runs Claude's logout for that account only.

3. **Separate macOS user or custom keychain file per account.** Strong isolation, but the default
   keychain search list is per macOS user rather than per process, so this adds setup and
   friction. Rejected unless 1 and 2 both fail.

## Recommendation

Do option 1 first; it may already be true and costs nothing. If the entries are shared, or the
installed version does not separate them, implement option 2 with an Overseer-owned Keychain item
per account and the least-privileged way Claude accepts to read it. Decide on `setup-token`
explicitly before using it.

## Acceptance

AC-53 in the main RFC is the acceptance criterion. With a second Claude account, its Verify
clause covers sign-in, a run, sign-out and sign-in again while a run on the desktop login keeps
working, with both identities and their credential entries staying separate. Option 1's
before/after Keychain attribute check, or option 2's store, is the evidence that the
credentials stay separate. No credential may appear in Overseer's database, events or logs.
