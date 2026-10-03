# Mods readable labels

The first packaged Mods scenario at42b8d24 completed its feature assertions and six theme/width combinations but exited1 at the final plain-words gate: it displayed the raw transport `claude` and `transport_accepted` with a raw run identifier in history. This is a genuine rendered failure, not a passing scenario. VSIX SHA256:72403a702da715c103dd959ace6c53988f0d87bd6bf752b3247596d640a90266; daemon reported build42b8d24a0507. The harness cleaned its remaining daemon.

The webview now uses the existing public transport names. History maps delivery outcomes to readable descriptions and uses the envelope run's title, with an honest missing-target fallback. It does not change the daemon's events, identities, saved snapshots or authority. Unknown outcomes never claim acceptance; contradictory payload run IDs do not select the history target.

The actual shipped-webview fixture failed first at the transport-name assertion, then passed after the correction. Added cases cover accepted/prepared/failed/uncertain/unknown outcomes, target title, contradictory payload identity and missing target. Existing hostile text, trust, form draft/focus and preview cases pass; host15/15 and source syntax/diff checks pass. The nice request encountered the sandbox priority restriction but assertions ran. Fresh packaged rerun and combined full result remain pending.

Independent review additionally caught the production host fallback rendering an untitled run as its raw ID. The host now retains the selector ID but labels it Untitled agent; actual host RED15/16 then GREEN16/16 is preserved separately. Final source review accepted all four files. Full extension units30/30 pass; no fixture assertion was weakened. Fresh rendered rerun remains pending.
