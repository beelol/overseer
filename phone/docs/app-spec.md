# Overseer for the phone: what the app is

The brief every screen is built from. Criteria: Gate N in `docs/overseer-rfc.md` (AC-115 to AC-137,
AC-141). Design: `docs/rfcs/phone-remote.md`. This file says what each screen holds, what it says,
and how it is named for tests.

## Rules for every screen

- **The phone talks to Overseer only.** Everything shown comes from the daemon through
  `phone/core` (the connection) and `phone/model` (the view models). A screen never computes what
  the model already gives, and never keeps its own copy of the daemon's state.
- **Tokens only.** Colours, type sizes, spacing, radii and durations come from
  `src/theme/tokens.generated.ts` (generated from `extension/design/tokens.js`). Nothing is
  written by hand. Light and dark follow the phone's setting and change while the app is open.
- **No platform checks outside `src/platform/`.** A screen asks a capability.
- **Opening the app never asks for anything.** No sign-in, no lock, no confirmation. The only
  system prompts are the ones the system itself shows (notifications, camera, local network), each
  explained in one sentence first.
- **Calm, with little text.** One idea per line. Labels of one to three words. No exclamation
  marks, no jargon. "Phone access", "the Mac", "agent". Never "gateway", "daemon", "session",
  "handshake" or "socket".
- **Fast.** Open on what is stored. Show what the person did at once (a sent message appears
  before the Mac answers). Lists build only visible rows. Events are applied in batches once per
  frame. Motion never waits on work.
- **One thumb.** What is used most sits within reach of a thumb at the bottom.
- **Every control has a name.** Icon-only controls carry an accessibility label. Text follows the
  system size up to the largest standard size without clipping. Portrait and landscape.
- **Every control has a test id**, `screen.part` in lower-case with dots (`agents.filter.needs`,
  `agent.composer.send`). Rows carry their id (`agents.row.<run id>`).
- **Destructive actions name what will be lost** and ask once.

## Connection line

One quiet line under the header, on every screen that shows live data. Hidden while connected.

| State (`phone/core`) | Text | Test id |
| --- | --- | --- |
| `connecting`, `reconnecting` | Reconnecting… | `connection.reconnecting` |
| `unreachable` | Mac unreachable · last contact 2 min ago | `connection.unreachable` |
| `off` | Phone access is off on the Mac | `connection.off` |
| `revoked` | This phone was removed on the Mac | `connection.revoked` |

While not connected the screen shows what is stored, marked with its age, and never as live.
Controls that change something stay usable: what is sent is queued, shown as queued, and sent once.

## Screens

| Route | Screen | Holds |
| --- | --- | --- |
| (launch) | The door | AC-136. Opens onto Agents, or onto Pair the first time. |
| `/pair` | Pair with your Mac | Scan the code, or type it. The phone's name. Then "Confirm on your Mac". |
| `/agents` | Agents | Every agent, children under parents, those that need you first. Filter, search. |
| `/agent/[run]` | Conversation | The agent's conversation, live. Permission requests. The composer. |
| `/agent/[run]/changes` | Changes | Changed files against a comparison. |
| `/agent/[run]/file` | A file's changes | Hunks, with Accept and Reject. |
| `/agent/[run]/merge` | Merge back | The plan, conflicts, Complete, Abort. |
| `/agent/[run]/pr` | Pull request | The plan, title, Open. |
| `/new` | New agent | Repository, agent, account, options, task. |
| `/accounts` | Accounts | Sign-in state, plan, usage. Sign in with a code. |
| `/settings` | Settings | Notifications, this phone, the Mac, safety, forget this Mac. |

### Pair with your Mac (`/pair`)

1. "On the Mac, open Overseer and choose **Pair a Phone**." 2. "Scan the code."
- The camera fills the upper part when the phone has one and allowed it. **Type the code** opens a
  field that takes a pasted or typed code (`pair.code`), always there on simulators.
- The phone's name (`pair.name`), filled in from the device, editable.
- After a code is taken: **Confirm on your Mac**, with the Mac's name. It waits up to a minute.
- When it did not work: "This code did not work. A code works once, for two minutes. Get a new one
  on the Mac." (`pair.error`). When the Mac did not answer: "Could not reach the Mac. Check that
  the phone is on the same network and phone access is on."
- Paired: straight to Agents. Never shown again unless the Mac removed this phone.

### Agents (`/agents`)

- Header: **Agents**; search (`agents.search`); menu (`agents.menu`): New agent, Accounts,
  Settings, Stop all agents.
- Filters (`agents.filter.all`, `.active`, `.needs`): All, Active, Needs you with its count.
- A row: the provider's logo, the title on one line, under it the repository, the status in words
  and the time ("shop · Needs you · 2 min"). Children are indented under their parent with a
  line. A row that needs you carries a mark.
- Swipe a row: Archive. Long press: Pin, Archive, Stop.
- Pull down to load again.
- Nothing yet: "No agents yet." and **New agent**.
- **New agent** is the button at the bottom (`agents.new`).

### Conversation (`/agent/[run]`)

- Header: back, the title on one line, the status in words. **Changes** with the count of changed
  files (`agent.changes`). More (`agent.more`): Stop, Merge back, Pull request, Clean up, Archive.
- The conversation, newest at the bottom, as `phone/model` gives it: your messages as bubbles, the
  agent's text as formatted text, tool steps as one line each and folded when they follow each
  other ("6 steps · Read · Searched …", tap to unfold), edits as chips that open the file's
  changes at that hunk, errors, native children nested under the step that started them, a quiet
  line when a turn ends (status, time, tokens, cost).
- A permission request is a card: what the agent wants to do and on what, **Allow** and **Deny**
  (`agent.permission.allow`, `.deny`). Deny can carry a sentence. Once answered the card says
  by whom ("Allowed on the Mac"). Answered elsewhere first: the card updates by itself.
- The composer (`agent.composer.text`): several lines, grows to five. Under it, chips for model,
  effort and permission mode when the agent supports them. Attach a photo or an image
  (`agent.composer.attach`). **Send** (`agent.composer.send`). While the agent works, Send queues
  the message and **Stop** (`agent.composer.stop`) stops the turn.
- A queued message is a bubble marked "Queued", then "Sending", then it is the message. Never twice.

### Changes (`/agent/[run]/changes`) and a file's changes (`/agent/[run]/file`)

- The comparison (`changes.comparison`): Latest run, Since task start, Base, A branch…
- A file row: status letter, the path with the file name strong, added and removed counts.
- A file: its hunks. Each hunk: "Lines 12 to 14", removed lines, added lines, syntax colouring,
  line numbers. Wrap long lines (`file.wrap`) or scroll sideways.
- **Accept** marks a hunk reviewed (`file.hunk.accept`); the mark shows in VS Code too.
  **Reject** puts the lines back (`file.hunk.reject`): "Put these 3 lines back?" with **Put back**.
- A file that cannot be shown says why: a binary file, larger than 2 MB, a link.
- The changes refresh while the agent edits.

### New agent (`/new`)

Repository (the ones Overseer knows), agent (the installed ones), account (signed-in ones first;
a signed-out one says so and offers Sign in), model, effort and permission mode where the agent
has them, where it works (a new worktree, or the current checkout), the task. **Start**
(`new.start`). What was chosen last time is chosen again.

### Accounts (`/accounts`)

By provider. Each account: name, signed in or not, plan, usage with the time it resets.
**Sign in** with a code: the code large, **Open the sign-in page**, **Copy the code**; it finishes
by itself. A provider without a code: "Sign in on the Mac."

### Settings (`/settings`)

- **Notifications**: all (`settings.notifications.all`), then each kind: Permission requests,
  Questions, Errors, Finished. Show text in notifications (off by default). When the Mac turned
  them off for every phone, it says so.
- **This phone**: its name, what it may do (Full control or Watch only), paired since.
- **The Mac**: its name, last contact, the fingerprint of its key.
- **Appearance**: "Follows your phone."
- **Safety**: App lock (off), Ask for unlock before changes that cannot be undone (off).
- **Forget this Mac**: "Forget this Mac? You will pair again to use Overseer here."

## What a watch-only phone sees

Everything, and no control that changes something: the composer, Allow and Deny, Accept and
Reject, Start and the actions are not shown. A line says "This phone may watch. Change it on the Mac."

## Notifications

A notification names the agent ("Claude · shop") and the moment ("Needs your permission"). Tapping
opens that agent. A permission request offers Allow and Deny on the notification, after unlock.
