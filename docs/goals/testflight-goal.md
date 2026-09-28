# Goal: ship the Overseer phone app to TestFlight (iOS)

Prepared 2026-09-27 by the owner. Scope: get a real TestFlight build of the Overseer phone app
(`phone/`, Gate N — [phone-remote RFC](../rfcs/phone-remote.md)) onto the owner's iPhone, built from
the **latest** app source, wearing the **owner's brand marks** ([AC-142](../overseer-rfc.md),
[brand.md](../design/brand.md)). iOS is the target; Android is done only if it is cheap and safe.

**The one hard rule: touch nothing but Overseer.** The Apple account (team STATION 42 INTERACTIVE
L.L.C., team id `FQ6YGD7554`) also holds `aquafriends` (`io.station42.aquafriends`). Never create,
change, revoke or delete another app's identifier, App Store Connect record, key, certificate,
provisioning profile or device — read them only to avoid a collision. Signing and simulators/
emulators are Overseer's own, never another project's (see [apple-assets rule](../../AGENTS.md)).

## Already done this session (2026-09-27)
- **TF-1** App ID registered: **Overseer** → `com.beelol.overseer.phone`, Explicit, **Push
  Notifications** enabled. `aquafriends` left untouched.
- **TF-2** App Store Connect record created: **Overseer Remote** (iOS), bound to
  `com.beelol.overseer.phone`, SKU `overseer-phone`, English (U.S.), Full Access. The listing name
  "Overseer" was taken on the App Store, so the *listing* is **Overseer Remote** (owner's pick); the
  home-screen/app name stays **Overseer** (from the binary, `app.config.ts`). `One More Fish` and
  `aquafriends` untouched.
- **TF-3** Agreements: **Free Apps Agreement is Active** — enough for TestFlight. The **Paid Apps
  Agreement is unsigned and intentionally left so** (only needed to sell / do IAP). The EU DSA
  trader-status prompt is for EU App Store distribution, not TestFlight.
- Keys list is empty — no APNs auth key exists. The push key is **deferred**: it is not needed for
  TestFlight and cannot be used until the daemon's push sender (PR #10) is merged. Create it later,
  as part of the real-iPhone push step, not this goal.
- Team: **STATION 42 INTERACTIVE L.L.C.**, team id `FQ6YGD7554`.
- **TF-4** marks: iOS assets regenerated from `docs/design/brand/`; `assets/icon.png` is the real
  Overseer mark, 1024² with no alpha (verified visually). **TF-5** release config: export-compliance
  flag `ITSAppUsesNonExemptEncryption=false` set in `app.config.ts`; bundle id scoped to Overseer.
- **TF-6** signed `.ipa`: built from the latest phone tip (`95b0946d`) in an isolated worktree,
  archived + exported with **manual** distribution signing — App Store profile "Overseer App Store"
  (`com.beelol.overseer.phone`) + the STATION 42 Apple Distribution cert. `altool` validation:
  VERIFY SUCCEEDED, no errors.
- **TF-7** upload: `altool --upload-app` → UPLOAD SUCCEEDED (Delivery UUID
  `2da48d08-b8d2-4c4b-b7dd-f94b702690a6`). Build **0.1.0 (1)** shows in TestFlight, Processing.
  Export compliance answered in the binary (no ASC prompt).
- **TF-8** setup: internal group **"Overseer Internal"** created (auto-distribution on), owner
  (`bilalitani1@gmail.com`) added as internal tester. Waiting on Apple processing → then owner
  installs on the iPhone.
- Signing note: the App Manager API key can't do cloud signing (Apple requires Admin), so a manual
  Overseer-scoped App Store profile was used with the existing team distribution cert. The API key
  (`VAT46LATWS`, App Manager) is used for upload only. `.p8` and profile live under `/private/tmp`,
  never committed. An earlier lost key (`337JA98RL5`) was revoked; aquafriends' assets untouched.

## Paste the block below into `/goal`

```
GOAL: Publish a TestFlight build of the Overseer phone app (phone/) for iOS, built from the latest app source and wearing the owner's brand marks, installable on the owner's iPhone. Follow AGENTS.md.

READ FIRST: docs/rfcs/phone-remote.md ("Steps for the owner"), docs/goals/testflight-goal.md (this goal, with the acceptance criteria), docs/design/brand.md and AC-142/AC-178 in docs/overseer-rfc.md.

ONE HARD RULE — ONLY OVERSEER: the Apple team (STATION 42, FQ6YGD7554) also owns aquafriends (io.station42.aquafriends). Never create, change, revoke or delete any identifier, App Store Connect app, key, certificate, provisioning profile or device that is not Overseer's. Read others only to avoid collisions. Overseer gets its OWN distribution cert/profile scoped to com.beelol.overseer.phone and, for Android, its own keystore. Never reuse another project's signing assets, simulators or emulators.

WHAT TO DO
1. Marks first: generate the iOS app icon, launch screen and door assets from docs/design/brand/ using the phone app's own scripts (gen-icons / gen-assets). App icon = overseer-app-icon.png (1024, no alpha); door + launch = overseer-icon-flat.png (grayscale); in-app mark = overseer-logo.png. No eye glyph or placeholder may remain (AC-178, iOS slice).
2. Release config: add phone/eas.json (or an equivalent local Xcode/Fastlane setup) with an iOS TestFlight profile — autoincrement the build number, version from app.config.ts, credentials scoped to the Overseer App ID only. Declare export compliance in the binary (ios.infoPlist.ITSAppUsesNonExemptEncryption) so the app's Noise/@noble encryption is answered once, not per upload.
3. Build the .ipa from the tip of the latest phone-remote work (PR #10, or main once merged) — never a fork or stale copy — signed with a distribution cert + a provisioning profile for com.beelol.overseer.phone. Validate it before upload.
4. Upload to TestFlight under the Overseer App Store Connect record; wait for "Ready to Test"; confirm export compliance shows no outstanding action.
5. Add the owner to an Internal Testing group so the build reaches the iPhone.
6. Android (only if attainable with no new paid account and no other project's assets): build a signed release from the same source and marks with an Overseer keystore. Otherwise record it not-started with the blocker.

OWNER ACTIONS (only the owner can do — password, Apple account, agreement, 2FA, device). Keep these on the README owner-actions list (AC-160) and keep going on everything else:
- Sign any outstanding Agreements (account-wide) so a build can reach TestFlight.
- Either approve the 2FA/authentication at upload, or create an App Store Connect API key scoped to Overseer and hand it to the build so upload is non-interactive.
- Create/confirm the App Store Connect app record for Overseer (can be done together in the browser).
- Install the TestFlight build on the iPhone and confirm it launches (TF-8).

RULES
- No blocking waits: when only the owner can unblock, add one precise item to the owner-actions list, ask one question, continue elsewhere.
- Secrets (.p8, API keys, keystore) are never committed; store where the implementation says.
- Test against an isolated OVERSEER_HOME; run phone/ "npm run check" (and scripts/test-all if the shared code changed) before asking for a merge; leave no test devices, daemons or builds running.
- Paid turns per AGENTS.md. App-store criteria and the ledger go to main; app changes go in the phone branch/PR.

DONE WHEN TF-2..TF-8 are verified with evidence (screenshots + build logs), the build is "Ready to Test" and installed by the owner, the ledger records are written, the marks are confirmed from docs/design/brand/, and aquafriends is provably unchanged. Android (TF-9) is done or recorded not-started with its blocker.
```

## Acceptance criteria

Provisional ids (TF-n) — fold these into Gate N's series in `docs/overseer-rfc.md` with real AC-NN
numbers **assigned right before pushing** (AC numbers move fast on `main`), each with its Verify
clause, then record them through `docs/verification/records.py`.

- [x] **TF-1 — App identity registered.** An Explicit App ID `com.beelol.overseer.phone` exists
  under team `FQ6YGD7554` with Push Notifications, created new. **Verify:** Identifiers shows
  *Overseer* → `com.beelol.overseer.phone` with Push on; `io.station42.aquafriends` present and
  unchanged. *(Done 2026-09-27.)*
- [x] **TF-2 — App Store Connect record.** An iOS app record bound to `com.beelol.overseer.phone`,
  with an SKU and primary language, created new. **Verify:** App Store Connect › Apps lists it on
  that bundle id; the aquafriends and One More Fish records are untouched. *(Done 2026-09-27:
  "Overseer Remote", SKU `overseer-phone`.)*
- [x] **TF-3 — Agreements active for TestFlight.** The Free Apps Agreement is Active. **Verify:**
  Business › Agreements shows Free Apps Agreement = Active. *(Done 2026-09-27. Paid Apps left
  unsigned by choice — not needed for TestFlight; EU DSA trader status is a separate, non-blocking
  owner item.)*
- [x] **TF-4 — The right marks on iOS.** *(Done 2026-09-27: icon.png 1024² no-alpha, real Overseer mark, verified.)* The home-screen icon, launch screen, door and in-app mark
  are generated from `docs/design/brand/` (app icon `overseer-app-icon.png` at 1024 with no alpha;
  launch + door `overseer-icon-flat.png` in grayscale; in-app `overseer-logo.png`); no placeholder
  or eye glyph remains; icons are script-generated, not hand-drawn (iOS slice of AC-178). **Verify:**
  the generated iOS icon set matches the brand source; simulator screenshots (light and dark) of the
  home icon, launch screen, door and an in-app mark; a repo search finds no placeholder mark.
- [x] **TF-5 — Release build config.** *(Done: ITSAppUsesNonExemptEncryption set; manual App Store profile Overseer-scoped.)* `phone/eas.json` (or an equivalent local Xcode/Fastlane
  config) has an iOS TestFlight profile: autoincrementing build number, version from
  `app.config.ts`, `ITSAppUsesNonExemptEncryption` set, credentials scoped to the Overseer App ID
  only. **Verify:** config present; a dry run resolves `com.beelol.overseer.phone` and a
  distribution profile scoped to it, and references no other project's identifier, key, cert,
  profile, simulator or emulator.
- [x] **TF-6 — Signed .ipa from the latest app.** *(Done: built from phone tip 95b0946d, distribution-signed, altool VERIFY SUCCEEDED.)* An App Store distribution `.ipa` is built from the
  tip of the latest phone-remote source, signed with the team distribution cert and a profile for
  `com.beelol.overseer.phone`. **Verify:** the build log shows the commit, bundle id and profile;
  the `.ipa` passes validation (Xcode Organizer or `xcrun altool`/notary validate).
- [x] **TF-7 — Uploaded to TestFlight.** Uploaded 2026-09-27 (UPLOAD SUCCEEDED); build 0.1.0 (1)
  processed to **Testing** and available to the internal group; export compliance answered in the
  binary (Binary State: Validated, App Uses Non-Exempt Encryption: No).
- [x] **TF-8 — Installed on the iPhone (owner).** Internal group "Overseer Internal", owner as
  tester. **Owner confirmed install AND launch on the iPhone 2026-09-27** ("finally i got it" /
  "its open on my phone"). Note: the first build took a while to surface to the tester (Apple's
  internal-distribution propagation for a new app's first build); the App Store Connect side was
  correct throughout.

**GOAL COMPLETE (2026-09-27):** Overseer is on TestFlight and running on the owner's iPhone. TF-1
through TF-8 done; TF-9 (Android) deferred with blocker. Built locally from phone tip `95b0946d`,
real brand mark, only-Overseer in the Apple account (aquafriends untouched).
- [ ] **TF-9 — Android (optional).** **Not started (blocker recorded 2026-09-27):** the TestFlight
  goal is iOS; the Android equivalent (Play Console internal testing) needs a **Google Play
  Developer account** (a separate one-time paid account the owner hasn't set up), which the goal put
  out of scope ("no new paid account"). A signed AAB with an Overseer keystore is buildable locally
  from the same source and marks if the owner later wants sideload/Play distribution — say the word.

## What is owner-only vs agent-doable

| Step | Who |
| --- | --- |
| TF-1 App ID, TF-4 marks, TF-5 config, TF-6 build, TF-9 Android | Agent |
| TF-2 App Store Connect record | Owner, or together in the browser |
| TF-3 Agreements, TF-7 upload auth (2FA **or** a scoped API key), TF-8 install on iPhone | Owner |

The APNs **push key** is out of scope here — it belongs to the real-iPhone push step and waits for
PR #10's push sender to be merged and ready to consume it.
