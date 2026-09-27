# AC-202 — the orchestration session (owner-confirmed): the steps

The criterion: with four agents working in one repository in different roles, the owner rallies
them from the conversation, resolves a conflict from its card, holds one agent and releases it,
changes another's direction, sees a check-in catch an agent that drifted, sets one agent to watch
another and reads its finding, puts Overseer on Auto and sees it settle a conflict by itself,
stops all of them at once, and asks from the phone what everyone is doing. Verified by the owner's
dated confirmation with a screenshot of each step and the friction log.

Everything below runs on the owner's own daemon and accounts. Paid turns: Overseer's own turns
(Claude, haiku is enough: set `overseer.chat.model` to `haiku`), one turn per agent per step. A
fixture rehearsal of every step is in `daemon/tests/overseer.rs` and `test/ui/scenario-home.js`.

## Before

1. Build and install the branch's VSIX (`node extension/scripts/package.js`, then install
   `extension/overseer-*.vsix`), or use the Marketplace build once pull request #14 is merged.
2. Open a repository with a few directories (for example `api/`, `web/`, `docs/`, `tests/`).
3. Settings: `overseer.chat.harness` = `claude`, `overseer.chat.model` = `haiku`.

## The steps (one screenshot each)

| # | Do | Expect |
| --- | --- | --- |
| 1 | From home (Overseer: New Agent), start four agents by typing a task and Enter, one per role: "build the login API in api/", "build the login page in web/", "write the docs in docs/", "write the tests in tests/". | Four agents in the side bar; four *Started* cards in the conversation at home. |
| 2 | At home, type `@overseer Rally my agents` and Enter. | Overseer's map: who owns what, overlaps, needs; a proposal to ask the agents whose digests lack an area for a report, with the cost; after Yes, the reports come back and a proposal for the areas; Yes records them (each agent's row shows its area on hover). |
| 3 | Make two agents touch the same lines (tell both, at home with `@`, to edit the same paragraph of `README.md`). | Within 10 s a conflict card in the conversation and *conflict* on both rows and tiles; Overseer needs you in Needs you. Resolve it from the card: *assign* to one; the other gets a guardrail (its chat shows the line). |
| 4 | `@overseer hold <agent> until <another> is done`, Yes. Then `@overseer release <agent>`. | *held* on the row and tile; a message to the held agent waits (its chat says *Release and send*); after the release it goes. |
| 5 | `@overseer tell <agent> to switch to <something else>`. | The redirect card: a snapshot, the turn stopped, the direction as the next turn from Overseer; the review offers *Since the change of direction*. |
| 6 | Tell an agent (at home, `@`) to write a file outside its area. | Within 2 s *outside its area* in its chat; the check-in that follows (every third turn, or set `@overseer check on <agent> every turn`) reads *drifting* with a proposal to redirect at Ask first. |
| 7 | `@overseer watch <agent> for deleted tests` (or `watch.start` from the command palette when it exists), then make the agent delete a test. | The watcher appears (a read-only run), wakes at the subject's turn end, files *stop*; the finding card in the conversation names the watcher; a proposal to hold the subject. With *hold on stop*, the subject is held within 2 s. |
| 8 | Put Overseer on Auto (the level in the conversation header), then make a same-lines conflict again. | Overseer settles it by itself and its card says how (assign or sequence). |
| 9 | `@overseer stop everyone`, Yes. | One card with one row per agent; every agent interrupted within 2 s. |
| 10 | From the phone (pull request #10's client, once on main): ask what everyone is doing. | The same conversation; the answer from the daemon's session; a yes from the phone names the phone as the approver. |

## The friction log

One line per item: what was expected, what happened, how long it took, what was confusing, what
was missing. Kept next to the screenshots in this folder as `friction.md`.

## Confirmation

`confirmed.md` in this folder: the date, the daemon and extension versions, the account used, and
the list of screenshots (`01-started.png` to `10-phone.png`).
