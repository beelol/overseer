# Security review of the phone gateway (Gate N, AC-130)

Reviewed: the daemon's changes on `claude/phone-remote-vscode-control-b48a34` at `b726970`
(`daemon/src/gateway/`, `review.rs`, `device_login.rs`, `pr.rs`, and the changes in `server.rs`,
`daemon.rs`, `store.rs`, `background.rs`), by a reviewer that had not written the code, read-only.
Date: 2026-09-26. Every finding is fixed and has a test that fails without the fix.

## Who was assumed to attack

1. Someone on the same network without the pairing code.
2. A paired phone that may only watch.
3. A paired phone with full control, reaching for what stays on the Mac.
4. A repository with hostile content (names, links, contents).
5. Two surfaces acting at the same moment.

Nothing was found that the first attacker can reach: the handshakes, the counter against replay,
pairing and revoking held.

## Findings and what was done

| # | Finding | Severity | Attacker | Fixed in | Test |
| --- | --- | --- | --- | --- | --- |
| 1 | A phone could pass `extra_args`, `args` or `approval_policy` to `task.create`. The guard only refused `harness: generic` and `program`. Extra arguments reach the harness's command line, so a phone could start a program on the Mac and switch off permission requests. | High | 3 | `gateway/classes.rs` `undeclared_params`, `gateway/remote.rs` | `ac130_a_phone_sends_only_what_the_protocol_describes`, `every_method_a_phone_may_call_describes_its_parameters` |
| 2 | `.git` was refused by exact spelling. macOS volumes ignore case, so `.GIT/config` opened the repository's configuration, to any paired phone, and `review.reject` could remove files there. | Medium | 2, 3 | `review.rs` `is_git_name`, `inside`; `files.rs` | `ac130_the_git_folder_is_closed_in_every_spelling`, `paths_and_comparisons_are_confined` |
| 3 | `review.reject` wrote through a temporary file whose name held the daemon's process id. A link with that name, put there by a repository or an agent, would have aimed the write outside the workspace. | Medium | 4 | `review.rs` `write_inside` | `ac130_rejecting_a_hunk_never_writes_through_a_link` |

How each was fixed:

1. **Allow, not refuse.** A request from a phone may hold only the parameters the protocol
   describes for its method (`protocol/protocol.json`). Anything else is refused with
   `invalid_params`, before the method runs. Every method a phone may call must describe its
   parameters field by field, or the test suite fails. The Mac's own surfaces are unchanged.
2. **Any spelling.** A name is `.git` whatever its case, with trailing dots or spaces, with
   characters the file system ignores, or as `GIT~1`. The resolved path is checked again after
   links are followed. The same rule applies to the folder listing.
3. **A file nobody can aim.** The temporary file has a random name, must not exist, is never
   opened through a link, and is created only after the folder was checked. Folders are checked
   before any is made.

## Weaknesses found on the way, below the bar of a finding, fixed too

| What | Done |
| --- | --- |
| A phone could start a device-code sign-in on a desktop-linked login and replace it | Refused, like sign-out and removal (`ac130_a_desktop_login_and_a_pairing_are_not_a_phones_to_replace`) |
| The plan of a pull request gave the remote's address as configured, with any token written into it | The user part is removed (`parses_github_remotes_only`, and the `.git` test) |
| A pairing request that ended also closed a pairing opened after it | It closes only its own |
| A phone removed while it was away never learned it and kept trying | It is told inside a handshake only its key completes (`ac119`, `ac141`, and the app's own test against the real gateway) |
| A comment said rejecting a hunk is refused while an agent writes; the code never did that | The comment says what the code does |

## Looked at and found sound

Forged and replayed session handshakes; pairing without the code, its single use and its
confirmation; the bytes outside the Noise message; a phone removed or phone access switched off
between the handshake and the session; scope changes during a session; the length of the key
fingerprint (shown and used to find the Mac, never to decide); nonces, frame order and joining;
no answer before a valid handshake; the test-only switches (each can only shorten a wait or
refuse more); method classes and the second check in the Mac-only methods; the acting device
left set after a failure (fails closed); what a watch-only phone can cause; the repository check;
follow-ups; permission answers; accounts; request ids per device; `..`, absolute paths and NUL;
links as the last part of a path and on the way; comparisons that start with `-` or hold `..`;
every program the new code starts (`git`, `gh`, `dns-sd`, `xcrun simctl`, the harness's sign-in,
`ps`) and what reaches its arguments; SQL (every statement has parameters); what events and
replies show of other devices (no keys, no push tokens); the pairing secret (only in the reply to
the Mac); the device-code sign-in (the address and the code reach the phone, no credential does).
