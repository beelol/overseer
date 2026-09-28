> **Name:** this feature is called **mods** (the owner's choice, 2026-09-27). The text below was drafted as "plugins"; read "mod" for "plugin".

# Side RFC: Plugins — rules, skills, styles and limits you plug into your agents

Status: owner request (2026-09-27). A product proposal for the owner to agree or change. Nothing is
built and no criteria are on the ledger. The [draft criteria](#draft--not-agreed-acceptance-criteria)
at the end are not agreed; they get AC numbers only after the owner agrees on the product.

## The request

The owner, 2026-09-27:

> plan out some system for "plugging things in". doesn't have to be code plugins. maybe they're
> "plugins" which is a mix of rules and skills and stuff? What would the end user do? ex: what if I
> want to apply a system that ensures minimizing tokens? sometimes it's via terminal or cli or
> something else. we just need to easily be able to apply something like that. one example is
> caveman, or I think something called unicorn? minimizing tokens is one example. maybe there are
> generally other transformers or constraints I'd want to apply. we also need to be able to install
> skills and rules with governance on where they go: install, then enable for global or one project
> etc.

## The short version

- A **plugin** is a named bundle of things that change how agents work: rules, skills, a writing
  style, limits, extra tools and harness options. Caveman is a plugin. So is "our repository's
  rules", "no web access" or "stay under medium effort".
- **Install** puts a plugin in Overseer's library. **Enable** decides where it applies:
  everywhere, one repository, one group of agents (Overseer's own session, watchers, a Swarm
  category), or one agent. Installing never applies anything by itself.
- Overseer **translates** each plugin for each harness (Claude Code, Codex, OpenCode) and hands it
  to the run through flags and files in the run's own folder. It never edits `~/.claude`,
  `~/.codex`, the repository or any other configuration of yours, the rule the daemon already keeps
  for its own tools (AC-180).
- Every agent shows **what is applied and why**: which plugin, from which scope, which version, how
  it reached this harness, and what could not be applied.
- **Governance**: every plugin has a source and a fingerprint, is pinned to a version, and anything
  that runs code (a tool server, a hook) needs your yes on the Mac, again whenever it changes.
- Recommendation: build this as Overseer's own bundle format, made of open formats that already
  exist (Markdown rules, Agent Skills, MCP), importing Claude Code plugins and repository files
  rather than inventing a new ecosystem. Start with text pieces (rules, skills, style) and the
  token-saving case; add limits and tools next; code-running pieces last.

## What a plugin is

A plugin is a folder with a short manifest and the pieces it carries. The owner never has to write
one: most come from a source (a Git repository, a Claude Code marketplace, a skill folder) or are
made in Overseer from a sentence ("New rule: never touch the migrations folder").

### The pieces

| Piece | What it does | Example | Runs code? |
| --- | --- | --- | --- |
| **Rules** | Standing instructions, always in the agent's context | "Answer in as few words as possible. Code and errors unchanged." | No |
| **Skills** | Instructions and files the agent loads only when the task needs them (the Agent Skills `SKILL.md` format) | A "write a commit message" skill; a "use our test harness" skill | No, unless the skill ships scripts |
| **Style** | How the agent writes its answers to you. One style per agent. | Caveman at *full*; "bullet points only" | No |
| **Transformers** | Change what goes into or out of the model on the way | Compress command output before the agent reads it (RTK); prefix every message you send | Usually yes (a hook or a program) |
| **Limits** | Constraints the harness or the daemon enforces | Web tools off; effort at most medium; never write `migrations/`; 200k tokens a day per agent | No |
| **Tools** | Extra tools through an MCP server | A documentation search server | Yes |
| **Harness options** | A harness's own settings, where no piece above covers it | Codex `model_verbosity = "low"` | Sometimes |
| **Native bundle** | A harness's own plugin, passed through unchanged to that harness only | A Claude Code plugin with hooks | Often |

A plugin may carry any mix. Caveman is rules plus a style plus a few skills. "Our repository's
rules" is rules only. A token-saver kit could be caveman's style plus RTK's transformer plus a
limit on effort.

### Who uses it, and why

- **The owner, daily.** Apply a way of working to many agents at once without editing three
  harnesses' configuration files by hand, then see and undo it from any surface.
- **The owner's accounts.** Today an account signed in through Overseer has its own configuration
  folder (`CLAUDE_CONFIG_DIR` or `CODEX_HOME` per profile, `daemon/src/daemon.rs`), so the rules and
  skills in your own `~/.claude` or `~/.codex` reach only the desktop-linked account. Plugins make
  "everywhere" mean every account and every harness.
- **A team or a repository.** A repository can say which plugins its agents should run with, the
  way VS Code recommends extensions for a workspace; you accept it once.
- **Overseer itself.** Its own session, its watchers and a Swarm category are groups a plugin can
  target: a terse style for check-ins, a testing skill for the QA category.
- **Plugin authors.** Anyone who already publishes a skill or a Claude Code plugin gets Codex and
  OpenCode users through Overseer with no extra work, as far as the pieces translate.

### What a plugin is not

- Not code that runs inside the daemon. The daemon reads manifests and text; it never loads a
  plugin as a library. Code-running pieces run where the harness runs them (a hook, an MCP server),
  as separate processes, after your yes.
- Not a way to loosen anything. A plugin cannot raise a permission mode, answer a permission
  request, pass an API key, turn off a guardrail or reach another agent. The daemon's rules come
  first (see [Precedence](#precedence-and-conflicts)).
- Not a marketplace Overseer hosts. Overseer reads sources that exist; it does not run a store.

## The token-saving case

This is the example the owner gave, so it is worth being concrete about what exists and what
Overseer can actually do.

### What caveman and "unicorn" are

**Caveman** is [JuliusBrussee/caveman][caveman] ("why use many token when few token do trick").
It makes the agent drop articles, pleasantries and hedging while leaving code, paths, errors and
security warnings in full. It now comes in three forms, which map neatly onto Overseer's pieces:

- **A skill**, the part most people mean: a rule file with levels `lite`, `full` (the default),
  `ultra` and classical-Chinese `wenyan` variants, switched with `/caveman [level|off]`, plus
  `/caveman-commit`, `/caveman-review`, `/caveman-compress` (shortens `CLAUDE.md`-style prose) and
  `/caveman-stats`. It installs with `npx skills add JuliusBrussee/caveman -g`, as a Claude Code
  plugin from its own marketplace, or as a Gemini extension, and works in any harness that reads
  Agent Skills. In Overseer terms: rules, a style and skills.
- **A proxy** (`@caveman-ai/cli`, then `caveman claude`, `caveman codex` and so on) that shrinks
  what the agent *reads*: logs, JSON, diffs and search results. In Overseer terms: a transformer
  that runs code.
- **A middleware** for people building their own apps. Not relevant to Overseer.

Its own numbers, not checked independently: about 65% fewer tokens in the headline, 8.5% fewer
output tokens on 86 tasks in a JetBrains test, 33.2% fewer input tokens through the proxy, and one
case where the proxy made things 9.9% worse. The skill adds about 1,000 input tokens per call.
Its README's own advice is the right one: run the same task with and without it and compare the
bill. A third-party analysis puts the skill's real saving at roughly 4–10% of a whole session,
because prose is a small part of it ([Pillitteri][pillitteri]). A July 2026 paper makes the
general point that fewer tokens is not always a lower cost ([arXiv 2607.12161][tokcost]). The
skill is MIT-licensed; the proxy's engine is BSL-1.1.

**"Unicorn": not identified.** Searches found no token-saving tool, skill or mode by that name,
and it is in none of the curated lists ([awesome-llm-token-optimization][awesome], [Pinggy's
roundup][pinggy]). The nearest names are unrelated (a CPU emulator; `UNILORN/claude-skills`, a
general skills collection). Likely candidates for what the owner remembers:

- caveman's own `ultra` or `wenyan` modes, which are its most memorable part;
- [**terse**][terse], a Claude Code skill in the same spirit (claims 58–78% compression, several
  modes and levels);
- [**RTK** (Rust Token Killer)][rtk], a single binary whose hook rewrites commands such as
  `git status` to `rtk git status` and compresses their output (claims 60–90%). It installs as a
  PreToolUse hook on Claude Code and Codex, a plugin on OpenCode, and hooks on Gemini and Cursor.
  It only sees shell commands, not Claude Code's own Read and Grep tools;
- [**Headroom**][headroom], which compresses tool output and history as a proxy, an MCP server or
  a wrapper (`headroom wrap claude`);
- [**TOON**][toon], a compact notation for JSON-like data. It is a format, not something you
  install into an agent.

None of this changes the design: each is rules, a style, skills, a transformer, or a mix. What the
owner needs is to plug any of them in, in one step, with a choice of where.

### Where the tokens go, and what each lever touches

A coding agent's turn spends tokens in three places. Overseer's spike for Gate S measured the
baseline: about 57k tokens per model iteration on Claude Code and 69k per turn on Codex exec,
mostly cache reads of the system prompt, tools and files (orchestrator RFC, *Spike results*).

| Where | What fills it | What reduces it | Kind of piece |
| --- | --- | --- | --- |
| What the model reads | System prompt, tool definitions, rules files, files opened, command output | Fewer or shorter rules; skills instead of always-on rules; compressed command output (RTK); fewer tools | Rules, skills, transformers, limits |
| What the model thinks | Reasoning tokens | Lower effort; a lighter model for routine work | Limits; route picking |
| What the model writes | Its prose to you, code, tool calls | Terse style (caveman); Codex's `model_verbosity` | Style, harness options |

Two honest consequences:

1. Caveman's skill saves on the prose the agent writes, which is often a small part of a turn;
   the code it writes is unchanged by design. The saving on the whole bill is smaller than the
   headline. Compressing command output (RTK, caveman's proxy) goes after the larger, input side.
2. Overseer cannot save a token after the fact. Summarising an answer for the phone makes it
   shorter to read, not cheaper. Savings come only from changing what the model reads, thinks or
   writes, so a token plugin must reach the harness before the turn.

### What Overseer adds on top

- **Measure it.** Overseer already records usage per turn. With a plugin on, the agent's panel can
  show tokens per turn with and without it on comparable work, labelled as an observation, not a
  controlled test. Route picking's usage learning records the plugin set so it does not credit a
  model for a saving caveman made.
- **Count its own cost.** Rules cost tokens every turn (cached, but not free). Every text piece
  shows its size, and an agent's always-on plugin text has a budget, so a token-saving plugin
  cannot quietly add more than it saves.
- **Apply it to Overseer too.** Overseer's check-ins and watchers are turns it pays for. A terse
  style for those roles is the cheapest win and needs no change to the agents.

## User stories

Each story says what the owner does and what they see. "The Mac" means VS Code or the terminal UI.

1. **A token-saving style for every agent.** I type "caveman" in *Install plugin*, pick the
   result, and choose **Everywhere**. The next turn of every agent, on every harness and account,
   runs with it. Each agent's header shows a *caveman* chip; hovering says "Everywhere, since
   14:02". Running turns are not touched; the change lands at each agent's next turn.
2. **One repository only.** In the same dialog I choose **This repository** instead. Agents in
   that repository, in any worktree, get it; others do not.
3. **One agent.** In the new-agent composer I open the *Plugins* chip and tick caveman for this
   agent only, or I do it later from the agent's header, from its next turn.
4. **Turn it off.** From the chip or the library I switch it off at the scope where it was on.
   If it is on everywhere and I want it off in one repository, I switch it off *there*; the
   everywhere binding stays. It remains installed until I remove it.
5. **Install a skill from a source and choose where it applies.** I paste a Git URL (or pick a
   Claude Code marketplace entry, or drop a folder with a `SKILL.md`). Overseer shows what is inside
   (two skills, no scripts), where it came from and its fingerprint. I install it, then enable one
   skill for one repository and leave the other off.
6. **See what is applied to a running agent, and why.** The agent's *Applied* view lists every
   plugin with its scope ("Everywhere", "Repository overseer", "This agent", "Swarm category QA"),
   version, what each piece became on this harness ("rules → developer instructions", "style →
   rules text", "MCP server → not applied: generic harness"), whether native children get it, and
   its size in tokens. Below it: what the harness loads by itself (the repository's `AGENTS.md`, its
   `.claude/skills`), marked *native, not managed by Overseer*.
7. **A team's shared rule set.** The repository has a checked-in `.overseer/plugins.toml` naming
   its rules and two recommended plugins. The first time I start an agent there, Overseer asks once:
   *This repository asks for 3 plugins* with what they contain. I accept, pick some, or decline.
   When the file changes, it asks again.
8. **Two plugins that conflict.** I enable a "verbose explanations" style everywhere while caveman
   is on in one repository. Overseer says, before any launch: *Two styles for agents in overseer:
   caveman (repository) wins over verbose (everywhere).* The narrower scope wins; I can flip it.
   Two plugins that both define a tool named `docs` are refused until I rename or drop one.
9. **A plugin that does not support a harness.** A plugin's hook is written for Claude Code
   only. On a Codex agent the Applied view says *hook: not applied on Codex (the plugin has none
   for it)*; its rules still apply. If I marked the plugin **required**, Overseer refuses to start
   that agent on Codex and says why, and route picking skips Codex for it.
10. **From the terminal.** `overseerd plugin install <source> --scope repo` and `overseerd plugin
    why <agent>` do the same as the dialogs, for scripts and for the terminal UI's users.
11. **My own global rules on every account.** Overseer offers once: *Your `~/.claude/CLAUDE.md` and
    3 skills apply only to the desktop-linked account. Make them a plugin for every account?* Yes
    copies them into a plugin I own (read once, never written back).
12. **Tell Overseer.** "Make all my agents in this repository terse" in the conversation gives a
    proposal card: *Enable caveman (installed) for repository overseer, from each agent's next
    turn.* One yes applies it.
13. **From the phone.** On an agent I see its Applied list and switch an installed plugin on or off
    for the agent, the repository or everywhere. Installing, and approving anything that runs code,
    stays on the Mac.
14. **Update safely.** The library says *caveman 1.3 → 1.4 available* with a diff of what changed.
    Nothing moves until I update; running agents pick it up at their next turn.
15. **A plugin that needs a program.** RTK needs the `rtk` binary. The plugin says so; Overseer
    shows *Needs `rtk` on PATH (not found)* and the command its author gives to install it. It does
    not install programs itself.
16. **A limit, not a suggestion.** I add "no web tools" for watchers. The Applied view marks it
    *enforced* on harnesses whose tools the daemon can switch off, and *watched* where it can only
    see the call afterwards, the same honesty guardrails use (AC-187).

## How it works

### Library and bindings

Two separate things, so installing and applying never blur:

- **The library** is the daemon's folder of installed plugins (under Overseer's data folder, next
  to its database). Each version is kept whole with its fingerprint (a hash of every file), its
  source and when and by whom it was installed.
- **A binding** says: this plugin (at this version, with these options) is on, or off, at this
  scope, for these harnesses and accounts. Bindings are daemon state, like holds and guardrails,
  so they work with VS Code closed and every surface sees the same thing.

A manifest is small and indicative (the implementing session settles the format):

```toml
name = "caveman"
version = "1.4.0"
source = "github.com/<author>/caveman@<commit>"
summary = "Terse replies. Code, errors and paths unchanged."

[options]
level = { choices = ["lite", "full", "ultra"], default = "full" }

[rules]
files = ["rules/{level}.md"]

[style]
file = "style/{level}.md"          # exclusive: one style per agent

[skills]
dirs = ["skills/caveman-commit", "skills/caveman-review"]

[harness.codex]
config = { model_verbosity = "low" }   # a harness option, Codex only

[needs]
network = false
```

A plugin made from a single sentence is the same thing with one rules file.

### Scopes

From broad to narrow:

| Scope | Applies to | Example |
| --- | --- | --- |
| **Everywhere** | Every agent on every harness and account | Caveman for all work |
| **Repository** | Agents in one repository, in any of its worktrees (identified by its remote, falling back to its path) | The team's rules |
| **Group** | Overseer's own session; watchers; one Swarm category; agents Overseer starts | Terse check-ins; the QA category's test skill |
| **Agent** | One agent and its native children, from its start or its next turn | Try a skill on one task |

Any binding can also be filtered to some harnesses, accounts or models ("only on Claude", "only on
the work account"). A filter narrows where it applies; it is not a scope of its own.

*Everywhere* means the owner's agents. Overseer's session and watchers are reached through their
group, so a style meant for coding agents does not change how Overseer talks to you unless you
say so. (An [open question](#open-questions-for-the-owner).)

### Precedence and conflicts

The effective set for an agent is worked out by the daemon at each turn, with no model:

1. **Collect** every binding whose scope and filters match the agent.
2. **Narrower wins** for the same plugin: agent over group over repository over everywhere. An
   *off* at a narrow scope removes an *on* from a broader one, and the reverse. Options (caveman's
   level) take the narrowest value.
3. **Locked bindings hold.** The owner can lock a binding ("always, everywhere"). Narrower scopes,
   repository files and Overseer cannot turn it off; only the owner unlocks it.
4. **Limits combine strictly.** Two limits on the same thing keep the stricter (effort ceilings
   medium and high give medium). Nothing narrower loosens a locked limit.
5. **Exclusive slots.** One style per agent; one tool server per name; one skill per name; one
   value per harness option. When two plugins fill the same slot, the narrower scope wins; at the
   same scope, Overseer refuses the second binding when it is made and asks which to keep. A
   manifest may also declare what it conflicts with.
6. **The daemon's rules come last and win.** Guardrails, holds, read-only watchers, permission
   answers, the no-API-key rule and Overseer's own tools sit above every plugin. A plugin's
   harness option that touches permissions, sandboxing, approvals or credentials is refused, as the
   daemon already refuses `extra_args` that carry keys or tokens (`adapters.rs`).

Conflicts the daemon can see (slots, names, options) are shown before launch. Conflicts in meaning
("be terse" in one rule, "explain everything" in another) cannot be detected reliably; Overseer
shows every rule that reaches an agent side by side and in order, and a plugin can declare its kind
(`provides = "style"`) so the common cases become slot conflicts.

Order of text that reaches the agent: plugin rules (broad to narrow), then the style, then
Overseer's briefing and guardrails (AC-190, AC-187), then your message.

### How each harness receives it

The principle, already proven by Gate S's tools: everything is delivered per run, through
command-line options and files in the run's own folder, and nothing is written into your
configuration, your repository or your worktree. Overseer runs Claude Code, Codex exec and
OpenCode as one process per turn (resuming the session), so the set can change between turns.

#### What each harness reads, and where

| | Claude Code | Codex | OpenCode |
| --- | --- | --- | --- |
| **Global (user)** | `~/.claude/CLAUDE.md`, `~/.claude/skills`, `~/.claude/agents`, `~/.claude/output-styles`, `~/.claude/settings.json` (hooks, enabled plugins), `~/.claude.json` (MCP). Under Overseer, `~` here is the account's `CLAUDE_CONFIG_DIR` | `$CODEX_HOME/AGENTS.md` (or `AGENTS.override.md`), `$CODEX_HOME/config.toml` (MCP, hooks, plugins, `developer_instructions`), `$HOME/.agents/skills` | `~/.config/opencode/opencode.json` and `AGENTS.md`, its `agents/`, `commands/`, `plugins/`, `skills/`; also reads `~/.claude/CLAUDE.md` and `~/.claude/skills` |
| **Project** | `CLAUDE.md`, `.claude/rules/`, `.claude/skills`, `.claude/agents`, `.claude/settings.json`, `.mcp.json`; also reads `AGENTS.md` | `AGENTS.md` from the repository root down to the working folder (32 KiB cap), `.codex/config.toml` (trusted projects only), `.agents/skills`, `<repo>/.codex` hooks | `opencode.json`, `AGENTS.md` (or `CLAUDE.md`), `.opencode/` agents, commands, plugins and skills, `.claude/skills`, `.agents/skills` |
| **Per run** | `--append-system-prompt[-file]`, `--append-subagent-system-prompt[-file]`, `--output-style`, `--settings <file>` (hooks and any setting), `--setting-sources`, `--plugin-dir`, `--add-dir` (its skills load), `--agents <json>`, `--mcp-config` with `--strict-mcp-config`, `--allowedTools`, `--disallowedTools`, `--model`, `--effort`; `--bare` skips all discovery | `-c key=value` for any config key: `developer_instructions`, `model_instructions_file`, `mcp_servers.*`, `model_verbosity`, `model_reasoning_effort`, inline hooks, `plugins.*`; `--profile`; `exec --ignore-user-config`, `--ignore-rules` | `OPENCODE_CONFIG_CONTENT` (inline config, wins over global and project), `OPENCODE_CONFIG` (a file), `OPENCODE_CONFIG_DIR` (a folder of agents, commands, plugins, skills), `--agent`, `-m`; `--pure` runs without external plugins |
| **Enterprise (locked)** | Managed settings: allowed marketplaces, managed-only hooks and MCP, forced plugins | `requirements.toml`: sandbox, approvals, hooks, allowed MCP servers and plugin sources | Managed config and macOS profiles |
| **Its own plugins** | Plugins from marketplaces, installed per user, project or local, recorded in `enabledPlugins` | Plugins bundling skills, MCP servers and hooks, from marketplaces with a pinned `ref` | JS/TS plugins or npm packages with hooks such as `tool.execute.before` |

Sources: [Claude Code settings][cc-settings], [memory][cc-memory], [skills][cc-skills],
[output styles][cc-styles], [hooks][cc-hooks], [plugins][cc-plugins], [CLI][cc-cli];
[Codex AGENTS.md][cx-agents], [config][cx-config], [skills][cx-skills], [hooks][cx-hooks],
[plugins][cx-plugins], [managed configuration][cx-managed], [exec][cx-exec];
[OpenCode config][oc-config], [rules][oc-rules], [skills][oc-skills], [plugins][oc-plugins].
All three read the same `SKILL.md` format ([Agent Skills][agentskills]), which is why skills are the
easiest piece to carry across.

#### How Overseer hands each piece over

Everything goes in the per-run row above, from files in the run's folder
(`runs/<id>/plugins/`). Items marked *spike* are the proposed route, to be confirmed on the
installed versions before the design is fixed, as Gate S did (AC-180).

| Piece | Claude Code | Codex | OpenCode | Generic |
| --- | --- | --- | --- | --- |
| Rules | `--append-system-prompt-file`, and `--append-subagent-system-prompt-file` so subagents get them | `-c developer_instructions=…` (children: spike) | `instructions` in `OPENCODE_CONFIG_CONTENT` | Message preface |
| Style | An output style in a run plugin, chosen with `--output-style` | Rules text, plus `model_verbosity` where the plugin sets it | Rules text, or the prompt of a run agent chosen with `--agent` | Message preface |
| Skills | A run plugin folder through `--plugin-dir` (or `--add-dir`) | By reference in the developer instructions; a `-c` route to an extra skills folder: spike | `skills/` in a run `OPENCODE_CONFIG_DIR` (spike), else by reference | By reference |
| Limits | `--disallowedTools`, `--allowedTools`, `--effort`, `--model`; path limits as deny rules (as guardrails do today); budgets by the daemon | Sandbox, `-c` tool switches, effort and model ceilings; path limits *watched*; budgets by the daemon | `tools` and `permission` in the run config; budgets by the daemon | Daemon only (*watched*) |
| Tools (MCP) | Merged into the run's one `--mcp-config` file beside Overseer's own server, still `--strict-mcp-config` | `-c mcp_servers.<name>.*` with per-tool approval, as Gate S does | `mcp` in the run config | Not applied |
| Transformers | A hook in a `--settings` file or run plugin (RTK's PreToolUse) | Inline `-c` hooks; hook trust is by hash, recorded in the account's folder (spike) | A plugin listed in the run config (`tool.execute.before`) | Not applied |
| Harness options | Added to the run's arguments, after the refusal list | `-c key=value`, after the refusal list | Run config keys, after the refusal list | Arguments |
| Native bundle | A Claude Code plugin through `--plugin-dir` | A Codex plugin (spike) | An OpenCode plugin in the run config | Not applied |

The account folder matters. A desktop-linked account's folder is shared with the desktop app, so
Overseer writes nothing there. An account Overseer signed in owns its folder, but Overseer still
prefers per-run delivery, so both kinds of account behave the same and nothing lingers after a
run.

Three delivery rules:

- **Prefer the channel that reaches children.** Claude Code's `--append-system-prompt` reaches
  the main agent only; `--append-subagent-system-prompt` exists for its subagents, and Overseer
  passes both. Each piece says in the Applied view whether native children get it (*yes*, *no* or
  *unknown*), and the translation prefers the channel that reaches them.
- **Prompt text is the fallback, not the default.** Any text piece can reach any harness, the
  generic one included, as a preface to the message, the way guardrail words do today. It costs
  tokens on every turn and is visible in the chat, so it is used only where nothing native exists,
  and the Applied view says *as message text*.
- **Skills by reference.** Where a harness has no skill loader, Overseer lists each skill's name
  and one-line description in the rules and points at its folder in the run's directory; the agent
  reads `SKILL.md` when it needs it. That is how skills work natively anyway, so little is lost.

### When a change takes effect

- At the agent's **next turn**. A running turn is never changed, restarted or interrupted for a
  plugin. The Applied view shows *pending: from next turn* until then.
- Codex's app-server transport keeps one process for a thread; options fixed at thread start
  (tool servers, config) apply to the next thread, and the view says so.
- Every turn records the exact set it ran with (plugin, version, fingerprint, options, what each
  piece became) as an event, so the history can answer "what was on when it did that?".

### Seeing what is applied

The Applied view, on every surface, answers four questions for one agent:

1. **What** is on: each plugin, version, options.
2. **Why**: the binding that put it there (scope, who set it, when), and any binding it overrode.
3. **How**: what each piece became on this harness, whether children get it, and what was not
   applied and why.
4. **Cost**: the text size each piece adds per turn, and, once there is data, observed tokens per
   turn with and without it.

Below that, **native** configuration the harness loads by itself (discovered read-only: the
repository's `AGENTS.md` and `CLAUDE.md`, its `.claude/skills`, the account's own folder), so
nothing that shapes the agent is hidden, even what Overseer does not manage.

### With Overseer's own features

| Feature | Rule |
| --- | --- |
| Guardrails (AC-187) | A guardrail is one agent's limit set in the moment, 1 KiB, by you or Overseer. A plugin is a standing, reusable one. *Keep as a rule* turns a guardrail into a plugin; a plugin's path limit shows in the agent's guardrail list with the same *enforced* or *watched* label. Guardrails always win. |
| Briefings and the channel (AC-190) | Unchanged, and after plugin text. The Overseer tool server is merged into the same per-run tool configuration as any plugin's tools; a plugin may not use the name `overseer`. |
| Overseer's session and watchers (AC-181, AC-193) | Reached through their group. They stay read-only with a fixed tool list, so only text pieces (rules, style) apply to them; tools, hooks and harness options are refused for these roles. |
| What Overseer may do (AC-185, AC-186) | Overseer can propose enabling or disabling an *installed* plugin: a Steer action for text-only plugins, Confirm for anything that runs code. It never installs, updates, removes or approves a plugin, and never changes a locked binding. |
| Swarm (AC-195) | A Swarm category carries its own bindings; its workers inherit them. Changes reach new jobs, not jobs in flight. Overseer changes a swarm's plugins only through the category, never per worker, keeping one decision-maker per swarm. |
| Route picking (pull request #2) | A plugin marked *required* makes routes whose harness cannot carry it ineligible, with that reason in the route's explanation. Usage learning records the plugin set of each turn. |
| Continuity and local models (Gate L) | A piece that needs the network (a remote tool server) is skipped on an offline run and says so. Small local models have small contexts, so the plugin text budget is shown against the model's context. |
| Voice Mode (Gate R) | "Make them all terse" is the same proposal as typed; enabling a text plugin is Steer, anything that runs code is Confirm. |

### Governance

- **Provenance.** Every installed version records its source (a Git URL and commit, a marketplace
  and entry, a folder, or *made here*), its fingerprint, and when and from which surface it was
  installed. The library shows it.
- **Two trust classes.** *Text* plugins (rules, style, skills without scripts, limits) install
  after one look at their contents. Plugins that **run code** (tool servers, hooks, transformers,
  skill scripts, native bundles) show the exact commands they would run, and need an explicit yes
  on the Mac. A change to any code-running piece, on update, asks again.
- **Versions.** Pinned by default. *Update available* shows a diff; nothing updates by itself. A
  binding can follow a source's latest version only when the owner asks for it, and only for text
  plugins.
- **Enable, disable, remove.** Disabling keeps the plugin installed. Removing lists the bindings it
  would end first. Older versions stay until nothing refers to them.
- **Repository files.** A checked-in `.overseer/plugins.toml` is a *request*, never an
  installation: Overseer asks once, records the file's fingerprint with the answer, and asks again
  when it changes, as VS Code does with workspace trust and recommendations. It can recommend or
  require; it cannot lock.
- **Words are data.** A repository file, a web page or an agent's message saying "install X" or
  "enable Y" is text. Only the owner installs; only the owner or an Overseer proposal the owner
  allows enables. Agents cannot change their own plugins.
- **Programs are the owner's.** A plugin can name programs it needs; Overseer checks for them and
  shows the author's install command. It never downloads or installs a program.
- **Everything is logged.** Installs, updates, approvals, bindings and every turn's applied set are
  daemon events, visible in the history on every surface.

### On each surface

- **VS Code.** A *Plugins* page (the library): each plugin with its source, version, trust class
  and a row of switches for *Everywhere*, *This repository* and groups. *Install plugin…* and *New
  rule…* in the command palette and on the page. In the new-agent composer, a *Plugins* chip shows
  the set this agent would get, with per-agent switches. In an agent's header, the applied chips;
  clicking opens the Applied view.
- **Terminal UI and command line.** `p` on a tile opens the focused agent's Applied view with
  switches for that agent. `overseerd plugin install | list | enable | disable | why | update |
  remove` (beside the existing `overseerd ctl`) covers the rest, for the terminal and for
  scripts.
- **Phone.** Each agent's Applied view, and switches for installed plugins at agent, repository and
  everywhere. Install, update, remove and approving code-running pieces are *Mac only* in the
  gateway's method classes (phone remote RFC).
- **The conversation with Overseer.** Questions ("what's on the login agent?") are Look. Changes
  are proposals with cards, as in the table above.

## Options and trade-offs

| Option | What it is | For | Against |
| --- | --- | --- | --- |
| **A. Message text only** | Every plugin becomes text Overseer adds to each message, like guardrail words | Works on every harness, the generic one too; tiny to build | Costs tokens every turn (odd for a token saver); no lazy skills, tools, hooks or real limits; clutters the chat |
| **B. Sync native files** | Overseer writes each harness's own files (`CLAUDE.md`, `AGENTS.md`, `.claude/skills`, config) in your homes or repositories, as Ruler or rulesync do | Uses each harness's full power; also affects agents run outside Overseer | Edits your configuration and repositories, which Overseer has never done; shows up in diffs and commits; hard to undo; per-agent scope is impossible; desktop-linked accounts share folders with the apps |
| **C. Overseer bundles, delivered per run** (recommended) | Overseer's own manifest over open formats, translated per harness and handed to each run through flags and its run folder | Every scope down to one agent; nothing of yours is edited; one Applied view; honest per-harness gaps; reuses Gate S's delivery path | Translation table to maintain as harnesses change; some pieces do not exist on some harnesses |
| **D. Claude Code plugins as the format** | Adopt Claude Code's plugin format and marketplaces, and translate outward | A real ecosystem, caveman included; less to design | Built around one harness; hooks and commands do not translate; its scopes are Claude's, not Overseer's |

**Recommendation: C, with D's ecosystem as an import source.** Overseer's scopes (repository,
group, one agent, every account) and its promise never to edit your configuration only work if
Overseer owns the binding and delivers per run, and Gate S already delivers tools that way on all
three harnesses. The pieces themselves stay in formats people already publish (Markdown rules,
Agent Skills, MCP, Claude Code plugins), so installing caveman or a skill needs nobody to
repackage anything. Build it in three steps: text pieces with scopes, the Applied view, the command
line and the token-saving case first; then limits, tools and the repository file; code-running
transformers and native bundles last, each behind the Mac's yes.

Option A stays as the fallback path inside C. Option B's reach outside Overseer is not a goal: the
owner's decision for Gate S was that agents outside Overseer are ignored for now.

## Open questions for the owner

1. **Which "unicorn"?** See [above](#what-caveman-and-unicorn-are). If you remember where you saw
   it, the plugin design does not change, but the first supported plugins might.
2. **Does *Everywhere* include Overseer itself?** Proposed: no; Overseer's session and watchers are
   opted in through their group, so its answers to you stay as clear as you set them.
3. **Should Overseer install programs a plugin needs** (RTK's `rtk`)? Proposed: no, it shows the
   command and you run it.
4. **Native configuration.** Proposed: leave the harness's own files (repository `AGENTS.md`, the
   account folder's settings) alone and show them. Or should a binding be able to turn them off
   for Overseer runs? Each harness has a switch for it (Claude Code `--setting-sources` or
   `--bare`, Codex `--ignore-user-config`, OpenCode `--pure`).
5. **Import your global rules** (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, your skills) into one
   plugin for every account? Proposed: offered once, never written back.
6. **Team files.** Is `.overseer/plugins.toml` in repositories wanted now, or later when others use
   Overseer?
7. **What may Overseer do alone?** Proposed: propose enabling installed text plugins (Steer) and
   anything that runs code (Confirm); never install.
8. **The phone.** Proposed: view and switch installed plugins; nothing else.
9. **Required plugins and route picking.** Should *required* exclude routes, or only warn?
10. **Name.** *Plugin* clashes with Claude Code's own plugins. Keep it, or *kit* / *pack*?
11. **Defaults for the token case.** Caveman at *full* for agents and *ultra* for Overseer's
    check-ins, or nothing on until you choose?

## Draft — not agreed: acceptance criteria

These are a draft for discussion. They are not on the ledger, have no AC numbers and bind nothing
until the owner agrees on the product above; then they are renumbered on `main`.

- [ ] **PLUG-01 — Install is not enable.** Installing from a Git URL and commit, a Claude Code
  marketplace entry, a local folder, a `SKILL.md` folder or a typed rule adds a version to the
  library with its source, fingerprint and time, and changes no agent. **Verify:** daemon tests
  install from each source kind with fixtures; the next launch of an existing agent carries no
  new piece.
- [ ] **PLUG-02 — Four scopes and filters.** Bindings at everywhere, repository (any worktree of
  it), group (Overseer's session, watchers, a Swarm category, agents Overseer starts) and agent,
  each filterable by harness, account and model, stored as daemon state. **Verify:** a matrix
  test resolves the effective set for agents across two repositories, three harnesses and two
  accounts.
- [ ] **PLUG-03 — Precedence.** Narrower wins; *off* removes a broader *on*; options take the
  narrowest value; locked bindings hold; limits combine to the stricter; one style, tool name,
  skill name and option value per agent. **Verify:** unit tests for each rule, including a
  locked binding a repository file tries to turn off.
- [ ] **PLUG-04 — Nothing of the owner's is edited.** Every piece is delivered through
  command-line options and files in the run's folder; no file under the owner's home harness
  folders, the repository or the worktree is written. **Verify:** a fixture run on each harness
  with plugins on leaves those folders byte-identical, and the worktree's diff empty.
- [ ] **PLUG-05 — Translation per harness, stated honestly.** Rules, style, skills, limits and
  tools reach Claude Code, Codex and OpenCode as the RFC's table says; a piece a harness cannot
  take is reported *not applied* with the reason, and a *required* plugin refuses the launch.
  **Verify:** fixture harnesses capture the arguments, environment and run-folder files per
  harness; one live turn per harness at the paid-turn limits shows a rule taking effect.
- [ ] **PLUG-06 — Next turn, never mid-turn.** A binding change reaches each affected agent at its
  next turn and never interrupts a running one; each turn records its exact applied set as an
  event. **Verify:** a scenario toggles a plugin during a fixture turn and checks the next
  turn's launch and the event.
- [ ] **PLUG-07 — The Applied view.** For any agent, VS Code, the terminal UI and the phone show
  what is on, why (binding and anything it overrode), how each piece reached the harness,
  whether children get it, its size, and the harness's own native files marked as such.
  **Verify:** packaged-UI scenario and a TUI test on one fixture agent with three plugins from
  three scopes.
- [ ] **PLUG-08 — Conflicts before launch.** Slot and name conflicts are shown when a binding is
  made and resolved by scope; two at one scope refuse the second binding with both names.
  **Verify:** tests for two styles, two tool servers named alike and two values for one Codex
  option.
- [ ] **PLUG-09 — Trust and versions.** Code-running pieces show their commands and need a yes on
  the Mac; an update that changes one asks again; versions are pinned and an update shows a
  diff first. **Verify:** tests for approval, re-approval on changed fingerprint and a refused
  unapproved hook.
- [ ] **PLUG-10 — The daemon's rules win.** No plugin can raise a permission mode, answer a
  permission, pass a key or token, override a guardrail or read-only role, or use the name
  `overseer`. **Verify:** each attempt is refused by the daemon with the reason.
- [ ] **PLUG-11 — Repository requests.** A checked-in `.overseer/plugins.toml` asks once, records
  the answer against its fingerprint, asks again when it changes, and cannot lock. **Verify:**
  scenario with a fixture repository.
- [ ] **PLUG-12 — With Overseer, Swarm and route picking.** Overseer proposes plugin changes as
  Steer (text) or Confirm (code) cards and never installs; a Swarm category's bindings reach its
  new jobs only; a required plugin makes routes that cannot carry it ineligible. **Verify:**
  daemon tests against the Gate S action classes and, once on main, the Swarm and route-picking
  contracts (partial until both are).
- [ ] **PLUG-13 — The command line.** `overseerd plugin install | list | enable | disable | why |
  update | remove` does what the dialogs do, through the same daemon methods. **Verify:** CLI
  tests against a fixture daemon.
- [ ] **PLUG-14 — The token case, measured.** Caveman (or its equivalent) installs from its
  published source, applies everywhere in one step, and the agent panel shows observed tokens per
  turn with and without it on comparable fixture turns, labelled as an observation. **Verify:**
  fixture usage events; one owner-run live comparison at the paid-turn limits.
- [ ] **PLUG-15 — Regression coverage.** The tests above run in `cargo test --workspace` and the
  fixture UI suite, and in `scripts/test-all` (AC-147); existing scenarios pass with no plugins
  installed and with a text plugin on everywhere.

## Sources

[caveman]: https://github.com/JuliusBrussee/caveman
[pillitteri]: https://pasqualepillitteri.it/en/news/846/claude-code-caveman-mode-token-saving
[tokcost]: https://arxiv.org/pdf/2607.12161
[awesome]: https://github.com/pleasedodisturb/awesome-llm-token-optimization
[pinggy]: https://pinggy.io/blog/tools_to_reduce_ai_coding_agent_token_usage/
[terse]: https://github.com/Dragoon0x/terse
[rtk]: https://github.com/rtk-ai/rtk
[headroom]: https://www.headroomlabs.ai/
[toon]: https://github.com/toon-format/toon
[cc-settings]: https://code.claude.com/docs/en/settings
[cc-memory]: https://code.claude.com/docs/en/memory
[cc-skills]: https://code.claude.com/docs/en/skills
[cc-styles]: https://code.claude.com/docs/en/output-styles
[cc-hooks]: https://code.claude.com/docs/en/hooks
[cc-plugins]: https://code.claude.com/docs/en/plugin-marketplaces
[cc-cli]: https://code.claude.com/docs/en/cli-reference
[cx-agents]: https://learn.chatgpt.com/docs/agent-configuration/agents-md.md
[cx-config]: https://learn.chatgpt.com/docs/config-file/config-reference.md
[cx-skills]: https://learn.chatgpt.com/docs/build-skills.md
[cx-hooks]: https://learn.chatgpt.com/docs/hooks.md
[cx-plugins]: https://learn.chatgpt.com/docs/plugins.md
[cx-managed]: https://learn.chatgpt.com/docs/enterprise/managed-configuration.md
[cx-exec]: https://learn.chatgpt.com/docs/non-interactive-mode.md
[oc-config]: https://opencode.ai/docs/config/
[oc-rules]: https://opencode.ai/docs/rules/
[oc-skills]: https://opencode.ai/docs/skills/
[oc-plugins]: https://opencode.ai/docs/plugins/
[agentskills]: https://agentskills.io/specification

Prior art for governance, and the idea taken from each:

- Claude Code marketplaces and managed settings keep *which sources are allowed* apart from *what
  is enabled where*; a repository's marketplace waits until the folder is trusted
  ([settings reference](https://code.claude.com/docs/en/settings-reference)). Taken: library and
  bindings are separate, and repository files are requests.
- Codex trusts a hook by the hash of its definition, so a changed hook is reviewed again, and keeps
  enforced requirements apart from defaults ([hooks](https://learn.chatgpt.com/docs/hooks.md),
  [managed configuration](https://learn.chatgpt.com/docs/enterprise/managed-configuration.md)).
  Taken: fingerprints and re-approval on change; locked bindings.
- Cursor rules say when each rule applies: always, when the agent decides, by file pattern, or
  when named ([rules](https://cursor.com/docs/context/rules)). Taken: rules versus skills, and
  path-scoped rules as a later option.
- VS Code workspace trust and extension recommendations
  ([workspace trust](https://code.visualstudio.com/docs/editing/workspaces/workspace-trust)).
  Taken: the repository asks once, and the answer is tied to the file.
- npm lockfiles pin exact versions with integrity hashes
  ([package-lock](https://docs.npmjs.com/cli/configuring-npm/package-lock-json)). Taken: pinned
  by default.
- The MCP Registry verifies who publishes a namespace
  ([registry](https://github.com/modelcontextprotocol/registry)). Taken: provenance is recorded.
  Signing waits until sources offer it.
- Ruler and rulesync write one set of rules into each agent's own files. That is option B below.

## The owner's answers (2026-09-27)

- **Examples:** caveman and RTK are the kind of thing meant. The name "unicorn" (or "pony") is not important; drop the search.
- **Scopes:** agents have their own "everywhere" (all agents), and Overseer's own session is a separate place to enable things. Enabling for all agents does not include Overseer's session.
- **What installing means:** a plugin installs everything it needs, isolated from the owner's own tools ("virtualized"), and deleting it removes everything it installed. Replace the proposal "Overseer shows the command" with that.
- **Global `~/.claude` and `~/.codex` rules:** ignored by default. They apply only if the owner opts in; a one-time "migrate them into Overseer" option may be offered.
- **Repository team files:** later, not in the first version.
- **Installing on its own:** by default, Overseer notices that a plugin would help (or that the owner asked for one) and asks first ("Install caveman? It will …"), like Codex asks; the owner says yes. A setting may later allow automatic installs.
- **Name: mods** (chosen 2026-09-27 over loadouts, rigs, traits, kits and lenses). The feature is "mods": install a mod, enable it for all agents, a repository, a group, one agent or Overseer's own session. Where this document says "plugin", read "mod"; the criteria use "mod".

## The chosen approach (agreed with the owner, 2026-09-27)

This supersedes the options above where they differ; the criteria will follow it.

**What a mod is.** A folder with a `mod.toml` manifest and its contents: text (rules, skills, an output style), tools (MCP servers), programs it depends on with the hooks that call them (RTK is one), limits and harness options, and where it applies. For example:

```toml
name = "rtk"
source = "github:rtk-ai/rtk"          # where it came from
homepage = "https://…"                # found by Overseer; its icon shows on the mod's card
[program]
build = "cargo install --root ."      # installed inside the mod's own folder
[hook]
on = "command-output"
run = "bin/rtk filter"                # a separate process, never inside the daemon
[permissions]
network = false
files = "the run's worktree, read only"
```

**Each piece has a kind** (text, tool, hook, program, limit, harness option), and Overseer translates each kind for each harness at launch. RTK, for example: when it is on for a repository or an agent, that run's launch puts the mod's own `rtk` on the run's path and adds a command hook to the run's own settings (a per-run settings file for Claude Code, the equivalents for Codex and OpenCode), so every shell command the agent runs goes through `rtk`. Nothing changes for other agents or the owner's own terminal. A piece a harness cannot take is shown on the mod card ("not applied on OpenCode: no command hook"), never silently dropped.

**Code runs at arm's length.** A mod may bring a program in any language (Rust, JavaScript, Python, a binary). It runs as its own process with the permissions its manifest declares, installed into the mod's own folder, and it is built there when it comes as source. Nothing a mod brings is loaded into the daemon itself: no in-process plugin interface, because the daemon holds the owner's accounts and logins. Deleting a mod removes everything it installed.

**Overseer makes the mods.** The owner does not write manifests. They say what they want ("make my agents use fewer tokens", "install caveman"). Overseer recognises what they mean and finds it itself: the project's real home (repository or site), its icon for the mod's card, and how it installs (an Agent Skill, an MCP server, a Claude Code plugin, a package), or it writes rules from the sentence. It writes the manifest and shows what it will install, run and change, and where. It installs on the owner's yes, isolated, and enables it where they said. There is no registry for now; one may come later.

**Each mod keeps its story.** Who asked and their words, when, what Overseer found and why it chose that source, the commands it ran, what it installed where, and every change since (enabled here, updated, removed). The mod's card shows it.

**Safety tiers.** Text-only mods (caveman, rule sets) are low friction. Tools (MCP servers) are asked about. Programs and hooks are asked about, isolated, and limited to their declared permissions. Code inside the daemon: never.

**Managing mods.** Mods are managed two ways, with the same result:
- **A Mods list** in VS Code (and in the terminal UI, and read-only on the phone). Each mod is a card with its icon, name, source link, where it is enabled, what it installed, its permissions, its version and its story. It can enable or disable a mod per place (all agents, a repository, a group, one agent, Overseer's own session), update it, or remove it.
- **By asking Overseer**, in the conversation or by voice: "what mods does this agent have?", "turn caveman off for this repo", "remove RTK", "make a mod that keeps answers under 200 words". These are daemon methods through Gate S's session, classed like its other actions: reading is a Look, enabling or disabling a text mod is a Steer, and installing, updating a program, or removing is a Confirm (asks the owner).
