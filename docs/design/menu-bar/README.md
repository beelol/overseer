# Overseer in the menu bar (AC-262 mockup)

Open `index.html` in a browser. It draws Overseer's menu-bar item with its menu open, on a light and a dark menu bar, at the Mac's own size. Pick the state at the top; hover rows, open a repository, answer a request or pick an agent, and the page says what the real item would do. Nothing talks to a daemon; the agents are made up.

The icon is `docs/design/brand/overseer-icon-flat.png` as a template image, so it takes the menu bar's colour. The menus use macOS's own colours and type. Only the dot has a colour of its own.

## The states

| State | Screenshot | What it shows |
| --- | --- | --- |
| 1 Quiet | `1-quiet.png` | 3 agents working, nothing needs you, no dot. Summary "3 working" (zero counts left out). The overseer submenu is open to show an agent row: title, then status and account. Voice Mode off, so Mute is greyed out. |
| 2 Needs you | `2-needs-you.png`, `2b-needs-you-submenu.png` | 2 permission requests at the top, each with Allow once and Deny. The icon carries the dot; the repositories holding a waiting agent get an orange pip. `2b` is the other way to answer: one submenu per request (Allow once, Deny, Open in VS Code). |
| 3 30 agents | `3-thirty-agents.png`, `5-narrow-thirty-agents.png` | overseer 18, site 7, notes 5, one needs-you item. The menu stays the same height however many agents run: one row per repository with its count. The overseer submenu lists the 8 most recent (needs-you, working, to review, idle, each with its account), then "Show all in Overseer…" for the other 10. |
| 4 Daemon stopped | `4-daemon-stopped.png` | The icon dimmed; the menu says "Overseer isn't running" and offers "Start Overseer". |

Choosing an agent opens it in VS Code; Open Overseer opens the three-column workspace; Talk to Overseer opens the conversation.

## Questions for the owner

1. The dot's colour: violet (Overseer's accent from `tokens.js`, the mockup's default), the menu bar's own colour, or red? Orange is left out because macOS already puts an orange dot in the menu bar for "microphone in use", and Voice Mode uses the microphone. (The "Dot" switch on the page shows all three.)
2. Answering a request: buttons right in the menu (default), or a submenu per request (`2b`)? Buttons are one click; the submenu keeps the menu shorter when several agents wait.
3. Add "Always allow" beside Allow once and Deny?
4. Repositories: ordered by latest activity (as drawn), by count, or by name? And past about 6 repositories, fold the rest into "More repositories"?
5. The summary leaves out zero counts ("3 working"). Keep, or always show all three?
6. The item stays in the menu bar when the daemon is stopped, so it has to start at login rather than with the daemon. Is that right? And should the menu end with a "Quit" (hide the item until next login)?
7. Dev daemons: no menu-bar item of their own (recommended), or a second item labelled with the dev name?
8. With several agents waiting, a dot (as drawn) or a number beside the icon ("2")?

## Build note

The repo already builds two small macOS helpers into the extension's `bin/`: `Overseer Notifier.app` (AC-52, Swift/AppKit, `extension/notifier/main.swift`, built by `extension/notifier/build.js` into a universal, ad-hoc-signed `LSUIElement` bundle with the AC-179 icon) and `Overseer Listener.app` (Voice Mode's Rust listener in a bundle, `extension/listener/build.js`); the daemon finds both next to its binary (`daemon/src/background.rs`, `daemon/src/voice/mod.rs`). The status item fits the notifier's pattern: a new `extension/menubar/main.swift` built by `extension/menubar/build.js` into `bin/Overseer Menu.app` (`LSUIElement`, `NSStatusItem` + `NSMenu`, the flat PNG exported at 18/36 px with `isTemplate = true`, the dot drawn as its own layer so the silhouette stays a template). Because it must show "not running" and offer "Start Overseer", it should outlive the daemon: register it as a login item (`SMAppService.mainApp`, macOS 13+), and have the daemon `open -g` it on start so it appears without a new login. It talks to the daemon over its Unix socket (`overseerd.sock`, `paths::socket_path`, honouring `OVERSEER_SOCKET`) with the same JSON-lines protocol as the extension: `hello` with `client: "menubar"` (so it never counts as a VS Code window for AC-45's notice, and the production guard of AC-212 applies), `state` plus the event subscription to rebuild the menu, `run.permission {run_id, request_id, allow}` for Allow once and Deny, `voice.get`/`voice.set` for Voice Mode and mute, and "Start Overseer" spawns `overseerd serve` detached as `extension/src/daemon-client.js` does. Choosing an agent opens `vscode://beelol.overseer/open-agent?run=<id>` (already used by notifications, `daemon/src/notices.rs`); Open Overseer and Talk to Overseer need two more URL handlers in the extension (the AC-250 layout and the conversation).
