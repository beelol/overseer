# Side RFC: Overseer develops Overseer — the dev daemons feature, production and deploy (Gate T)

Status: requested by the owner on 2026-09-27. Criteria: AC-206 to AC-215 in the main RFC
([Gate T](../overseer-rfc.md#gate-t--overseer-develops-overseer-added-by-the-owner-2026-09-27)).
The goal is [dev-instance-goal.md](dev-instance-goal.md). Built in four stages, each its own
pull request, each merged before the next starts: the production guard (`claude/prod-guard`),
the dev daemons tooling (`claude/dev-instance`), guided owner tests (`claude/guided-tests`) and
the deploy (`claude/deploy`).

**The name (owner, 2026-09-27): "dev daemons".** *The dev daemons feature* runs developer versions
of Overseer (a daemon, and VS Code, the TUI or a simulator pointed at it) without colliding with
each other or with the production extension the owner has installed. One dev daemon is also called
an instance: it has a name `x` and runs as `dev-x`.

## Why

The owner's words (2026-09-27): *"we're going to need a version where we can run the Rust client,
and then run the extension, pointing to one of the Rust clients. I think we need a way to run this
containerized, at least for testing. If I want to use Overseer to develop Overseer in the future, I
would like to be able to tell it to build Overseer. It should be able to figure out easily how to go
make its own dev-running version of the Rust client."*

The core case: **the owner develops Overseer while using it.** The installed daemon, the owner's
VS Code and the paired phone keep running normally, and agents, started from that very Overseer,
build and test dev daemons beside them, automatically and without asking. The owner calls it
"like a deployment stage": environments with one deliberate promotion path.

## Environments

| | **dev** | **production** |
| --- | --- | --- |
| What | any number of named instances an agent builds and tests from a checkout | the owner's installed Overseer |
| Who starts it | any agent, without asking (`scripts/dev`) | the owner (VS Code, the TUI, the phone) |
| Lifetime | disposable (`scripts/dev clean`) | kept; data and logins are the owner's |
| Changes by | rebuilding from any checkout | **deploy** only |

**Deploy** is the only path from dev to production: a pull request is merged to `main` (AC-146),
then `scripts/deploy` builds from `main` and installs it for the owner. Nothing else reaches
production.

The owner set the order: **first, production can never point at a dev version; only then may
anyone open dev versions; then deploy.**

## Stage 1 — the production guard (AC-212, and AC-213 after pull request #10)

### What is production

- **The extension** installed in the owner's standard VS Code extensions folder
  (`~/.vscode/extensions`; `~/.vscode-insiders/extensions` for Insiders). An extension loaded from
  any other folder (`code --extensions-dir …`: the UI test harness, dev profiles) is not
  production and keeps today's behaviour.
- **The TUI** when no dev marker file sits next to its binary.
- **The daemon** those two start.

### The dev marker

Everything dev is marked, explicitly, in two ways:

1. `OVERSEER_INSTANCE=dev-<name>` in a dev daemon's environment (`<name>`:
   `[a-z0-9][a-z0-9-]{0,31}`);
2. a file `overseer-dev-instance` (its content: `dev-<name>`) next to the binaries `scripts/dev`
   copies into an instance folder.

A daemon is **dev** when either marker is present.

### A dev build refuses the production home

A dev daemon refuses to serve (exit code 4, one clear line) unless `OVERSEER_INSTANCE` is
`dev-<name>` (and matches the marker file when both exist), `OVERSEER_HOME` is set, its data
folder is not the standard one, and its socket is neither the standard socket nor inside the
standard long-path fallback folder (`/tmp/overseer-<uid>/`). It checks before it creates any
folder. It reports its instance in `hello` and skips the notifier's LaunchServices registration.

### Production refuses anything marked dev

Production clients (the extension and the TUI as defined above):

- **use the standard instance even with dev variables leaked into their environment.** They
  ignore `OVERSEER_HOME`, `OVERSEER_SOCKET` and `OVERSEER_INSTANCE` in their own environment and
  remove them from the environment of every daemon command they run (`socket-path`, `serve`).
  Example: VS Code opened with `code .` from a terminal where `scripts/dev env` was evaluated
  still reaches the owner's daemon. The TUI's explicit `--home DIR` flag is a deliberate choice
  and still works (its tests use it);
- **never start or ask a daemon binary that carries the dev marker** (`overseer.daemonPath`,
  `OVERSEERD`, `--daemon`); they say which binary and why;
- **refuse a daemon whose `hello` reports an instance**: they disconnect at once, do not retry
  into it and say so.

The extension's dev pin settings (stage 2) are ignored in production and are `machine`-scoped,
so a repository's `.vscode/settings.json` (this repository, opened in the owner's VS Code)
cannot set them.

Why these rules and not a container or a build flag: the environment is what leaks (a terminal,
a launcher), so production must not trust it; the extensions folder is what VS Code itself uses
to separate installations, and it is what the test harness already changes; a marker next to
the copied binaries needs no second build of the daemon.

### The phone (AC-213, after pull request #10)

The phone app finds the Mac by Bonjour (`_overseer._tcp`, `fp=` in TXT) or a typed address.
Production side: the release app browses only `_overseer._tcp` and refuses a gateway whose
handshake reports a dev instance, even at a typed address; a dev daemon's gateway never
advertises `_overseer._tcp` (only `_overseer-dev._tcp` with `inst=dev-<name>`, and only with
`--mdns`) and reports its instance in the handshake. `phone/` and the gateway are being
finished in pull request #10 by another agent and are not edited here.

## Stage 2 — the dev daemons tooling (AC-206 to AC-211)

### Isolation on the Mac ("containerized")

A real container cannot host what Overseer is on the Mac: VS Code's window, macOS notifications
(LaunchServices must launch the notifier), the keychain the harnesses keep their logins in, the
iOS simulator, and Bonjour on the host network. Docker on macOS runs a Linux VM, so none of those
reach it. **Decision:** isolation on the Mac is a separate:

| Piece | Production | Dev instance `x` |
| --- | --- | --- |
| Data folder | `~/Library/Application Support/Overseer` | `<dev root>/x/home` (`OVERSEER_HOME`) |
| Socket | `<data>/run/overseerd.sock` (or `/tmp/overseer-<uid>/…`) | `<dev root>/x/overseerd.sock` (`OVERSEER_SOCKET`) |
| Binaries | inside the installed extension | copied to `<dev root>/x/bin/` with the dev marker |
| VS Code | the owner's profile and extensions | `<dev root>/x/vscode/profile`, `…/extensions` |
| Gateway (phone) | port 47810, `_overseer._tcp` | its own free port and key; `_overseer-dev._tcp` with `inst=dev-x`, off unless `--mdns` |
| Harness logins | the owner's | none (fixture harnesses) unless `--owner-logins` for this instance |
| Notifications | the Overseer notifier | a log file in the instance folder |
| Ollama | the owner's server | none (pointed at a closed port) |

The dev root is `~/.overseer-dev` (or `OVERSEER_DEV_ROOT`), outside production's folder.

**Later step (not in Gate T):** a real Linux container (Docker) for the daemon, the TUI and the
Rust and extension unit tests, `scripts/dev up --container`, which also gives AC-41 (Linux) its
first environment. It cannot run VS Code UI scenarios or the phone simulators and does not
replace the Mac isolation above.

### One command: `scripts/dev`

```
scripts/dev up     --name x [--repo <path>] [--release] [--no-build] [--restart] [--owner-logins] [--mdns] [--json]
scripts/dev down   --name x [--keep-clients]   stop its daemon and agents (and its VS Code and TUI)
scripts/dev status --name x [--json]           what runs, where, from which commit
scripts/dev list   [--json]                    every instance under the dev root
scripts/dev logs   --name x [-f]               the daemon's log
scripts/dev ctl    --name x <method> [json]    one request to that instance's daemon
scripts/dev env    --name x                    shell lines that point a dev tool at it
scripts/dev code   --name x [folder] [--vsix <file>] [--no-build]   an isolated VS Code pointed at it
scripts/dev tui    --name x [--dry-run]        the instance's TUI pointed at it
scripts/dev phone  --name x --platform ios|android   the simulator app pinned to it (after #10)
scripts/dev clean  --name x | --all            down, then remove the instance folder(s)
```

- `up` builds `overseerd` and `overseer-tui` from the checkout (`cargo build`, debug unless
  `--release`), **copies** them into the instance's `bin/` with the dev marker (replacing files by
  rename, so rebuilding never changes a running daemon's binary), starts the daemon detached with
  the instance environment, waits for `hello`, and prints one block: name, repo, commit (and
  whether the tree was dirty), pid, socket, data folder, gateway port, logins mode, and the next
  commands. `--json` prints the same as JSON. `up` on a running instance says so and changes
  nothing; `--restart` rebuilds and restarts it (agents keep running and are reattached; open
  windows reconnect).
- Instance metadata lives in `<dev root>/x/instance.json`; `list` and `status` read it and check
  that the pid and socket are alive.
- `down` stops the instance's agents and daemon (`daemon.stop_all`), then its VS Code windows and
  TUI (unless `--keep-clients`), then anything still running from the instance folder.
- `clean --all` is the one-line way to leave nothing behind.

### Safety

- **Refusals in the script:** a name that is not `[a-z0-9][a-z0-9-]{0,31}`; a dev root inside
  production's folder; a socket path equal to production's or under `/tmp/overseer-<uid>/`; a
  socket path over 100 bytes (it names a shorter `OVERSEER_DEV_ROOT`); a `--repo` that is not an
  Overseer checkout. Inherited `OVERSEER_*` variables are dropped before the instance's are set.
- **The daemon's own refusals** are stage 1's: a dev daemon never opens the production home.
- **Logins:** by default the Claude harness is the repository's fixture, Codex and OpenCode are
  disabled, the "desktop" logins point at an empty folder inside the instance
  (`OVERSEER_TEST_SYSTEM_HOME`) and Ollama at a closed port. `--owner-logins` (per instance,
  recorded in `instance.json` and shown by `status`) uses the real harnesses with the owner's
  logins; paid turns then follow `AGENTS.md`. Nothing is ever signed in or out by `scripts/dev`.
- **Never production:** no command stops, restarts, reinstalls or connects to the production
  daemon; `down` and `clean` act only on pids, sockets and folders recorded for that instance.
  `code` installs only with `--user-data-dir` and `--extensions-dir` inside the instance folder.

### One-way isolation

Production needs nothing from dev daemons, knows nothing about them and never talks to one.
All awareness is on the dev side: nothing in the daemon or the TUI reads the dev root; dev
sockets are never in production's runtime folder or its fallback; no file is shared; dev
instances never advertise `_overseer._tcp`.

### VS Code pointed at an instance

`scripts/dev code --name x` builds the VSIX from the checkout (`node extension/scripts/package.js`,
or `--vsix <file>`), installs it into the instance's own profile and extensions folders, writes
the profile's settings and opens VS Code with them:

- `overseer.daemonPath`: the instance's `bin/overseerd`;
- `overseer.daemonSocket`: the instance's socket;
- `overseer.devInstance`: `dev-x` (the status bar reads **Overseer dev-x**, the window title
  starts with `[dev-x]`).

With `overseer.daemonSocket` set (only honoured outside the standard extensions folder), the
extension connects only to that socket and checks that `hello` reports `dev-x`. It never starts
a daemon and never falls back to another socket: when the instance is not running it says
*"Dev instance dev-x is not running (socket …). Start it with `scripts/dev up --name x`."* and
keeps retrying the same socket, so `scripts/dev up --restart` is picked up by open windows.

### The TUI

`scripts/dev tui --name x` runs the instance's `bin/overseer-tui --daemon bin/overseerd` with the
instance environment (a dev TUI, marked, honours it), after checking the instance is up.

### The phone, on the simulators (AC-210, after pull request #10)

Each dev daemon gets its own gateway port (`up` picks a free one from 47900 up and sets
`OVERSEER_GATEWAY_PORT`), its own gateway key (in its data folder, so its own `fp`), and no
advertising (`OVERSEER_GATEWAY_MDNS=off`) unless `--mdns`. `scripts/dev phone --name x --platform
ios|android` launches the installed simulator build with the instance's address as a launch
argument (iOS simulator: `127.0.0.1:<port>`; Android emulator: `10.0.2.2:<port>`), the way
`phone/e2e/lab.mjs` already runs its own daemon with mDNS off and the `manual-address` flow types
an address. Dev builds of the app accept an instance filter for `_overseer-dev._tcp`; release
builds never do (AC-213). Until #10 merges, `scripts/dev phone` prints the address to type.

## Stage 3 — guided owner tests (AC-215)

Owner request (2026-09-27): the owner says to any agent "let's start the voice mode test" (or any
owner check a gate lists), and the agent runs it for them in a dev daemon:

1. starts a dev daemon and an isolated dev VS Code built from the right branch (or main), never
   touching production;
2. pulls up what the test needs: the view, the settings, a scratch repository, fixture agents;
3. walks the owner through the steps one at a time, recording each answer or observation into the
   gate's evidence;
4. cleans up at the end.

**Data.** Each gate's owner checks live in `docs/owner-checks/<name>.json`: the gate and criteria,
the ref to build (a branch or `main`), what to prepare (settings, a scratch repository, fixture
agents, a VS Code command to open the right view), and the steps (what to do, what to watch for,
what to record, and the criterion each serves). Voice Mode's checks
([voice-mode.md](voice-mode.md#the-owners-checks), steps 1 to 8) are the first.

**Runner.** `scripts/dev test <name>` runs one. An agent mediates the conversation, so the runner
works one step per call: `--start` builds and prepares everything and prints step 1; `--record
"<what the owner said or saw>"` records it and prints the next step; `--skip "<why>"` records a
skipped step; `--status` repeats where the test is; `--finish` writes the evidence (Markdown and
JSON under `docs/verification/evidence/owner-checks/<name>/<date>/`) and cleans the dev daemon up.
In a terminal (a TTY) `scripts/dev test <name>` walks the owner through the same steps by itself.
`scripts/dev test --list` names every check. `AGENTS.md` tells agents to use it whenever the owner
asks for an owner check.

## Stage 4 — deploy (AC-214)

`scripts/deploy` is the one path from dev to production. It is run by the owner, or by an agent
only when the owner asked it to in that conversation; it asks before it changes anything
(`--yes` skips the question).

1. **Build from `main`:** fetch `origin/main` into a clean temporary worktree (never the owner's
   checkout), install the pinned packaging tools there and run `node extension/scripts/package.js`:
   the VSIX with the release daemon and the notifier, stamped with the commit.
2. **Install the extension** into the owner's VS Code (`code --install-extension <vsix> --force`)
   and register the notifier with LaunchServices.
3. **Restart the daemon only when no runs are active** (AGENTS.md): it asks the daemon for its
   active runs; with some active it waits (checking every 30 s, up to `--wait` minutes) or, with
   `--no-wait`, stops and says how many are active and how to deploy later. With none it stops the
   old daemon (`daemon.shutdown`) and starts the new one once. Open VS Code windows reconnect; a
   reload picks up the new extension.
4. **Keep data and logins:** it never writes the data folder except its own `deploys/` record, and
   never touches harness logins or the keychain.
5. **Record and roll back:** `deploys/history.json` in production's data folder lists each
   deploy (commit, time, VSIX kept under `deploys/`); `scripts/deploy --rollback` reinstalls the
   previous one the same way; `scripts/deploy --status` shows what is deployed.

Tests deploy onto a temporary "production" (`HOME`, VS Code profile and extensions folder all
temporary), never the owner's.

## Agents developing Overseer

An agent asked to "build Overseer and try it" reads `AGENTS.md`, finds *Running a dev Overseer*,
and runs:

```
scripts/dev up --name <short-name> --repo <its worktree>
scripts/dev ctl --name <short-name> state
scripts/dev code --name <short-name> <folder>     # when it needs the UI
scripts/dev clean --name <short-name>             # always, when done
```

It never touches production and never deploys unless the owner asked.

## Tests

- Stage 1: `daemon/tests/dev_instance.rs` (a dev daemon's refusals, beside a standard daemon
  under a temporary default home that keeps its pid, socket, state and data);
  `test/unit/production-guard.js` (a production extension with dev variables leaked in reaches
  the standard socket, refuses a marked binary and a daemon reporting an instance);
  `tui/src/locate.rs` and `tui/tests` (the same for the TUI).
- Stage 2: `test/dev/run.js` in `scripts/test-all` (two instances side by side through
  `scripts/dev` with a temporary `HOME` and dev root; `--help` complete; nothing left);
  `test/ui/scenario-dev-instance.js` (an isolated VS Code pointed at A shows A's agents, not B's).
- Stage 3: `test/dev/guided.js` (every owner-check file is valid; a fixture check run through
  `--start`, `--record`, `--skip` and `--finish` writes its evidence and leaves nothing running).
- Stage 4: `test/deploy/run.js` (deploy onto a temporary production: waits for active runs,
  restarts once, data and logins intact, recorded, rolled back).
