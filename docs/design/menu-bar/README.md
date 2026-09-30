# Overseer in the menu bar (AC-262 mockup)

Open `index.html` in a browser. It draws Overseer's menu-bar item with its menu open, on a light and a dark menu bar, at the Mac's own size. Pick the state at the top; hover rows, open a repository, answer a request (the ▾ beside Allow once holds Always allow) or pick an agent, and the page says what the real item would do. Nothing talks to a daemon; the agents are made up.

The icon is `docs/design/brand/overseer-icon-flat.png` as a template image, so it takes the menu bar's colour. The menus use macOS's own colours and type. Only the dot (or number) has a colour of its own: Overseer's violet from `extension/design/tokens.js`.

## The states

| State | Screenshot | What it shows |
| --- | --- | --- |
| 1 Quiet | `1-quiet.png` | 3 agents working, nothing needs you, no dot. Summary "3 working" (zero counts left out). The overseer submenu is open to show an agent row: title, then status and account. Voice Mode off, so Mute is greyed out. |
| 2 Needs you | `2-needs-you.png`, `2b-allow-dropdown.png` | 2 permission requests at the top, each with a split button "Allow once ▾" and Deny; the icon carries the dot. `2b` has the ▾ open: "Always allow" with the rule the harness offers for the session (Claude Code's permission suggestion such as `Bash(cargo test:*)`, Codex's "for the rest of this session"). Repositories are most recent first; one holding a waiting agent gets an orange pip. |
| 3 30 agents | `3-thirty-agents.png`, `7-narrow-thirty-agents.png` | overseer 18, site 7, notes 5, one needs-you item. The menu stays the same height however many agents run: one row per repository with its count. The overseer submenu lists the 8 most recent (needs-you, working, to review, idle, each with its account), then "Show all in Overseer…" for the other 10. |
| 4 Six waiting | `4-six-waiting.png`, `4b-six-waiting-number.png` | At most 4 requests in the menu, newest first, then "2 more waiting · Show all in Overseer…", so nothing is pushed off the screen. `4b` is the number version: "6" beside the icon instead of the dot. |
| 5 Dev daemon | `5-dev-daemon-hover.png`, `5b-dev-daemon-menu.png` | A dev daemon (`scripts/dev up --name voice`, running as `dev-voice`) gets its own item beside the installed Overseer's: the same icon with a "!" in its lower corner and no text. Hovering shows "Dev daemon: dev-voice"; its menu opens with that as the first, greyed row. |
| 6 Daemon stopped | `6-daemon-stopped.png` | The icon dimmed; the menu says "Overseer isn't running" and offers "Start Overseer". |

Every running menu ends with Quit ("Agents keep running"): it hides the item until the next login and never stops the daemon. Choosing an agent opens it in VS Code; Open Overseer opens the three-column workspace; Talk to Overseer opens the conversation.

## The owner's answers (2026-09-29)

1. Dot colour: violet.
2. Answering: buttons in the menu, with Allow once as a split button whose ▾ holds "Always allow" (the harness's session rule), then Deny.
3. At most 4 requests in the menu, then "N more waiting · Show all in Overseer…".
4. Several waiting: keep the dot; the number version is shown for comparison (`4b`).
5. Repositories most recent first; the summary leaves out zero counts.
6. The item starts at login, is only a display (never a second daemon), and has Quit at the bottom.
7. Dev daemons get their own item: the icon with a "!", the name in the tooltip and as the menu's first greyed row.

## Build note

The repo already builds two small macOS helpers into the extension's `bin/`: `Overseer Notifier.app` (AC-52, Swift/AppKit, `extension/notifier/main.swift`, built by `extension/notifier/build.js` into a universal, ad-hoc-signed `LSUIElement` bundle with the AC-179 icon) and `Overseer Listener.app` (Voice Mode's Rust listener in a bundle, `extension/listener/build.js`); the daemon finds both next to its binary (`daemon/src/background.rs`, `daemon/src/voice/mod.rs`). The status item follows the notifier's pattern: `extension/menubar/main.swift` built into `bin/Overseer Menu.app` (`LSUIElement`, `NSStatusItem` + `NSMenu`, the flat PNG as a template image, the dot drawn as its own layer so the silhouette stays a template). It is registered as a login item only by the real install path (`scripts/deploy`), never by tests. It talks to the daemon over its Unix socket (`overseerd.sock`, `paths::socket_path`, `OVERSEER_SOCKET` for a dev daemon) with the extension's JSON-lines protocol: `hello` with `client: "menubar"` (never counted as a VS Code window for AC-45; the answer names the instance, so a dev daemon's item shows its "!" and name), `state` plus the event subscription to rebuild the menu, `run.permission {run_id, request_id, allow}` for Allow once and Deny (Always allow needs the daemon to pass the harness's session rule back), `voice.get`/`voice.set` for Voice Mode and mute; "Start Overseer" spawns `overseerd serve` detached as `extension/src/daemon-client.js` does. Choosing an agent opens `vscode://beelol.overseer/open-agent?run=<id>` (already used by notifications, `daemon/src/notices.rs`); Open Overseer and Talk to Overseer are two more URL handlers in the extension.
