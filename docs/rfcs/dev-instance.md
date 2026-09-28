# Side RFC: a dev Overseer next to the real one (Gate T)

Status: requested by the owner on 2026-09-27. Criteria: AC-206 to AC-211 in the main RFC
([Gate T](../overseer-rfc.md#gate-t--overseer-develops-overseer-added-by-the-owner-2026-09-27)).
Built on the branch `claude/dev-instance`.

## Why

The owner's words (2026-09-27): *"we're going to need a version where we can run the Rust client,
and then run the extension, pointing to one of the Rust clients. I think we need a way to run this
containerized, at least for testing. If I want to use Overseer to develop Overseer in the future, I
would like to be able to tell it to build Overseer. It should be able to figure out easily how to go
make its own dev-running version of the Rust client."*

The core case: **the owner develops Overseer while using it.** The installed daemon, the owner's
VS Code and the paired phone keep running normally, and agents, started from that very Overseer,
build and test dev instances beside them, automatically and without asking.

## Goals

1. **Build from any checkout.** A daemon (`overseerd`), the TUI (`overseer-tui`) and the VS Code
   extension (VSIX) are built from the checkout the command runs in, or from `--repo <path>`
   (another worktree, an agent's worktree).
2. **Fully isolated from the installed Overseer.** Each dev instance has its own data folder,
   socket, VS Code profile and extensions folder, gateway port and advertised identity. It never
   uses the owner's daemon, data, logins, settings, keychain entries, launch agents or notifier
   registration.
3. **Several named instances side by side** (`a`, `pr-17`, `agent-3f2c`), each with its own name.
4. **Point a client at a chosen instance**: an isolated VS Code, the TUI, and (after pull
   request #10) the phone app on the iOS simulator or Android emulator.
5. **Start, stop, status, list, logs and clean**, and nothing left running afterwards.
6. **Discoverable by agents from the repository alone**: one command documented in `AGENTS.md`,
   and `scripts/dev --help` complete on its own.

Non-goals: changing how the installed Overseer works; a relay or remote host; a second
orchestrator. Dev instances are ordinary daemons with their own folder.

## "Containerized": what isolation means on the Mac

A real container cannot host what Overseer is on the Mac: VS Code's window, macOS notifications
(LaunchServices must launch the notifier), the keychain the harnesses keep their logins in, the
iOS simulator, and Bonjour on the host network. Docker on macOS runs a Linux VM, so none of those
reach it. **Decision:** isolation on the Mac is a separate:

| Piece | Installed Overseer | Dev instance `x` |
| --- | --- | --- |
| Data folder | `~/Library/Application Support/Overseer` | `<dev root>/x/home` (`OVERSEER_HOME`) |
| Socket | `<data>/run/overseerd.sock` (or `/tmp/overseer-<uid>/…` for long paths) | `<dev root>/x/overseerd.sock` (`OVERSEER_SOCKET`) |
| Binaries | inside the installed extension | copied to `<dev root>/x/bin/` at `up` |
| VS Code | the owner's profile and extensions | `<dev root>/x/vscode/profile`, `…/extensions` |
| Gateway (phone) | port 47810, `_overseer._tcp` | its own free port, `_overseer-dev._tcp` with `inst=x`, off unless asked |
| Harness logins | the owner's | none (fixture harnesses) unless `--owner-logins` for this instance |
| Notifications | the Overseer notifier | a log file in the instance folder |
| Ollama | the owner's server | none (pointed at a closed port) |

The dev root is `~/.overseer-dev` (or `OVERSEER_DEV_ROOT`), outside the installed Overseer's
folder. A dev instance is marked by `OVERSEER_INSTANCE=<name>` in its daemon's environment.

**Later step (not in Gate T):** a real Linux container (Docker) for the daemon, the TUI and the
Rust and extension unit tests, `scripts/dev up --container`, which also gives AC-41 (Linux) its
first environment. It cannot run VS Code UI scenarios or the phone simulators and does not
replace the Mac isolation above.

## Decisions

### One command: `scripts/dev`

```
scripts/dev up     --name x [--repo <path>] [--release] [--no-build] [--owner-logins] [--mdns] [--json]
scripts/dev down   --name x            stop its daemon, agents, VS Code and TUI
scripts/dev status --name x [--json]   what runs, where, from which commit
scripts/dev list   [--json]            every instance under the dev root
scripts/dev logs   --name x [-f]       the daemon's log
scripts/dev ctl    --name x <method> [json]   one request to that instance's daemon
scripts/dev env    --name x            the environment lines that point a tool at it
scripts/dev code   --name x [folder] [--vsix <file>]   an isolated VS Code pointed at it
scripts/dev tui    --name x            the TUI pointed at it
scripts/dev phone  --name x --platform ios|android   (after pull request #10)
scripts/dev clean  --name x | --all    down, then remove the instance folder(s)
```

- `up` builds `overseerd` and `overseer-tui` from the checkout (`cargo build`, debug unless
  `--release`), **copies** them into the instance's `bin/` (rebuilding the checkout never swaps a
  running daemon's binary), starts the daemon detached with the instance environment, waits for
  `hello`, and prints one block: name, repo, commit (and whether the tree was dirty), pid, socket,
  data folder, gateway port, logins mode, and the next commands. `--json` prints the same as JSON.
  `up` on a running instance says so and changes nothing; `up --restart` rebuilds and restarts it.
- Instance metadata lives in `<dev root>/x/instance.json`; `list` and `status` read it and check
  the pid and socket are alive.
- `clean --all` is the one-line way to leave nothing behind.

### Safety

- **Names**: `[a-z0-9][a-z0-9-]{0,31}`. The instance folder is always under the dev root.
- **Refusals in the script**: a dev root inside the installed Overseer's folder; a socket path
  equal to the installed one or under `/tmp/overseer-<uid>/`; a socket path over 100 bytes (it
  names a shorter `OVERSEER_DEV_ROOT`); a `--repo` that is not an Overseer checkout.
- **Refusals in the daemon** (`OVERSEER_INSTANCE` set): `serve` exits with a clear message when
  `OVERSEER_HOME` is unset, or when the data folder or socket is the installed Overseer's, so a
  dev daemon can never open the owner's data even if started by hand. It skips the notifier's
  LaunchServices registration. `hello` reports the instance name.
- **Logins**: by default the Claude harness is the repository's fixture, Codex and OpenCode are
  disabled, the "desktop" logins point at an empty folder inside the instance
  (`OVERSEER_TEST_SYSTEM_HOME`) and Ollama at a closed port. `--owner-logins` (per instance,
  recorded in `instance.json` and shown by `status`) uses the real harnesses with the owner's
  logins; paid turns then follow `AGENTS.md` (ChatGPT: `gpt-5.6-luna` at low effort; Claude:
  haiku, light use). Nothing is ever signed in or out by `scripts/dev`.
- **Never the production daemon**: no command stops, restarts, reinstalls or connects to the
  installed daemon; `down` and `clean` only act on pids and sockets recorded for that instance.
- **Never the owner's VS Code**: `code` installs the VSIX only with `--user-data-dir` and
  `--extensions-dir` inside the instance folder; the owner's profile, extensions and settings
  are never written.

### One-way isolation

The installed Overseer (daemon, extension, TUI, phone pairing) needs nothing from dev instances,
knows nothing about them and never talks to one. All awareness is on the dev side:

- Nothing in the daemon or TUI reads the dev root; dev sockets are never in the installed
  daemon's runtime folder or its `/tmp/overseer-<uid>` fallback.
- The extension's pin settings (below) are `machine`-scoped: a repository's
  `.vscode/settings.json` (for example this repository, opened in the owner's VS Code) cannot
  point the owner's extension at a dev instance. They are only written into dev profiles.
- Dev instances never advertise `_overseer._tcp`, so the owner's phone never sees them; when
  `--mdns` is given they advertise `_overseer-dev._tcp` with `inst=<name>`.

### VS Code pointed at an instance

`scripts/dev code --name x` builds the VSIX from the checkout (`node extension/scripts/package.js`,
or `--vsix <file>`), installs it into the instance's own profile and extensions folders, writes
the profile's settings and opens VS Code with them:

- `overseer.daemonPath`: the instance's `bin/overseerd`;
- `overseer.daemonSocket`: the instance's socket;
- `overseer.devInstance`: `x` (the status bar reads **Overseer dev x**, the window title starts
  with `[dev x]`).

With `overseer.daemonSocket` set, the extension connects only to that socket. It never starts a
daemon and never falls back to the socket `overseerd socket-path` would give: when the instance
is not running it says *"Dev instance x is not running (socket …). Start it with
`scripts/dev up --name x`."* and keeps retrying the same socket.

### The TUI

`scripts/dev tui --name x` runs the instance's `bin/overseer-tui --daemon bin/overseerd` with the
instance environment, after checking the instance is up (the TUI would otherwise start a daemon).

### The phone, on the simulators (after pull request #10)

The phone app finds the Mac by Bonjour (`_overseer._tcp`, `fp=` in TXT) or a typed address
(`manual-address` flow). Each dev instance needs, and gets:

- **its own gateway port**: `up` picks a free port from 47900 up and sets `OVERSEER_GATEWAY_PORT`;
- **its own identity**: the gateway's key lives in the instance's data folder (a different `fp`),
  and advertising is off (`OVERSEER_GATEWAY_MDNS=off`) unless `--mdns`, which advertises
  `_overseer-dev._tcp` with `inst=<name>` (a daemon change in `gateway/mod.rs` once #10 is on
  main: with `OVERSEER_INSTANCE` set, never `_overseer._tcp`);
- **a pin**: `scripts/dev phone --name x --platform ios|android` launches the installed simulator
  build with the instance's address as a launch argument (iOS simulator: `127.0.0.1:<port>`;
  Android emulator: `10.0.2.2:<port>`), the way `phone/e2e/lab.mjs` already runs its own daemon
  with mDNS off and the `manual-address` flow types the address. Dev builds of the app accept an
  instance filter (`inst=`) for `_overseer-dev._tcp`; release builds browse only `_overseer._tcp`.

Pull request #10 is being finished by another agent, so `phone/` is not edited on this branch:
the app-side launch argument and filter are AC-210's work after #10 merges.

## Agents developing Overseer

An agent asked to "build Overseer and try it" reads `AGENTS.md`, finds *Running a dev Overseer*,
and runs:

```
scripts/dev up --name <short-name> --repo <its worktree>
scripts/dev ctl --name <short-name> state
scripts/dev code --name <short-name> <folder>     # when it needs the UI
scripts/dev clean --name <short-name>             # always, when done
```

It never touches the installed daemon, and `clean` leaves nothing running.

## Tests

- `daemon/tests/dev_instance.rs`: a "production" daemon under a temporary default home, a dev
  daemon beside it; the production daemon's socket, pid, state, data folder and clients are
  unchanged; the daemon's refusals.
- `test/dev/run.js` (in `scripts/test-all`): two instances through `scripts/dev` side by side
  with a temporary `HOME` and dev root; the default home is never created; `--help` covers every
  command; `clean --all` leaves no process, socket or folder.
- `test/unit/dev-pin.js`: the extension's pinned socket never falls back and never spawns.
- `test/ui/scenario-dev-instance.js`: an isolated VS Code pointed at instance A shows A's
  agents, not B's; stopping A shows the "not running" notice and nothing connects elsewhere.
