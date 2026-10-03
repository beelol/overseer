# No-mod transcript regression

The combined full run at `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2` failed the actual packaged UI audit: grid text rose from the unchanged Gate J budget of 993 characters to 1017 across every tested theme/width. The prior full source63 grid had 993; the current grid contains two extra `mods applied` rows although no Mod is enabled. `grid-comparison.json` retains only those synthetic grid metrics/text samples. The coordinator also inspected the current screenshot and confirmed the visible row.

Both conversation renderers fell through their unknown-event handler for Mods bookkeeping. The correction adds only `mods_applied` and `mods_changed` to each renderer's quiet classification. Daemon event storage/transport, raw event logs, dedicated Mods view/history, refresh subscriptions, outcomes and unknown-event visibility remain unchanged. This avoids implying delivery success from a raw event name, including prepared, failed and uncertain outcomes.

The new nine cases execute the actual shipped webview and phone model, with no-mod and enabled fingerprints across four delivery outcomes plus library-change/unknown-event controls. All nine first failed in the webview (`red.log`), and after only the extension correction all nine failed independently in the phone (`phone-red.log`). After both corrections, full phone-model type checks and all161 tests passed (`green.log`). No source audit threshold was changed.

Base for this fix: combined follow-up da69176. Fresh packaged audit and final combined full qualification are still pending; the existing full run is unchanged and retains its failures. This is presentation correctness, not token compression or a Mods delivery qualification.
