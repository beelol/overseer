# One Overseer layout (AC-264): way A or way B

Both ways give the layout you asked for:

- the agents list on the left;
- the agent's review wide in the middle;
- one Overseer panel on the right.

The right panel is home: Overseer's conversation and Voice Mode. Picking an agent turns that same panel into the agent's chat. A back arrow at the top of the chat, or ⌥⌘U, returns it to Overseer.

In both ways, your other VS Code windows keep their tabs, and no user setting is written. The difference is how the tab rows go away.

All screenshots are Overseer Dark, from the packaged extension with fixture agents. Each test also opens a second VS Code window (`notes-repo`), and each "beside" picture shows it next to the Overseer window. That window keeps its tab row the whole time.

## Way A: Overseer in VS Code's right side bar

| | |
|---|---|
| Home | [a/02-home-1920x1080.png](a/02-home-1920x1080.png) |
| An agent picked | [a/05-agent-1920x1080.png](a/05-agent-1920x1080.png) |
| An agent picked, 1440×900 | [a/07-agent-1440x900.png](a/07-agent-1440x900.png) |
| Beside your other window | [a/04-home-beside-other-window.png](a/04-home-beside-other-window.png), [a/06-agent-beside-other-window.png](a/06-agent-beside-other-window.png) |
| Before, and after running it again | [a/01-before-1920x1080.png](a/01-before-1920x1080.png), [a/08-after-1920x1080.png](a/08-after-1920x1080.png) |

**What you see:** the window stays as it is and rearranges in about 4 seconds. The Overseer panel opens in VS Code's right side bar. That side bar has no tab row, only a small "Overseer" title with maximize and close buttons.

**What stays:** the review keeps one tab row ("Review: Finished edit"). VS Code can hide tab rows only through a setting, and a setting would change every window. Diffs only has no breadcrumbs. Follow opens the agent's files as ordinary editors, which would show the tab row and breadcrumbs.

**Downsides:**

- Extensions cannot size VS Code's side bars. Overseer widens its panel by narrowing the middle 60 px at a time with the side bar hidden, which reached 712 px (37%) at 1920.
- That width is fixed in pixels. At 1440×900 the panel stays 712 px and the review shrinks to 366 px: see the 1440 screenshot.
- The panel keeps its width if you hide and show the right side bar. VS Code's own views that live there, such as its chat, share that side bar.
- ⌥⌘U does not yet know when the panel is showing Overseer, so it always goes to Overseer. That can be fixed.

## Way B: the window reopens as the Overseer window

| | |
|---|---|
| Home | [b/03-home-1920x1080.png](b/03-home-1920x1080.png) |
| An agent picked | [b/06-agent-1920x1080.png](b/06-agent-1920x1080.png) |
| An agent picked, 1440×900 | [b/08-agent-1440x900.png](b/08-agent-1440x900.png) |
| Beside your other window | [b/05-home-beside-other-window.png](b/05-home-beside-other-window.png), [b/07-agent-beside-other-window.png](b/07-agent-beside-other-window.png) |
| Before; VS Code's question about unsaved work; back again | [b/01-before-1920x1080.png](b/01-before-1920x1080.png), [b/02-unsaved-prompt.png](b/02-unsaved-prompt.png), [b/09-back-in-folder-window-1920x1080.png](b/09-back-in-folder-window-1920x1080.png) |

**What you see:** the window reloads, which takes about 5 to 6 seconds, and comes back as the Overseer window. There are no tab rows and no breadcrumbs anywhere, so it looks the same as Focus Mode.

**How it works:** the window reopens on a small workspace file that Overseer keeps in its own storage, never in your repository. The file holds your folder and the look settings. Those settings belong to that one window only, which is why your other windows keep their tabs.

**What changes:**

- **Window title:** "Overseer — ws-repo — Visual Studio Code". It is your usual title with the folder's name. VS Code would otherwise say "ws-repo (Workspace)".
- **VS Code's recent list:** never shows the Overseer window's file. Your folder stays in it.
- **Running it again:** reopens your folder the way VS Code kept it. Measured: both groups, every tab in order, the active tab of each, Explorer and the terminal panel, in about 5 seconds.
- **Picking an agent and ⌥⌘U:** work as described above. From home, ⌥⌘U goes to the agent's chat, and pressing it again goes back.

**Downsides:**

- **Unsaved work:** VS Code asks "Do you want to save the changes you made to d.txt?" (Save, Don't Save, Cancel) before it reopens the window. With Save, the words are on disk; nothing is lost silently. Phase 2 should let Overseer handle this itself, either by asking first in plain words or by carrying the unsaved text across.
- **Reloads:** the window reloads on both the way in and the way out. Not measured: whether a command still running in the terminal survives the reload.
- **Explorer header:** if you open Explorer in the Overseer window, its header reads "WS-REPO (WORKSPACE)".
- **Other extensions:** they see the Overseer window as a different workspace, so things they remember per folder (a chosen Python interpreter, for example) are remembered separately there.

## Recommendation: B

B is the Focus Mode look you picked, with no tab rows or breadcrumbs over anything, and it fits any window size. It gets there without touching any other window and without writing a setting. Running it again gives your folder back as VS Code kept it, which also makes "restore exactly" free.

A keeps a tab row over the review and can only size its panel in fixed steps, which breaks at 1440×900.

B's cost is a 5-second reload each way, and VS Code's own save question when something is unsaved. Phase 2 can make that question Overseer's.

## Found on the way

A window opened on a workspace file in Overseer's storage must use a `file:` path. Overseer's storage location is a `vscode-userdata:` address, and VS Code treats a window opened on a file there as a "virtual workspace". It then disables every extension that does not support those, Overseer included. The prototype uses the `file:` path.

"Open Dashboard in New Window" (AC-244, `dashboard-mode.js`) opens its window on such an address, so Overseer is likely off in that window. Focus Mode is retired in phase 2 anyway.

## How these were made

- The prototype runs with `node test/ui/scenario-one-layout-a.js` and `node test/ui/scenario-one-layout-b.js`.
- Their checks and measurements are in `a/result.json` and `b/result.json`.
- The commands are "Overseer: One Layout, Way A: Overseer in the Right Side Bar" and "Overseer: One Layout, Way B: Reopen as the Overseer Window" (`extension/src/one-layout.js`). They are prototype only, and phase 2 replaces them with Workspace (⌥⌘⇧O).
