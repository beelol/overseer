# Side RFC: offline mode and local models

Status: proposed by the owner on 2026-09-26. Acceptance criteria: AC-83 to AC-97 (Gate L) in the
[main RFC](../overseer-rfc.md#gate-l--offline-mode-and-local-models-added-by-the-owner-2026-09-26).
Builds on the verified OpenCode + Ollama path ([AC-14](../verification/AC-14.md), the
[Ollama log](../verification/evidence/ac-14/opencode-ollama.log)), the account model
([account governance RFC](account-governance.md)) and usage reporting ([AC-62](../verification/AC-62.md)).

## Why

Today a run dies with its connection. When the laptop leaves Wi-Fi, a VPN drops, or a provider has an
outage, the harness prints a network error, the turn is marked failed, and the agent stops. Nothing
retries, nothing moves the work anywhere else, and the failure card looks the same whether the whole
network is gone or only one provider is down. The owner's rule is simple: **the work must always get
done.** A lost connection is a reason to change how, never a reason to stop.

Local models are the way to keep working with no network at all. Overseer already runs OpenCode
against local Ollama models (a `qwen3-coder:30b` run completed a real file write with no account and
no paid tokens), but choosing one is manual, nothing knows how much memory a model needs, and
nothing knows whether this machine can run it.

## Goal

1. **Tell an outage from being offline.** If OpenAI is unreachable and Claude works, the work goes to
   Claude when that is the best working option. Only when nothing online works is Overseer offline.
2. **Transition to a local model when offline**, if the *transitioning* setting is on: read the
   machine's memory, pick the best local model that fits, download it (and Ollama itself) when the
   settings allow, move the work over, and say so: *Transitioning to qwen3-coder:30b (local) because
   you've disconnected.*
3. **Never fail or freeze when transitioning is off.** Say that Overseer is offline, keep retrying
   until a connection returns, deliver the pending message exactly once, and offer local models for
   new agents in the meantime.
4. **Stay inside a conservative memory budget.** A model never takes more than a share of total
   memory (40% by default, never above 50%), and never more than what is free right now minus
   headroom. The budget is reassessed for every new run and turn.
5. **Come back.** When the connection returns, new agents default to online again and a local agent
   can switch back to its original harness and account.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Provider | Who serves the model: `anthropic`, `openai` or `local` (Ollama on this machine). |
| Connection state | One daemon-wide value: **online**, **degraded** (named providers unreachable) or **offline** (no network). |
| Provider health | Per provider: reachable, unreachable (with the reason) or unknown (probes off). |
| Local harness | The harness that runs local models: OpenCode with an Ollama provider (verified); Codex `--oss --local-provider ollama` (to verify). |
| Local model | An Ollama model tag installed on this machine, plus the context length Overseer runs it at. |
| Budget | The most memory one local model may take right now (see [Memory budget](#memory-budget-and-model-fit)). |
| Transition | Moving a run's work to another harness, account or model because of the connection state. |
| Handoff | The mechanism of a transition: a successor run in the same task and workspace, started with a handoff prompt, linked to its predecessor. |
| Transitioning | The setting that allows Overseer to transition on its own. Off means wait and retry. |

## Connection state

The daemon owns the state, because runs continue while VS Code is closed. It is one of three values
with a reason and a per-provider breakdown, changed only by evidence, and every change is an event.

```
              provider probes fail / harness network errors,
              baseline internet still answers
   ONLINE  ───────────────────────────────────────────►  DEGRADED (openai unreachable)
     ▲  ▲                                                     │
     │  │ all probes pass                                     │ baseline internet fails
     │  └────────────────────────────────────────────────┐    ▼
     │                                                   └─ OFFLINE (no network)
     └──────────── baseline internet and a provider answer again
```

**Baseline internet.** A TLS handshake with certificate verification to two independent well-known
hosts, one by name and one by IP (so a DNS failure and a routing failure are told apart), plus the
system's default route. A captive portal fails the certificate check and counts as offline.

**Provider health.** One credential-free probe per provider: a TLS handshake or `HEAD` to the hosts
the harness itself uses (`chatgpt.com` and `api.openai.com` for Codex on a ChatGPT login;
`api.anthropic.com` and `claude.ai` for Claude Code). Any HTTP answer, including 401 or 403, means
reachable. Connect, DNS, TLS or timeout failures mean unreachable. Sustained 5xx or 529 answers from
the harness (two within two minutes) also mark the provider unreachable with reason `outage`.

**Harness evidence.** Error classification (`daemon/src/adapters.rs`, `classify_error`) gains a
`network` class: `ECONNREFUSED`, `ENOTFOUND`, `EAI_AGAIN`, `ETIMEDOUT`, `ENETUNREACH`, `fetch failed`,
`getaddrinfo`, `network is unreachable`, TLS handshake failures, 502/503/504/529 and Codex's
`willRetry` stream errors. A network-class error triggers an immediate probe round.

**Cadence.** Probes run every 30 seconds while any run is active or waiting, every 5 minutes when
idle, immediately on a network-class error, and stop entirely with `offline.probes = false` (then
only harness errors drive the state, and it can only be `online` or `degraded`).

**What is never offline.** `auth`, `rate_limit` and `quota` errors do not change the connection
state; they are account states with their own handling (Sign in again, usage and limits). A
provider that answers 429 is reachable.

| Evidence | State | Shown as |
| --- | --- | --- |
| All probes pass | online | nothing extra |
| OpenAI hosts fail, Anthropic and baseline pass | degraded | `OpenAI unreachable` |
| Both providers fail, baseline passes | degraded | `Claude and OpenAI unreachable` (no failover target; policy behaves as offline) |
| Baseline fails | offline | `Offline` |
| 429 or usage limit from one account | online | usage warning on that account (AC-62) |

## Policy

What Overseer does depends on the state, the *transitioning* setting, and what the run needs.

| State | Run situation | Transitioning on | Transitioning off |
| --- | --- | --- | --- |
| degraded | the run's provider is unreachable; another online provider has a signed-in, installed harness | **fail over** to the best working provider (order: `offline.providerOrder`, default anthropic → openai) | wait and retry the same provider |
| degraded | the run's provider is unreachable; no other online provider works | treat as offline (below) | wait and retry |
| degraded | the run's provider is fine | nothing; the state is shown | nothing |
| offline | any run with a pending or network-failed turn | **transition to a local model** that fits; download only if allowed *and* a connection to the registry exists (so in practice, prefetched or already installed) | wait and retry; offer local models for new agents |
| offline | no eligible local model (none installed, none fits, downloads off) | wait and retry, and say why no local model was used | wait and retry |
| back online | local or failed-over runs | finish the current turn; then per `offline.returnOnline` (default: offer **Switch back**; new agents default to online) | resume the original harness through its own resume path |

Two rules hold in every cell:

- **Work is never dropped.** A pending user message is kept with the run and sent exactly once, by
  whichever harness ends up doing the turn.
- **Nothing is silent.** Every transition, retry schedule and return is a system card in the chat and
  an event in the log, with the reason and the time.

## Local inventory

The daemon reports what the machine can do. Unknown stays unknown; nothing is estimated as zero.

| Fact | Source | Notes |
| --- | --- | --- |
| Total memory | `sysctl hw.memsize` (macOS), `/proc/meminfo` (Linux) | Apple silicon unified memory: RAM is the GPU memory. |
| Available memory now | `vm_stat` free + inactive + purgeable pages (macOS), `MemAvailable` (Linux) | sampled at every pick; smoothed over three samples so a momentary dip does not flip a decision. |
| Memory pressure | `memory_pressure` / `vm.memory_pressure` (macOS) | warn level lowers the budget one step; critical refuses new local picks. |
| Ollama installed | `/Applications/Ollama.app`, `ollama` on PATH (`/usr/local/bin`, `/opt/homebrew/bin`) | version from `ollama --version`. |
| Ollama running | `GET http://127.0.0.1:11434/api/version` | Overseer never assumes a non-loopback host. |
| Installed models | `GET /api/tags` | name, size on disk, family, parameter size, quantization, context length, capabilities (`tools`, `thinking`, `vision`). |
| Model geometry | `POST /api/show` | `block_count`, `head_count_kv` (scalar or per-layer array), `key_length`, `value_length`, `embedding_length`, `head_count`, `parameter_count`. |
| Loaded models and real memory | `GET /api/ps` | `size` and `size_vram` of each loaded model at its `context_length`; the measured number Overseer trusts most. |
| Disk free for downloads | `statvfs` on Ollama's models directory | a pull is refused when the model plus 10% would not fit. |

On this machine (measured 2026-09-26): 128 GiB total, Apple M5 Max, Ollama 0.34.2 running, eight
Qwen models installed (17.3 GiB to 75.8 GiB on disk), and `qwen3-coder:30b-64k` loaded at 23.7 GiB
for a 65,536-token context against 17.3 GiB on disk. The owner's "28 GB" in the request is this
128 GiB machine.

## Memory budget and model fit

### The budget

```
ceiling_share   = total × offline.ramCeilingPercent / 100        default 40%, hard maximum 50%
ceiling_now     = available_now − headroom                        headroom = max(4 GiB, 10% of total)
budget          = min(ceiling_share, ceiling_now)
```

- **Why 40%.** An idle Mac already uses 10–20% of its memory; VS Code, the harness, a browser and
  the build take more. 80% is where the machine starts paging. One model at 40% leaves room for
  everything else and for a second, smaller model if needed. The owner may raise it to 50%; the
  daemon refuses higher values.
- **Why two terms.** The share protects the machine from the model; the free-memory term protects the
  model from everything else that is running right now. Either one alone is wrong: 40% of 128 GiB is
  51 GiB, but with Xcode and Docker holding 90 GiB the honest answer is a much smaller model.
- **Reassessment.** The budget is computed for every new run and every new turn (OpenCode starts one
  process per turn, and the model choice is per turn), never in the middle of a turn. A working turn
  is never killed to free memory. If the budget shrank, the next turn uses a smaller model or a
  smaller context and the chat says so once.

### Fit

A model is measured or estimated at a given context length:

```
estimate(model, ctx) = weights_on_disk + kv_cache(ctx) + overhead
kv_cache(ctx)        = Σ over layers ( head_count_kv[layer] × (key_length + value_length) ) × ctx × 2 bytes   (f16 cache)
overhead             = 1 GiB   (compute buffers; observed 0.4 GiB, kept conservative)
fits                 = size(model, ctx) ≤ budget
```

- `head_count_kv` is a per-layer array for hybrid-attention models (Qwen 3.5 has KV heads on 10 of
  40 layers); the sum handles both shapes.
- A **measured** size from `/api/ps` at the same tag and context replaces the estimate as soon as it
  exists, and is recorded (`local_models_measured`) so later picks use real numbers. On this machine
  the estimate for `qwen3-coder:30b` at 64k is 24.3 GiB against 23.7 GiB measured.
- **Context is the lever.** If the preferred model does not fit at the target context
  (`offline.contextTarget`, default 65,536), Overseer tries 32k, then 16k (`offline.contextFloor`).
  Below 16k a coding agent cannot hold the tool schema, the handoff and a few files, so the model is
  skipped instead.
- **Ranking.** Catalogue tier first, then the largest context that fits. A model that only fits below
  32k is considered after every model that fits at 32k or more. Within a tier, prefer what is
  installed over what needs a download.

### Worked examples (40% ceiling, memory otherwise free)

Estimates from the formula; disk sizes from Ollama (approximate for models not on this machine).

| Total memory | Budget | Picks, in order | Not chosen |
| --- | --- | --- | --- |
| 16 GiB | 6.4 GiB | `qwen2.5-coder:7b` at 16k (≈6.3 GiB); `qwen2.5-coder:3b` at 32k (≈3.9 GiB) | `7b` at 32k (≈7.2 GiB) |
| 32 GiB | 12.8 GiB | `qwen2.5-coder:14b` at 16k (≈12.4 GiB); `qwen2.5-coder:7b` at 32k (≈7.2 GiB) | `14b` at 32k (≈15.4 GiB) |
| 64 GiB | 25.6 GiB | `qwen3-coder:30b` at 64k (≈24.3 GiB, measured 23.7) | `qwen2.5-coder:32b` at 32k (≈27.6 GiB) |
| 128 GiB | 51.2 GiB | `qwen3-coder:30b` at 128k (≈30.3 GiB); `qwen2.5-coder:32b` at 32k; `qwen3.5:35b-a3b` at 64k (≈24.5 GiB) | `qwen3.5:122b` (75.8 GiB on disk) |

With 40 GiB of other work running on the 128 GiB machine, `ceiling_now` is about 75 GiB, so the
share still decides; with 100 GiB in use it is about 15 GiB and the pick drops to a 14B-class model at
16k, exactly as on a 32 GiB machine.

### Context length in Ollama

Ollama's default context is small and per model. Overseer sets the context it picked by creating a
derived tag with a two-line Modelfile (`FROM qwen3-coder:30b` / `PARAMETER num_ctx 65536`), named
`overseer/qwen3-coder-30b-64k`. `ollama create` shares the weight blobs, takes no extra disk and is
instant; the owner's own `-64k` and `-32k` tags on this machine were made the same way. Derived tags
are listed under the base model in the UI and removed with **Clean up local models**. When Overseer
starts the Ollama server itself, `OLLAMA_CONTEXT_LENGTH` is the fallback.

## Model catalogue

Overseer ships a small catalogue of coding models known to work as agents. A model is **eligible**
for automatic picks only when Ollama reports the `tools` capability *and* the catalogue marks it
verified with the local harness; `offline.allowUnverifiedModels` widens that to any installed model
with `tools`. Ranking within a tier is by parameter count.

| Tier | Model (Ollama tag) | Disk | Notes |
| --- | --- | --- | --- |
| 1 | `qwen3-coder:30b` (MoE, 3B active) | 17.3 GiB | **verified**: completed a write through OpenCode ([log](../verification/evidence/ac-14/opencode-ollama.log)); fast for its size |
| 1 | `qwen2.5-coder:32b` | ≈20 GB | to verify |
| 1 | `qwen3.5:35b-a3b` (MoE; tools, thinking, vision) | 22.2 GiB | installed here; to verify |
| 1 | `gpt-oss:120b` | ≈65 GB | needs a 50% ceiling on 128 GiB; to verify |
| 2 | `gpt-oss:20b` | ≈14 GB | Codex's own `--oss` default; to verify with both local harnesses |
| 2 | `devstral:24b` | ≈14 GB | to verify |
| 2 | `qwen2.5-coder:14b` | 8.4 GiB | **failed** on 2026-09-25: emitted its tool call as text through OpenCode; excluded until it passes |
| 3 | `qwen2.5-coder:7b` | ≈4.7 GB | to verify |
| 3 | `qwen3:8b` | ≈5.2 GB | to verify |
| 4 | `qwen2.5-coder:3b`, `qwen2.5-coder:1.5b` | ≈1.9 GB, ≈1 GB | last resort for 8–16 GiB machines; to verify |

The verification check is the same for every entry: a one-turn task through the real local harness
that must create a file with the `write` tool, in a disposable repository. The result (pass, or the
exact failure) is recorded in the catalogue file (`daemon/src/local_catalogue.json`) with the
Ollama and harness versions, and the ledger. Tiers are a proposal until AC-87 confirms them; the
owner can reorder with `offline.preferredModels`.

## Local harness

| Harness | How | Status |
| --- | --- | --- |
| **OpenCode** | The `local` account's `opencode.json` (in its `XDG_CONFIG_HOME`) gets an `ollama` provider (`@ai-sdk/openai-compatible`, `baseURL http://127.0.0.1:11434/v1`) listing the derived tags with `tool_call: true`; `model` and `small_model` point at the pick; `autoupdate: false`, `share: disabled`, `agent.general.permission.task: allow` as in the mock fixture. Model per turn with `-m ollama/<tag>` (already supported). | verified path; the config writer is new |
| **Codex** | `codex exec --oss --local-provider ollama -m <tag>` (flags present in the installed 0.155 binary). Keeps Codex's file-change and child telemetry with a local model. | to verify (AC-87); second choice until then |
| **Claude Code** | Would need `ANTHROPIC_BASE_URL` and a placeholder token in the harness environment. Overseer never forwards `ANTHROPIC_*` or token variables (AC-16), so this is **not in scope**; see [Open questions](#open-questions). | out of scope |

The local account is the existing `local` provider ("OpenCode (local models)", `daemon/src/accounts.rs`),
renamed **Local (Ollama)**, created automatically when Ollama is found, with no sign-in. Its status
row shows the Ollama version, the number of installed models and the current budget.

## Downloads and installing Ollama

Downloading needs a connection, so it happens while online or degraded, never offline. Two settings
gate it, both off by default.

**Models** (`offline.allowModelDownloads`):
- `POST /api/pull` with streaming progress; a system card in the chat shows *Downloading
  qwen2.5-coder:14b · 3.2 of 9.0 GB* with **Cancel**. Partial downloads resume.
- Disk space is checked first; a refused pull says how much is missing.
- The first pull ever asks once in a dialog with the size; later pulls rely on the setting.
- **Prefetch** (`offline.prefetch`, off): while online, keep the best-fitting eligible model for this
  machine downloaded, so going offline works without a download. Recomputed when the catalogue,
  the settings or the machine's memory change. Never runs while a paid run is streaming.

**Ollama** (`offline.allowOllamaInstall`):
- Homebrew when present (`brew install --cask ollama`); otherwise the official macOS archive from
  `ollama.com/download`, whose code signature must verify as Ollama's Developer ID (`codesign
  --verify` and `spctl --assess`) before it is opened. A failed check deletes the download and
  reports it.
- Overseer starts `ollama serve` itself only when nothing answers on `127.0.0.1:11434`, as a
  supervised child bound to loopback, and stops it after `offline.ollamaIdleMinutes` (default 30)
  without local runs. An Ollama the user runs (the menu-bar app) is used as is and never stopped.
- Linux uses the distribution package or the official install script, behind the same setting
  (AC-41 remains deferred).

Downloads come from the Ollama registry only (`registry.ollama.ai`, or `offline.registry` for a
mirror). Nothing is executed from a download except the verified Ollama application.

## Transition (handoff)

A handoff moves work from a **predecessor** run to a **successor** run in the same task and the same
workspace. It is the one mechanism behind failover, going local, and switching back.

1. **Stop the predecessor's turn** if it is still running (the same interrupt the user's Stop uses);
   a turn that already failed with a network-class error needs nothing.
2. **Snapshot** the workspace (a run-start snapshot, as for any turn), so the review's *Latest run*
   base is the moment of the handoff and the successor's edits show on their own.
3. **Pick** the target: the best working online provider's harness and a signed-in account, or the
   local harness with the budget's model (the pick and its reasons are an event).
4. **Compose the handoff prompt** from the daemon's own records, bounded to about 6k tokens so it fits
   the smallest context Overseer runs (16k):

   ```
   You are continuing a task another agent started; its model became unreachable.
   Task: <the task's original prompt>
   Repository <name>, working tree <path>, branch <branch>. Do not change branches.
   Done so far (the previous agent's last messages, newest last):
   <up to 8 assistant messages, each trimmed to 600 characters>
   Files changed since the task started: <path +a −d, …> (from the review's file list)
   The user's last message, not yet answered: <pending prompt, if any>
   Continue from here. Check the files before editing; do not redo finished work.
   ```

5. **Start the successor** with the run's remembered turn options where the target supports them
   (model per turn, permission mode), `predecessor_run_id` set and `handoff_reason` recorded
   (`offline`, `provider_unreachable:openai`, `back_online`, `user`).
6. **Mark the predecessor** `handed_off` (a terminal state distinct from `failed`, with the successor's
   id in `exit_reason`), release its workspace ownership to the successor (one writer at a time), and
   keep its session id so a switch back can resume the original harness's own session when the
   harness supports it.
7. **Announce** in the chat of both runs. The predecessor ends with *Transitioning to
   **qwen3-coder:30b** (local, Ollama) because you've disconnected. Work continues in the same
   worktree.* with a details disclosure (budget, estimate, alternatives considered). The successor
   opens with *Continued from "<title>" after the connection was lost at 13:02.* In the side bar the
   task shows one agent with a small handoff marker; the predecessor is reachable from the successor's
   header and stays in history (AC-63).

Failover reads *OpenAI is unreachable; continuing with **Claude Code** (account "Work") because it is
the best working option.* A handoff never reuses the failed provider's credentials for anything.

## Wait and retry (transitioning off, or no target)

- The failed turn's run becomes **`waiting_for_connection`**, a new active lifecycle state. It is
  entered only on a network-class turn failure or a stall while probes say offline (below); it never
  comes from silence alone (AC-06).
- **Backoff.** Retry after 5 s, then doubling to a cap of 2 minutes (`offline.retryCapSeconds`), with
  ±20% jitter, forever, until the user stops the run. Each attempt is preceded by a probe: the harness
  is relaunched only when the probe for its provider passes, so paid accounts are not hammered
  with doomed requests. Each attempt is an event (`retry`, attempt n, next in s).
- **Exactly once.** The pending prompt is stored on the turn; a relaunch resumes the harness's own
  session (`--resume`, `exec resume`, `--session`) and sends that prompt. A turn that already produced
  a `turn_done` is never re-sent.
- **Stall.** A harness that keeps retrying internally shows no error. If the state is offline and a
  running turn has produced no event for `offline.stallSeconds` (default 90), Overseer interrupts it
  (recorded as Overseer's action, not the harness's) and applies the policy as if the turn had failed.
- **Shown as** one quiet system card, updated in place: *Offline. Retrying the connection every 2
  minutes (attempt 4). Nothing is lost; your message is sent when the connection returns.* with
  **Use a local model now** and **Stop**. Runs in this state are not dimmed and are not per-run
  items in Needs you; one global Needs-you item reads *Offline · 3 agents waiting* with **Use local
  models** and **Keep waiting**. New agents started while offline offer local models only, with the
  online harnesses labelled *offline*.

## Back online

- Local and failed-over runs **finish their current turn**; nothing is interrupted to switch back.
- **New agents** default to their online harness and account again as soon as the state is online.
- Per `offline.returnOnline`: `offer` (default) shows *Back online. This agent is still on a local
  model.* with **Switch back to Claude Code** and **Stay local**; `auto` switches back at the next
  turn and says so; `stay` keeps local runs local. Switching back is a handoff (reason `back_online`)
  whose prompt summarises the local work; when the original harness's session still exists, the
  successor resumes it instead of starting fresh.
- Runs in `waiting_for_connection` resume on their next scheduled attempt, at once when the probe
  passes.

## Several local agents

- Ollama keeps one copy of a loaded model; two local runs on the same tag share it and Ollama
  serialises or parallelises their requests (`OLLAMA_NUM_PARALLEL`). Overseer shows *Queued behind 1
  local agent* on the waiting tile rather than pretending both stream.
- A second, different model loads only if the sum of both fits the budget; otherwise the new run uses
  the already-loaded model when it is eligible, or waits with *Waiting for local model* shown.
- If available memory falls under the headroom while a model is loaded, no turn is killed; the next
  turn's pick shrinks (smaller context first, then a smaller model) with one note in the chat.
- Local runs report usage as tokens with cost 0 and the provider mark **Local**; no quota windows.

## Settings

The daemon owns and enforces these, because the policy must work with VS Code closed. VS Code
settings are the editing surface; the extension pushes them to the daemon (`settings.set`), which
persists them in its `meta` table and validates ranges. `overseerd ctl offline.status` prints the
state, the budget and the current pick.

| Setting (`overseer.offline.*`) | Default | Meaning |
| --- | --- | --- |
| `transition` | `true` when a verified local model is installed, else `false` (proposed) | Let Overseer fail over and go local on its own. Off: wait and retry. |
| `providerOrder` | `["anthropic", "openai", "local"]` (proposed; owner to confirm) | Preference among working providers for failover; `local` is always last. |
| `allowModelDownloads` | `false` | Pull models from the Ollama registry when a pick needs one. |
| `allowOllamaInstall` | `false` | Install Ollama (Homebrew or the verified official archive) when none is found. |
| `prefetch` | `false` | While online, keep the best-fitting eligible model downloaded. |
| `ramCeilingPercent` | `40` (maximum `50`) | Share of total memory one local model may take. |
| `ramHeadroomGiB` | `max(4, 10% of total)` | Free memory that must remain after loading. |
| `contextTarget` | `65536` | Preferred context length. |
| `contextFloor` | `16384` | Smallest context Overseer will run a coding agent at. |
| `preferredModels` | `[]` | Ordered tags tried before the catalogue. |
| `allowUnverifiedModels` | `false` | Let automatic picks use installed `tools` models not verified in the catalogue. |
| `localHarness` | `"opencode"` | `opencode` or `codex` (once verified). |
| `returnOnline` | `"offer"` | `offer`, `auto` or `stay`. |
| `retryCapSeconds` | `120` | Longest wait between retries. |
| `stallSeconds` | `90` | Silence while offline before Overseer interrupts a turn. |
| `probes` | `true` | Credential-free reachability probes. Off: only harness errors drive the state. |
| `ollamaIdleMinutes` | `30` | Stop an Overseer-started Ollama after this idle time. |
| `registry` | `""` | Mirror for model downloads (empty: Ollama's registry). |

## Protocol and data

- **Methods:** `connection.status`; `local.inventory`; `local.pick` (dry run: the pick and every
  rejected candidate with its reason); `local.pull`, `local.pull_cancel`; `local.install_ollama`;
  `local.cleanup` (derived tags); `settings.get`, `settings.set`; `run.handoff` (**Use a local model
  now**, **Switch back**); `run.retry_now`.
- **Events:** `connection` (state, reason, per-provider health); `local_pick` (model, context,
  estimate, measured, budget terms, rejected candidates); `local_download` (progress, done,
  cancelled, failed); `handoff` (predecessor, successor, reason); `retry` (attempt, next_in_ms,
  probe result); `status` with `waiting_for_connection` and `handed_off`.
- **Store:** `runs.predecessor_run_id`, `runs.handoff_reason`; `turns.attempts` and
  `turns.pending_prompt`; table `local_models_measured(tag, ctx, bytes, ollama_version, at)`;
  settings in `meta`.
- **Lifecycle:** `ACTIVE` gains `waiting_for_connection`; terminal states gain `handed_off`.
  Statuses still come only from real signals (AC-06); the retry scheduler and the stall interrupt are
  Overseer's own recorded actions.

## UI

Follows the Gate J principles (less text, quiet until it needs you) and the Gate K layout.

- **Status bar:** `$(eye) Overseer 2 active` gains `· $(cloud-offline) Offline` or
  `· $(warning) OpenAI unreachable`; hover shows the reason and the last probe time.
- **Side bar:** one line under the Agents header while degraded or offline; runs waiting for a
  connection show a cloud-offline icon, not the red failure icon; handed-off predecessors fold under
  their successor. The **Local (Ollama)** account row shows *3 models · budget 51 GiB*.
- **Composer and New Task:** the model chip lists local models under **Local** with a fit badge:
  *fits at 64k*, *fits at 16k*, *too big (31 GiB > 25 GiB budget)*, *not installed · 9 GB*. Offline,
  online harnesses are shown with an *offline* label and disabled with the reason.
- **Chat:** the system cards above, each once, updated in place; the details disclosure carries the
  budget arithmetic and the alternatives. Local runs show the Local provider mark (AC-65 rules: a
  neutral codicon until a licensed Ollama mark is recorded).
- **Needs you:** one global item while agents wait offline; transitions are not attention items.
- **Grid:** waiting tiles show the retry countdown quietly; local tiles carry the Local mark.

## Security and privacy

- Probes send no credentials and no payload beyond a TLS handshake or an empty `HEAD`.
- Ollama is only ever addressed at `127.0.0.1:11434`; an Overseer-started server binds loopback.
- Downloads: models from the Ollama registry (or the configured mirror) only; Ollama itself only
  with a verified Developer ID signature; nothing else is executed from a download.
- The credential rules are unchanged: no API keys, no forwarded `*_API_KEY`, `ANTHROPIC_*`,
  `OPENAI_*` or token variables. Local runs need none. Handoffs never move credentials between
  accounts.
- With a local model the repository never leaves the machine; the chat's Local mark makes that
  visible.

## Limits and out of scope

- **Offline means no downloads.** A model that is not installed cannot be fetched with no network;
  prefetch exists for exactly this. The chat says *No local model is installed that fits; downloads
  need a connection* and Overseer waits.
- **Local models are weaker.** Overseer picks the best that fits, and says which one; it does not
  claim parity with the online model.
- **Claude Code on local models** is out of scope (credential rule).
- **Quota- and rate-limit-driven routing** stays on the main RFC's later roadmap; this RFC fails over
  on unreachability and outages only.
- **Discrete GPUs** (Linux): the budget applies to system memory; VRAM-aware picks come with AC-41.
- **Other local runtimes** (LM Studio, llama.cpp servers): the inventory is written behind a small
  interface, but only Ollama is implemented.
- **Metered connections:** macOS can report an expensive path; if the daemon can read it cheaply,
  prefetch pauses on it. Not required for the gate.

## Open questions

| Question | Recommendation |
| --- | --- |
| Should `transition` default on? | On when a verified local model is already installed, otherwise off with a one-time offer the first time Overseer goes offline. Downloads and installs stay opt-in. |
| Provider order for failover | `anthropic` before `openai` as the owner's stated preference; confirm. |
| Claude Code with local models via `ANTHROPIC_BASE_URL` | No for now: it needs a token variable Overseer refuses to forward, and OpenCode already covers local. Revisit with a narrow, explicit exception if Claude Code becomes the only harness with a wanted capability. |
| Fail over on quota or rate limit too? | Not in Gate L. Add a separate criterion under quota-aware routing; the handoff mechanism will be reusable. |
| Codex `--oss` as the local harness | Verify under AC-87; if it passes, offer it as `localHarness: codex` because it keeps Codex's telemetry. |
| Derived tags versus per-request `num_ctx` | Derived tags: the OpenAI-compatible route OpenCode uses cannot carry `num_ctx`. Revisit if OpenCode gains native Ollama options. |

## Phases (goal candidates)

Each phase is independently useful and verifiable; the criteria are in the main RFC.

| Phase | Criteria | Outcome |
| --- | --- | --- |
| 1. See | AC-83, AC-85, AC-86, AC-87, AC-88 | Connection state, local inventory, budget and fit, verified catalogue, settings owned by the daemon. Nothing changes routing yet. |
| 2. Choose local | AC-89, AC-90, AC-94 | Local models are a first-class choice online, with downloads and Ollama install behind settings. |
| 3. Keep working | AC-84, AC-91, AC-92, AC-93, AC-96 | Failover, transition to local, wait and retry, back online, several local agents. |
| 4. Confirm | AC-95, AC-97 | Honest UI at every state; the owner pulls the plug during a real run and sees the transition. |

## Acceptance

AC-83 to AC-97 in the main RFC are the acceptance criteria. Their Verify clauses cover, in short:

- the three connection states from fixture-controlled probes and harness errors, with a 429 never
  counted as offline;
- failover from an unreachable provider to a working one, and to local only when nothing online works;
- the inventory, the budget formula against machine profiles (16, 32, 64 and 128 GiB), the fit table,
  and estimates within 15% of measured sizes;
- every catalogue model that this machine can run passing or failing the write check, recorded;
- settings enforced by the daemon with VS Code closed; no download or install without its setting;
- a download with progress and cancel; an Ollama install with signature verification; loopback only;
- a real transition to OpenCode + Ollama with the picked model when probes go offline, announced in
  the owner's words, in the same worktree, with the run tree showing predecessor and successor;
- wait and retry with backoff, exactly one delivery of the pending message, and the harness's own
  resume when the connection returns;
- back online: new agents default to online, Switch back and Stay local both work;
- local models offered in the composer with fit badges; several local agents sharing one model;
- the owner's dated confirmation after disconnecting during a real run.
