# Prepared goal: implement the phone remote on the same network

Status: not activated. This file holds the goal for the first pull request and the full instruction behind it.
Scope: [phone remote RFC](phone-remote.md) and AC-115 to AC-137 and AC-141 (Gate N) in the
[main RFC](../overseer-rfc.md#gate-n--phone-remote-on-the-same-network-added-by-the-owner-2026-09-26).

## Goal for the first pull request

This is the goal to activate. It is the simulator milestone, 22 of the gate's 24 criteria. It is
3596 characters, under the 4,000 limit, and stands on its own.

```text
GOAL: Build the simulator milestone of Gate N, Overseer's phone remote on the same network, in one draft pull request from this worktree.

READ FIRST: docs/rfcs/phone-remote.md (design), docs/rfcs/phone-remote-goal.md (full instruction), and Gate N in docs/overseer-rfc.md (criteria with their Verify clauses).

WHAT TO BUILD
- In overseerd (Rust): a gateway for phones, off by default and switched on the desktop only; pairing started on the Mac; an encrypted, mutually authenticated session; a device list with scopes and revoke; resume from the event cursor; exactly-once requests; daemon methods for files, review marks and pull requests; a push sender with switches.
- In phone/ (Expo, TypeScript): one app for iOS and Android that lists every agent, shows each conversation live, and controls everything VS Code controls. Light and dark themes generated from extension/design/tokens.js, following the system setting. A platform layer with one generic interface per capability. The door on a cold start, and one motion system.
- In the extension and the terminal UI: the phone access switch, Pair a Phone, and the Devices list.

ORDER (do not start a phase before the one above it has evidence)
1. Prove: spikes and speed baselines on both simulators (AC-115).
2. Connect: AC-116 to AC-120, AC-134, AC-141.
3. Hold: AC-121 to AC-123.
4. See and control: AC-124 to AC-127, AC-131.
5. Feel: AC-135 to AC-137.
6. Needs you, safely: AC-129, AC-130.
7. Confirm: AC-132.

DONE WHEN ALL OF THESE HOLD
1. Verified, with evidence records written through records.py and boxes checked: AC-116, AC-118, AC-119, AC-121, AC-122, AC-123, AC-124, AC-125, AC-126, AC-127, AC-130, AC-131, AC-132, AC-134, AC-141.
2. Verified on the iOS simulator and the Android emulator and recorded as partial, with the iPhone part named as the owner's next step: AC-115, AC-117, AC-120, AC-129, AC-135.
3. AC-136 and AC-137 verified except for the owner's marks: a review page is published and the owner has been asked.
4. AC-128 and AC-133 left not started with their blockers recorded (AC-107; the owner's iPhone).
5. cargo test --workspace and the existing packaged-UI suites pass with phone access off and with it on.
6. The pull request is marked ready. Its report lists what is verified, what is partial, and the steps the owner still has to do.

RULES
- The phone talks only to the Overseer daemon. No relay, no server, no cloud account.
- Pair once. The app never asks to pair, sign in or confirm again, and it does not lock itself.
- Do not change the Unix socket boundary (AC-08).
- Rust on the phone only if a measurement shows it is needed.
- Do not read, use or change Apple assets or Android virtual devices that belong to other projects. Create Overseer's own.
- Test against an isolated OVERSEER_HOME, never the owner's running daemon.
- Live agent turns follow the owner's paid-turn rules: tiny prompts, one attempt per step.
- Never accept a license, enter a password or change a system setting for the owner.
- Keep the pull request a draft that says work continues until done. Never push to another agent's branch. Criteria, records and RFC revisions go to main.
- Never weaken or delete a criterion, and never record evidence that was not produced. A partial milestone is progress, not completion.
- When only the owner can unblock a step, ask one precise question and continue with the rest. Never wait in a foreground loop.
- If a spike fails, revise the RFC and record why before building on it.

BUDGET: eight hours for this session. If it ends first, commit, push, and report exactly what remains.
```

| Group | Criteria |
| --- | --- |
| Verified in full on the simulators (15) | AC-116, AC-118, AC-119, AC-121, AC-122, AC-123, AC-124, AC-125, AC-126, AC-127, AC-130, AC-131, AC-132, AC-134, AC-141 |
| Simulator part now, iPhone part later (5) | AC-115, AC-117, AC-120, AC-129, AC-135 |
| Waiting for the owner's marks (2) | AC-136, AC-137 |
| Outside this pull request (2) | AC-128, AC-133 |

## Full instruction

The goal above points here. This is the detail the implementing agent follows.

> Implement Gate N, the phone remote on the same network, against AC-115 to AC-137 and AC-141 in
> `docs/overseer-rfc.md`, following `docs/rfcs/phone-remote.md`. Deliver a gateway inside
> `overseerd` that is off by default, pairing that needs the Mac, an encrypted and mutually
> authenticated session, and one phone app for iOS and Android in `phone/` that sees and controls
> every agent and every Overseer system without ever losing the session. The app is hyper fast,
> opens through the door, and moves well throughout.
>
> Use the best option that is not slow for every part, chosen by measurement. Do not force Rust
> onto the phone: use it only where it removes a second implementation of security code or where
> a measurement shows it is needed. Keep everything that differs between iOS and Android in the
> platform layer, behind generic interfaces, and prefer a maintained cross-platform library to
> own native code.
>
> The app follows the phone's light or dark setting and looks the same as Overseer Light and
> Overseer Dark in VS Code. Generate its tokens from `extension/design/tokens.js`; never copy
> values by hand. The door appears on a cold start only.
>
> Build and verify on the iOS simulator and the Android emulator first, with Expo as the stack.
> The parts of a criterion that need the owner's iPhone stay unchecked, named in its record, until
> the owner has done the device steps in the RFC. A simulator never satisfies a device check. End
> the session's report with the steps the owner still has to do.
>
> The Apple developer assets already on this Mac (identifiers, keys, certificates, profiles) and
> the Android virtual devices already there belong to other projects of the owner's. Do not read,
> use or change them. Overseer gets its own identifier, its own keys and its own virtual device.
>
> The owner pairs once. After that the app never asks to pair, sign in or confirm, and it does not
> lock itself. Phone access is switched on and off on the desktop only. Notifications can be
> switched on and off. There is no voice mode.
>
> The phone talks only to the Overseer daemon. Do not build a relay, a server, or anything that
> runs agents away from the Mac; those belong to a later RFC. Do not change the Unix socket
> boundary (AC-08).
>
> Work in this order, and do not start a phase before the one above it has evidence:
>
> 1. **Prove (AC-115).** Run the spikes on the simulators and record the speed baselines. Write
>    the reuse decision. If a spike fails, revise the RFC and record why before building on it.
> 2. **Connect (AC-116 to AC-120, AC-134, AC-141).** Gateway with its desktop switch, pairing
>    once, encryption, devices, discovery, with a minimal app built on the platform layer and the
>    generated protocol types.
> 3. **Hold (AC-121 to AC-123).** Resume from the cursor, exactly-once requests, an awake Mac.
> 4. **See and control (AC-124 to AC-127, AC-131).** Agents, conversations, control, review and
>    the rest of Overseer. Move file reading, reviewed marks and pull request creation behind
>    daemon methods, and make VS Code use them.
> 5. **Feel (AC-135 to AC-137).** The speed budget, the door, and motion throughout. Keep the
>    budget green from here on: a change that breaks it is not finished.
> 6. **Needs you, safely (AC-129, AC-130).** Push notifications, confirmations, the security
>    review and the fuzz test.
> 7. **Overseer itself (AC-128).** Only when AC-107 exists; otherwise leave it not started with
>    that blocker recorded, and continue.
> 8. **Confirm on the simulators (AC-132).** Regression coverage. This completes the simulator
>    milestone.
> 9. **The real iPhone.** After the owner's device steps: measure the speed budget first and
>    confirm or change the stack, then discovery, push, and the owner's session (AC-133).
>
> For each criterion: reproduce what is missing with a focused test or scenario, implement a
> bounded change, run the relevant checks and the regressions for what the change touches, and
> write the evidence record in `docs/verification/AC-NNN.md` through `records.py`. Check a box
> only when the whole criterion is verified with evidence. A change that invalidates earlier
> evidence reopens the affected boxes.
>
> Use fixture harnesses for streams, disconnects, retries, permissions and load. Live turns follow
> the owner's paid-turn rules: tiny prompts, one attempt per step, no retry loops. Fixture evidence
> never satisfies a criterion that asks for the owner's iPhone.
>
> Steps that need the owner (accepting the Xcode license, and later signing, the push key,
> scanning the code and the final session): prepare everything first, ask one precise question,
> and continue independent work while waiting. Never block on a foreground wait. Never accept a
> license, enter a password or change a system setting on the owner's behalf.
>
> Other gates are being built at the same time. Before changing shared daemon code, fetch `main`
> and build on what is there; define the file, review and pull request methods once and let other
> surfaces use them. Gate M is ignored for now.
>
> Open the pull request as a draft and say in it that work continues; mark it ready only when the
> milestone is done, so the monitor that merges finished work (AC-146) leaves it alone. Never push
> to another agent's pull request. Add the phone's tests to the one command that runs every test
> (AC-147) when it exists. The door and the app icon use Overseer's mark from AC-142; until its
> files exist, read the current mark from one source file so the swap is one change.
>
> Build the implementation in its own worktree and pull request. Criteria, records and RFC
> revisions go to `main`. Keep the license and notice of anything adopted from another project.
> Make no purchases, change no logins on the owner's behalf, and publish no app to a store.
>
> Never weaken or delete a criterion to finish, and never record evidence that was not produced.
> When what remains needs the owner, a device or a product decision, preserve the work and report
> that exact blocker. A partial milestone is progress, not completion.
>
> The simulator milestone is done when every part of AC-115 to AC-137 and AC-141 that the
> simulators can verify is verified with reproducible evidence, except AC-128 while AC-107 does
> not exist, and every device part is named in its record with the owner's next step.
>
> Done means: AC-115 to AC-137 and AC-141 are verified with reproducible evidence, except AC-128 while AC-107
> does not exist; the existing suites pass with phone access off and on; the README's capability
> table and limits are current; and the owner has confirmed the session on their iPhone.

## What the owner provides

| Item | Needed for | When |
| --- | --- | --- |
| The Xcode license accepted and the developer tools pointed at Xcode | The iOS simulator | Done on 2026-09-26 |
| The session budget for the implementing agent | Activation | At activation |
| Marks on the door and the transitions, on a review page | AC-136, AC-137 | Phase 5 |
| A new app identifier and a new push key for Overseer, and signing | The real iPhone | Phase 9 |
| The iPhone, on the same network as the Mac | The device parts and AC-133 | Phase 9 |

The exact steps are in the RFC under [Steps for the owner](phone-remote.md#steps-for-the-owner).
The Android emulator needs nothing from the owner.

## Activation boundary

Writing this document is planning. No goal, scheduled task, implementation, live test or app build
has been started. The owner activates the goal by starting a session with the goal text above and
the session's limits. The owner left the remaining choices to the implementing agent on
2026-09-26; the RFC lists them, and the owner can change any of them.
