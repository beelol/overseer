# VS Code Mods update RPC correction

Base: combined follow-up eeb1f3f1f7595d4a63a6b4edbc501da62c6f00d1. The update preview carries operation=update; both install and update must commit through the declared mods.install method. The panel instead constructed nonexistent mods.update. One production statement now uses mods.install for either preview operation, retaining the update confirmation label and immutable preview guards.

The host mock now retains the preview operation. The new case cancels once with no mutation, then confirms and checks the exact preview/install RPC pair with no bind or other mutation. Original focused run:14/15, solely the wrong method; corrected:15/15. All extension unit files30/30 pass after granting the fixture runner its required socket/process access. Initial restricted suite27/30 is retained separately: local listen EPERM and process setup failures. Focused nice requests also reported sandbox priority restriction, without preventing those assertions. Source syntax and git diff --check passed. Independent mods_design source review accepted.

No package or UI run on this source yet; active full2a remains frozen and does not contain this correction. AC269 stays partial. No Rust build, owner profile, installed daemon or paid turn used.
