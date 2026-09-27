# Prepared goal: implement the phone remote on the same network

Status: not activated. This file is a reusable instruction for a future implementation session.
Scope: [phone remote RFC](phone-remote.md) and AC-115 to AC-137 (Gate N) in the
[main RFC](../overseer-rfc.md#gate-n--phone-remote-on-the-same-network-added-by-the-owner-2026-09-26).

## Goal text

> Implement Gate N, the phone remote on the same network, against AC-115 to AC-137 in
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
> The phone talks only to the Overseer daemon. Do not build a relay, a server, or anything that
> runs agents away from the Mac; those belong to a later RFC. Do not change the Unix socket
> boundary (AC-08).
>
> Work in this order, and do not start a phase before the one above it has evidence:
>
> 1. **Prove (AC-115).** Measure both app stacks on the owner's iPhone and choose by the numbers,
>    unless the owner has named the stack. Run the other spikes and write the reuse decision. If a
>    spike fails, revise the RFC's proposed default and record why before building on it.
> 2. **Connect (AC-116 to AC-120, AC-134).** Gateway, pairing, encryption, devices, discovery,
>    with a minimal app built on the platform layer and the generated protocol types.
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
> 8. **Confirm (AC-132, AC-133).** Regression coverage, then the owner's session.
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
> Steps that need the owner (scanning the code on the iPhone, the local network permission, the
> push key, Face ID, the final session): prepare everything first, ask one precise question, and
> continue independent work while waiting. Never block on a foreground wait.
>
> Build the implementation in its own worktree and pull request. Criteria, records and RFC
> revisions go to `main`. Keep the license and notice of anything adopted from another project.
> Make no purchases, change no logins on the owner's behalf, and publish no app to a store.
>
> Never weaken or delete a criterion to finish, and never record evidence that was not produced.
> When what remains needs the owner, a device or a product decision, preserve the work and report
> that exact blocker. A partial milestone is progress, not completion.
>
> Done means: AC-115 to AC-137 are verified with reproducible evidence, except AC-128 while AC-107
> does not exist; the existing suites pass with phone access off and on; the README's capability
> table and limits are current; and the owner has confirmed the session on their iPhone.

## What the owner provides

| Item | Needed for | When |
| --- | --- | --- |
| The iPhone, on the same network as the Mac | AC-115, AC-117, AC-120, AC-129, AC-133, AC-135, AC-136 | From phase 1 |
| Apple Developer team and signing for a development build | Installing on the iPhone | Phase 1 |
| A push key from the Apple Developer account, stored in the Mac's Keychain | AC-115, AC-129 | Phase 1, then phase 5 |
| Answers to the RFC's open questions, or acceptance of the recommendations | Every phase | Before phase 2 |
| Marks on the door and the transitions, on a review page | AC-136, AC-137 | Phase 5 |
| The session budget for the implementing agent | Activation | At activation |

## Activation boundary

Writing this document is planning. No goal, scheduled task, implementation, live test or app build
has been started. The owner activates the goal by starting a session with the goal text above and
the session's limits. Until the open questions are answered, the recommendations in the RFC stand
as proposed defaults, not as the owner's decisions.
