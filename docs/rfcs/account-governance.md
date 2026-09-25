# RFC: Simple account governance

Status: proposed (owner request, 2026-09-25). Tracked by
[AC-46](../overseer-rfc.md#gate-i--future-owner-requests-2026-09-25) in the main RFC.

## Problem

Today Overseer has *profiles*, one per harness. Each is either an "existing login" (the
harness's default credential folder, which may be shared with a desktop app and switch
accounts underneath you) or an isolated credential folder. That works, but it is organised
around harnesses instead of the thing people actually own: accounts. Adding the same ChatGPT
account for `codex` and `codex-app` should not be two decisions. It should also be obvious
which account a run will bill to.

## Goal

An ultra-simple account list:

1. **Add an account**: choose a provider (Claude / Anthropic, OpenAI / ChatGPT, Devin once it
   has an account login), give it a name, and sign in with that provider's own login flow.
2. **See its state**: signed in or not, plan, and a short identity fingerprint (never the
   email or token), plus the last time it was used.
3. **Pick it for a task**: New Task first chooses the harness, then shows only the accounts
   compatible with that harness, as tiles.
4. **Re-sign in or remove it**, affecting only that account.

## Model

| Concept | Meaning |
| --- | --- |
| Provider | Who issues the login: `anthropic`, `openai`, `devin`, … |
| Account | A named provider login owned by Overseer: its own credential folder (0700), identity fingerprint, plan, status. |
| Harness compatibility | Static map of harness to accepted providers: `claude` → anthropic; `codex`, `codex-app` → openai; `opencode` → anthropic or openai (through OpenCode's provider login) and local providers; `devin` → devin; `generic` → none. |
| Linked desktop login | An account entry that points at a harness's default folder (e.g. `~/.codex`, `~/.claude`) is labeled **follows <app>**. It is never logged out by Overseer, and the task picker warns that it can change when the app switches accounts. |

One account can serve several harnesses: the daemon derives each harness's credential
folder from the account (`CODEX_HOME=<account>/codex`, `CLAUDE_CONFIG_DIR=<account>/claude`,
`XDG_DATA_HOME=<account>/opencode-data`). Signing in once per harness family remains
possible where a harness keeps its own separate store. The UI makes that a visible "sign in
for <harness>" step on the account, not a separate account.

## Rules (carried over and made explicit)

- Account login only. No API keys or personal access tokens, and no API-key fallback. An
  account whose harness reports an API-key login shows as *not usable*.
- Credentials never enter Overseer's database or events. Only a one-way identity
  fingerprint, the plan and timestamps are stored.
- Removing or signing out an account touches only its own folder. Desktop logins are
  read-only to Overseer.
- A run records the account it used. Switching the desktop app's account never changes a
  fixed account.

## UI sketch

- **Accounts** view: one row per account, with a provider icon, name, plan, status dot,
  "follows ChatGPT app" badge where applicable, and inline Sign in / Re-sign in / Remove.
- **New Task**: harness tiles, then compatible account tiles (disabled tiles explain why,
  e.g. "not signed in", "needs an OpenAI account"), then workspace and prompt. Styling is
  covered by AC-47.

## Migration

Existing isolated profiles become accounts of the matching provider. The system profiles
(`system-codex`, `system-claude`, `system-opencode`) become linked desktop logins. Run
history keeps its profile ids, mapped to the new account ids.

## Out of scope

Quota-aware routing between accounts, automatic failover, shared team accounts and billing
reports remain later work (see the main RFC's later roadmap).

## Acceptance

AC-46 in the main RFC is the acceptance criterion. Its Verify clause covers:

- adding an account per available provider through the UI;
- only compatible accounts offered per harness;
- the desktop-linked versus fixed account behavior when the desktop app switches accounts;
- re-sign-in and removal isolation.
