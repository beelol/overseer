# Queued by the owner (2026-09-27): after the current work

The current work is Auto and Swarm on `claude/auto-swarm`, the phone app (#10), Voice Mode (#16) and the dev daemons feature (Gate T). When that has merged, these run in this order. Each runs as its own agent and none blocks the others.

## 1. The Overseer site on GitHub Pages

A very good-looking public page for Overseer, built by its own agent.
- **Look:** exactly Overseer's look, using the bold third theme ("Overseer", Gate M) and its design tokens (`extension/design/tokens.js`), with the owner's mark from `docs/design/brand/`.
- **Content:** each feature explained in turn: the agents list and chat, review, the grid and dashboard, Talk to Overseer and Gate S's control, Auto, Swarm, Continuity, Audio and Voice Mode, the phone app, mods and dev daemons.
- **Motion:**
  - smooth scrolling;
  - the Overseer mark takes part as you scroll: it moves between sections, changes how it moves, and does its talking animation (Voice Mode's mark, `docs/design/voice-mark/`) as it introduces the next piece.
- **Build:** a static site that GitHub Pages can serve (a workflow deploys it). The agent chooses the tooling; plain HTML, CSS and JavaScript are fine, and nothing server-side is needed.
- **Its own criteria** in the main RFC, verified with screenshots at desktop and phone widths, with reduced motion respected.

## 2. Mods

Build the mods feature (`docs/rfcs/mods.md`, "The chosen approach") with its own agent: turn the draft into agreed criteria on main first, then build it criterion by criterion.

## Usage telemetry (queued by the owner, 2026-09-28)

Know which models and accounts are used most and how much each costs: per run and turn, the harness, model, effort, account (by Overseer's own id, never the login), tokens and cost when reported, quota readings, duration and outcome. It feeds Auto's route choices and the model-priority research.
- **First, local only:** append-only files under Overseer's home (for example JSON lines per day), with a way to see and delete them, and nothing leaving the machine.
- **Later:** an optional upload to something cheap or self-hosted that the owner runs (their Coolify), switched off by default and named in the settings.
- **Never:** prompts, code, file contents, credentials or email addresses.
- Its own RFC section and criteria before it is built, by its own agent.

## 3. Then: every open criterion, then usability

- **Keep going** until every acceptance criterion is met or waits only on the owner (the everything goal, `docs/goals/everything.md`).
- **Then look for gaps in real use.** The bar is that Overseer is clearly better than talking to Codex and Claude Code separately and relaying between them. It should feel like a real orchestrator: you talk to it, it picks the right models and accounts, sends work to the right agents, and keeps them on track. Find what gets in the way of that, write it up as criteria, and fix it.
