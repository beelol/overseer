# Side RFC: Continuity — offline mode and local models

Status: proposed by the owner on 2026-09-26; decisions taken the same day (below). Acceptance
criteria: AC-83 to AC-98 and AC-138 to AC-140 (Gate L) in the
[main RFC](../overseer-rfc.md#gate-l--continuity-offline-mode-and-local-models-added-by-the-owner-2026-09-26).
Implementation: **its own worktree and pull request** (owner, 2026-09-26). Builds on the verified OpenCode + Ollama
path ([AC-14](../verification/AC-14.md), the [Ollama log](../verification/evidence/ac-14/opencode-ollama.log)),
the account model ([account governance RFC](account-governance.md)) and usage reporting
([AC-62](../verification/AC-62.md)).

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

## The name

The mode is called **Continuity**. The owner asked for a good name and offered "transitioning",
"seamless mode" and "model reconciliation". *Seamless* overpromises (a local model is weaker, and
Overseer says so); *reconciliation* sounds like merging. *Continuity* says exactly what the mode
guarantees: the work continues across connection changes, and Overseer tells you how. In the UI it is
one switch, **Continuity**, with the caption *Keep working when the connection drops*. Settings live
under `overseer.continuity.*`. The chat still uses the owner's verb for the moment itself:
*Transitioning to qwen3-coder:30b (local) because you've disconnected.*

## Decisions from the owner (2026-09-26)

| Topic | Decision |
| --- | --- |
| Default | Continuity is **on by default**, and explained once: the first time an agent is started with it on, a one-time notice says what may happen. |
| Offline detection | Ask the system whether it is connected; confirm with probes; treat every agent failing on connection errors as evidence too. Offline is different from one provider being down. |
| Memory | Read the machine's real memory and decide from it, on every machine, not tuned to the owner's. Cross-platform where practical; on macOS the Rust daemon asks the kernel directly. |
| Models | Start with the Qwen coder family. |
| Retry | Keep retrying for **36 hours** at most. |
| Failover order | OpenAI, then Anthropic, then whatever other providers have accounts, then local. |
| Prefetch | Kept as a feature, **off until asked**: Overseer offers it once when downloads are first allowed, and only then keeps a fitting local model downloaded. |
| Downloads and Ollama install | Opt-in settings, off by default, offered in the first-use notice. |
| Permission modes | Carried over on every handoff, never loosened. The OpenCode adapter gains Plan only, Accept edits, Auto and Ask first; until Ask first is verified, such a run waits and offers the move instead. |
| Failover target | Among several accounts, the one with the most quota left, then the most recently used. The model is the one last picked for that harness, otherwise its default. |
| Back online | Local runs offer Switch back; new agents go online automatically. |
| Stall | Overseer interrupts a turn after 90 seconds of silence while offline, and a turn that only keeps reconnecting after 30 seconds with its provider unreachable. |
| After 36 hours | The run fails with the reason, keeps its message and offers Retry now. |
| No verified model fits | Wait and say why; a setting can allow unverified models. |
| Quota and rate limits | Not part of this gate; they come with quota-aware routing. |
| Claude Code on local models | No. |
| Memory safety | Overseer must never open a model that could crash the computer (AC-140). |
| Goal | One goal for all of Gate L; its first step is the spike on OpenCode's session transports (AC-139). |
| Build-time permissions | Model downloads for verification (about 28 GB), an Ollama install test in an isolated folder, light paid turns. The owner turns Wi-Fi off and on for the live checks. |
| Where it is built | In its own worktree and its own new pull request, which carries everything the goal produces (code, evidence, ledger and RFC updates). Opened early as a draft, kept up to date with main and out of conflict with the other work in flight. Nothing is pushed to main directly. |
| Other proposals in this RFC | Accepted as written (budget, handoff, catalogue verification, out-of-scope items). |

## Goal

1. **Tell an outage from being offline.** If OpenAI is unreachable and Claude works, the work goes to
   Claude. Only when the machine has no working connection is Overseer offline.
2. **Transition to a local model when offline**, with Continuity on: read the machine's memory, pick the
   best local model that fits, download it (and Ollama itself) only when the settings allow, move the
   work over, and say so: *Transitioning to qwen3-coder:30b (local) because you've disconnected.*
3. **Never fail or freeze with Continuity off.** Say that Overseer is offline, keep retrying (for up to
   36 hours), deliver the pending message exactly once, and offer local models for new agents.
4. **Stay inside a conservative memory budget.** A model never takes more than a share of total memory
   (40% by default, never above 50%), and never more than what is free right now minus headroom. The
   budget is reassessed for every new run and turn.
5. **Come back.** When the connection returns, new agents default to online again and a local agent can
   switch back to its original harness and account.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Provider | Who serves the model: `openai`, `anthropic`, any later online provider, or `local` (Ollama on this machine). |
| Connection state | One daemon-wide value: **online**, **degraded** (named providers unreachable) or **offline** (no working connection). |
| Provider health | Per provider: reachable, unreachable (with the reason) or unknown (probes off). |
| Local harness | The harness that runs local models: OpenCode with an Ollama provider (verified); Codex `--oss --local-provider ollama` (to verify). |
| Local model | An Ollama model tag installed on this machine, plus the context length Overseer runs it at. |
| Budget | The most memory one local model may take right now (see [Memory budget](#memory-budget-and-model-fit)). |
| Transition | Moving a run's work to another harness, account or model because of the connection state. |
| Handoff | The mechanism of a transition: a successor run in the same task and workspace, started with a handoff prompt, linked to its predecessor. |
| Continuity | The mode that lets Overseer transition on its own. Off means wait and retry. |

## Connection state

The daemon owns the state, because runs continue while VS Code is closed. It is one of three values
with a reason and a per-provider breakdown, changed only by evidence, and every change is an event.

```
              provider probes fail / harness network errors,
              the system and the baseline still answer
   ONLINE  ───────────────────────────────────────────►  DEGRADED (openai unreachable)
     ▲  ▲                                                     │
     │  │ system, baseline and provider agree                 │ the system reports no network,
     │  └────────────────────────────────────────────────┐    │ or the baseline fails
     │                                                   └─ OFFLINE (no working connection)
     └──────────── system, baseline and a provider answer again
```

### Three sources, in order of trust

1. **The system.** The daemon asks the operating system whether the machine is connected, the way
   the owner suggested, and listens for changes so a Wi-Fi toggle is noticed within seconds.
   - macOS: System Configuration reachability of the default route (`SCNetworkReachability` flags;
     the same answer `scutil --nwi` prints as `REACH : flags … (Reachable)` per address family, with
     `No network information` when there is none) and the presence of a default route. Change
     notifications through the SystemConfiguration callback; polling `scutil --nwi` every 5 seconds
     is the fallback.
   - Linux: NetworkManager's connectivity state (`nmcli -g CONNECTIVITY general`: `full`, `limited`,
     `portal`, `none`, `unknown`) with its D-Bus `StateChanged` signal when NetworkManager runs;
     otherwise a default route in `/proc/net/route` and carrier state in `/sys/class/net/*/operstate`.
   - One trait in `daemon/src/net.rs` with a macOS and a Linux implementation; no shelling out where
     a library call exists. Windows later.
2. **Probes.** They confirm the system's answer and see what the system cannot: broken DNS, a VPN
   that routes nowhere, a captive portal. The **baseline** is a TLS handshake with certificate
   verification to two independent well-known hosts, one by name and one by IP (so a DNS failure and
   a routing failure are told apart); a portal fails the certificate check. **Provider** probes are
   one credential-free TLS handshake or `HEAD` per provider to the hosts the harness itself uses
   (`chatgpt.com` and `api.openai.com` for Codex on a ChatGPT login; `api.anthropic.com` and
   `claude.ai` for Claude Code). Any HTTP answer, including 401 or 403, means reachable. Sustained
   5xx or 529 answers reported by the harness (two within two minutes) mark the provider
   unreachable with reason `outage`.
3. **The agents.** Error classification (`daemon/src/adapters.rs`, `classify_error`) gains a
   `network` class: `ECONNREFUSED`, `ENOTFOUND`, `EAI_AGAIN`, `ETIMEDOUT`, `ENETUNREACH`,
   `fetch failed`, `getaddrinfo`, `network is unreachable`, TLS handshake failures,
   502/503/504/529 and Codex's `willRetry` stream errors. A network-class error triggers an
   immediate system check and probe round. When every provider that active runs use fails with
   connect-level errors within two minutes, that is offline-level evidence even if the system says
   connected.

### Decision rule

| The system says | Baseline probe | Providers and agents | State and reason |
| --- | --- | --- | --- |
| no network | not needed | — | **offline** — `no network (system)`, at once |
| connected | fails | — | **offline** — `no working connection (DNS, portal or VPN)` |
| connected | passes | one provider fails | **degraded** — `openai unreachable` |
| connected | passes | every provider in use fails | **degraded** — `no working provider`; the policy treats it as offline |
| connected | probes off | every provider in use fails with connect-level errors | **offline** — `all agents lost their connection` |
| connected | probes off | one provider's agents fail | **degraded** |
| connected | passes | all pass | **online** |

- **Debounce.** A change needs two consistent readings 5 seconds apart, except the system's "no
  network", which is trusted at once. Returning to online needs the system, the baseline and the
  run's provider to agree.
- **Cadence.** The system is checked on its change notifications, every 5 seconds while any run is
  active or waiting, and every 60 seconds idle. Probes run every 30 seconds while runs are active or
  waiting, every 5 minutes idle, and immediately on a network-class error. `continuity.probes =
  false` turns probes off; the system and the agents still drive the state.
- **What is never offline.** `auth`, `rate_limit` and `quota` errors do not change the connection
  state; they are account states with their own handling (Sign in again, usage and limits). A
  provider that answers 429 is reachable.

| Example | State | Shown as |
| --- | --- | --- |
| Wi-Fi turned off | offline | `Offline` |
| Hotel Wi-Fi before the portal login | offline | `Offline · captive portal` |
| Connected, OpenAI hosts fail, Anthropic passes | degraded | `OpenAI unreachable` |
| Connected, both providers fail | degraded (policy: offline) | `Claude and OpenAI unreachable` |
| 429 or usage limit from one account | online | usage warning on that account (AC-62) |

## Policy

What Overseer does depends on the state, whether Continuity is on, and what the run needs.

| State | Run situation | Continuity on | Continuity off |
| --- | --- | --- | --- |
| degraded | the run's provider is unreachable; another online provider has a signed-in, installed harness | **fail over** to the best working provider in `continuity.providerOrder` (default OpenAI, then Anthropic, then other providers with accounts) | wait and retry the same provider |
| degraded | the run's provider is unreachable; no other online provider works | treat as offline (below) | wait and retry |
| degraded | the run's provider is fine | nothing; the state is shown | nothing |
| offline | any run with a pending or network-failed turn | **transition to a local model** that fits; a download happens only if allowed *and* the registry is reachable, so in practice a prefetched or already installed model | wait and retry; offer local models for new agents |
| offline | no eligible local model (none installed, none fits, downloads off) | wait and retry, and say why no local model was used | wait and retry |
| back online | local or failed-over runs | finish the current turn; then per `continuity.returnOnline` (default: offer **Switch back**; new agents default to online) | resume the original harness through its own resume path |

Two rules hold in every cell:

- **Work is never dropped.** A pending user message is kept with the run and sent exactly once, by
  whichever harness ends up doing the turn.
- **Nothing is silent.** Every transition, retry schedule and return is a system card in the chat and
  an event in the log, with the reason and the time.

## Local inventory

The daemon reports what the machine can do. Unknown stays unknown; nothing is estimated as zero.

### Memory, read from the machine

The budget must work on every machine, so the daemon reads real numbers rather than assuming any.
`daemon/src/sys.rs` defines one `Memory` trait (`total`, `available`, `pressure`) with a platform
implementation behind it:

| Platform | Total | Available now | Pressure |
| --- | --- | --- | --- |
| macOS (Rust asks the kernel directly) | `sysctl hw.memsize` | `host_statistics64(HOST_VM_INFO64)`: free + inactive + purgeable pages × the page size, the numbers `vm_stat` prints | `sysctl kern.memorystatus_vm_pressure_level` (normal, warning, critical), with `kern.memorystatus_level`, the system's own free percentage, reported beside it |
| Linux | `/proc/meminfo` `MemTotal` | `/proc/meminfo` `MemAvailable` | `/proc/pressure/memory` (PSI) |
| Windows | later | later | later |

As built, both platforms read the numbers directly (`daemon/src/sys.rs`), with no extra crate; the
macOS numbers are tested against `vm_stat` and `sysctl` and sit within 5% of them. Available memory is sampled at every
pick and smoothed over three samples, so a momentary dip does not flip a decision. Apple silicon has
unified memory, so RAM is the GPU memory; discrete GPUs on Linux come with AC-41.

### Ollama and models

| Fact | Source | Notes |
| --- | --- | --- |
| Ollama installed | `/Applications/Ollama.app`, `ollama` on PATH (`/usr/local/bin`, `/opt/homebrew/bin`); Linux `ollama` on PATH or the systemd unit | version from `ollama --version` |
| Ollama running | `GET http://127.0.0.1:11434/api/version` | Overseer never assumes a non-loopback host |
| Installed models | `GET /api/tags` | name, size on disk, family, parameter size, quantization, context length, capabilities (`tools`, `thinking`, `vision`) |
| Model geometry | `POST /api/show` | `block_count`, `head_count_kv` (scalar or per-layer array), `key_length`, `value_length`, `embedding_length`, `head_count`, `parameter_count` |
| Loaded models and real memory | `GET /api/ps` | `size` and `size_vram` of each loaded model at its `context_length`; the measured number Overseer trusts most |
| Disk free for downloads | `statvfs` on Ollama's models directory | a pull is refused when the model plus 10% would not fit |

Calibration point (this machine, 2026-09-26): 128 GiB total, Ollama 0.34.2 running, and
`qwen3-coder:30b-64k` loaded at 23.7 GiB for a 65,536-token context against 17.3 GiB on disk. The
owner's "28 GB" in the request is this 128 GiB machine; nothing in the design depends on it.

## Memory budget and model fit

### The budget

```
ceiling_share   = total × continuity.ramCeilingPercent / 100     default 40%, hard maximum 50%
ceiling_now     = available_now − headroom                        headroom = max(4 GiB, 10% of total)
ceiling_level   = total × (system_free_level − 45) / 100          where the system gives a level (macOS)
budget          = min(ceiling_share, ceiling_now, ceiling_level)
```

- **Why 40%.** An idle machine already uses 10–20% of its memory; VS Code, the harness, a browser and
  the build take more. 80% is where paging starts. One model at 40% leaves room for everything else
  and for a second, smaller model if needed. The owner may raise it to 50%; the daemon refuses more.
- **Why two terms.** The share protects the machine from the model; the free-memory term protects the
  model from everything else running right now. Either alone is wrong: 40% of 128 GiB is 51 GiB, but
  with Xcode and Docker holding 90 GiB the honest answer is a much smaller model.
- **Why a third term (added 2026-09-26, from a live run).** "Available" counts pages the system
  could give back, and on a busy machine that overstates what a model can take. With another
  session's 24 GiB model loaded, an emulator and two builds running, this machine read 30.5 GiB
  available and the two terms allowed 17.7 GiB; a 14.2 GiB load then took the system to its
  warning level (its own free level fell from 47% to 26%), and the check stopped and unloaded.
  macOS gives its own account of free memory (`kern.memorystatus_level`), and it reported pressure
  from about 40% down. The budget now also keeps that level at 45% or more: the same load is
  refused with the numbers (2.6 GiB would have been left). On an idle 128 GiB machine (75% free by
  the system's count) this term is 38.4 GiB and it decides, under the 40% share. Where the system
  gives no level (Linux today), the two terms decide alone.
- **Reassessment.** The budget is computed for every new run and every new turn (OpenCode starts one
  process per turn, and the model choice is per turn), never in the middle of a turn. A working turn
  is never stopped for a tighter budget (only critical memory pressure pauses local runs, see
  [Memory safety](#memory-safety-ac-140)). If the budget shrank, the next turn uses a smaller model or a
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
  exists, and is recorded (`local_models_measured`) so later picks use real numbers. The estimate for
  `qwen3-coder:30b` at 64k is 24.3 GiB against 23.7 GiB measured.
- **Context is the lever.** A model is tried at the target context (`continuity.contextTarget`,
  default 65,536, or the model's own maximum when that is smaller), then at half of it, down to the
  floor (`continuity.contextFloor`, default 16,384): 64k, 32k, 16k.
  Below 16k a coding agent cannot hold the tool schema, the handoff and a few files, so the model is
  skipped instead.
- **Ranking.** A model that only fits below 32k is considered after every model that fits at 32k or
  more. Then the catalogue tier (the owner's `preferredModels` first, in the owner's order); within
  a tier what is installed before what needs a download, then the longest context that fits, then
  the larger model.

### Worked examples (40% ceiling, memory otherwise free)

Machine profiles, not one machine. Estimates from the formula; disk sizes from Ollama (approximate
for models not measured here). The order follows the ranking above: a model that fits at 32k comes
before a larger one that only fits at 16k, because OpenCode's own instructions and tool list
already take about 10,600 tokens (measured in the spike), which leaves little of a 16k context.
The rows assume every model has passed its check. **They have not:** the verification of
2026-09-26 passed `qwen3-coder:30b` and failed every `qwen2.5-coder` size (see
[Model catalogue](#model-catalogue)), so on its own Overseer picks `qwen3-coder:30b` or nothing.
A machine whose budget is under about 20 GiB (the 16 and 32 GiB rows) has no eligible model
today: a run there waits and says why, unless the owner turns on `allowUnverifiedModels`. (Corrected on 2026-09-26 while building the pick: the first version
of this table listed the 16k fits first and gave 128k for the last row, against the rule and the
64k target.)

| Total memory | Budget | Picks, in order | Not chosen |
| --- | --- | --- | --- |
| 16 GiB | 6.4 GiB | `qwen2.5-coder:3b` at 32k (≈3.9 GiB); `qwen2.5-coder:1.5b` at 32k; then `qwen2.5-coder:7b` at 16k (≈6.3 GiB) | `7b` at 32k (≈7.2 GiB) |
| 32 GiB | 12.8 GiB | `qwen2.5-coder:7b` at 32k (≈7.2 GiB); `qwen2.5-coder:3b` at 32k; then `qwen2.5-coder:14b` at 16k (≈12.4 GiB) | `14b` at 32k (≈15.4 GiB) |
| 64 GiB | 25.6 GiB | `qwen3-coder:30b` at 64k (≈24.3 GiB, measured 23.7) | `qwen2.5-coder:32b` at 32k (≈27.6 GiB) |
| 128 GiB | 51.2 GiB | `qwen3-coder:30b` at 64k (≈24.3 GiB; at 128k, ≈30.3 GiB, when `contextTarget` is raised to 131072); `qwen2.5-coder:32b` at 32k | anything over 51 GiB, for example `qwen3.5:122b` (75.8 GiB on disk) |

With 40 GiB of other work running on a 128 GiB machine, `ceiling_now` is about 75 GiB, so the share
still decides; with 100 GiB in use it is about 15 GiB and the pick drops to a 14B-class model at 16k,
exactly as on a 32 GiB machine.

### Context length in Ollama

Ollama's default context is small and per model. Overseer sets the context it picked by creating a
derived tag with a two-line Modelfile (`FROM qwen3-coder:30b` / `PARAMETER num_ctx 65536`), named
`overseer/qwen3-coder-30b:64k` (the context is the tag itself: Ollama turns a name without a tag
into `<name>:latest`, which the first live run showed). `ollama create` shares the weight blobs, takes no extra disk and is
instant; the owner's own `-64k` and `-32k` tags were made the same way. Derived tags are listed under
the base model in the UI and removed with **Clean up local models**. When Overseer starts the Ollama
server itself, `OLLAMA_CONTEXT_LENGTH` is the fallback.

## Memory safety (AC-140)

The owner's condition: Overseer must never open a model that could crash the computer.

- **No path around the budget.** A model whose measured or estimated size is above the budget cannot
  be started from Overseer at all: not by an automatic pick, not from the composer, not by prefetch,
  not during catalogue verification. It is shown as *too big* with the numbers, and this gate has no
  override. On a 128 GiB machine that includes `qwen3.5:122b` (75.8 GiB on disk), even though it
  is installed.
- **Fresh numbers before every load.** The budget is recomputed from new memory readings immediately
  before a load, never reused from an earlier pick.
- **Load watchdog.** While a model loads, memory is sampled every second. If available memory falls
  under half the headroom, or the system reports critical memory pressure, Overseer cancels the
  load, unloads the model (`keep_alive: 0`) and says so; the run waits or takes a smaller pick.
- **Critical pressure valve.** If the system reports critical memory pressure while local runs are
  working, Overseer pauses them (the same interrupt as Stop, message kept, state
  `waiting_for_memory`) and unloads the model; they resume when the pressure is back to normal. This
  is the only case in which Overseer stops a working local turn.
- **One model at a time during verification**, smallest first, each unloaded after its check, with
  memory recorded before and after. A verification stops at once when the system reports
  pressure, and starts no model while it does.
- **Out of reach.** Overseer cannot stop a model the user starts in Ollama themselves. It sees that
  model in `/api/ps` and in available memory, counts it as used, and shrinks its own pick.

## Model catalogue

Overseer ships a small catalogue of coding models known to work as agents. The owner's direction is
to **start with the Qwen coders**; other families are candidates for later. A model is **eligible**
for automatic picks only when Ollama reports the `tools` capability *and* the catalogue marks it
verified with the local harness; `continuity.allowUnverifiedModels` widens that to any installed
model with `tools`. Ranking within a tier is by parameter count.

| Tier | Model (Ollama tag) | Disk | Result on 2026-09-26 (OpenCode 1.15.13 through `opencode serve`, Ollama 0.34.2) |
| --- | --- | --- | --- |
| 1 | `qwen3-coder:30b` (MoE, 3B active) | 17.3 GiB | **passed**, 3 of 3, at a 64k context (23.7 GiB loaded), 22 to 28 s a turn |
| 1 | `qwen2.5-coder:32b` | 18.5 GiB | **failed**, 0 of 3, at 32k (26.5 GiB loaded): wrote its tool calls as text |
| 2 | `qwen2.5-coder:14b` | 8.4 GiB | **failed**, 0 of 3, at 32k: tool calls as text, or "done" with nothing written |
| 3 | `qwen2.5-coder:7b` | 4.4 GiB | **failed**, 0 of 3, at 32k: said "done" without calling a tool |
| 4 | `qwen2.5-coder:3b` | 1.8 GiB | **failed**, 0 of 3, at 32k: tool calls as text |
| 4 | `qwen2.5-coder:1.5b` | 0.9 GiB | **failed**, 0 of 3, at 32k: tool calls as text |

The whole `qwen2.5-coder` family fails the same way through OpenCode: the model writes the call
(`{"name": "write", "arguments": …}`) into its reply instead of calling the tool, so nothing runs,
and the smaller sizes sometimes reply "done" with nothing written. Overseer's one nudge does not
change it. They are excluded from automatic picks and shown as unverified; a user may still name
one. What this means for small machines is said under
[Worked examples](#worked-examples-40-ceiling-memory-otherwise-free), and a family that does call
tools at 8 to 16 GiB is the first thing to look for next ([log](../verification/evidence/ac-87/opencode-serve.txt)).

Later candidates, added only after the Qwen coders are verified: `qwen3.5:35b-a3b` (installed here;
tools, thinking, vision), `gpt-oss:20b` (Codex's own `--oss` default) and `gpt-oss:120b`,
`devstral:24b`, `qwen3:8b`.

The verification check is the same for every entry: a one-turn task through the real local harness
that must create a file with the `write` tool, in a disposable repository. The result (pass, or the
exact failure) is recorded in the catalogue file (`daemon/src/local_catalogue.json`) with the Ollama
and harness versions, and in the ledger. Tiers are a proposal until AC-87 confirms them; the owner
can reorder with `continuity.preferredModels`.

## Local harness

| Harness | How | Status |
| --- | --- | --- |
| **OpenCode** | Through `opencode serve` (harness id `opencode-serve`, chosen by the spike below). The `local` account's `opencode.json` (in its own `XDG_CONFIG_HOME`) gets an `ollama` provider (`@ai-sdk/openai-compatible`, `baseURL http://127.0.0.1:11434/v1`) listing the derived tags with `tool_call: true`; `enabled_providers: ["ollama"]`; `model` and `small_model` point at the pick; `autoupdate: false`, `share: disabled`. The model and agent are chosen per prompt. | transport verified by the spike; the bridge and the config writer are new |
| **Codex** | `codex exec --oss --local-provider ollama -m <tag>` (flags present in the installed 0.155 binary). Keeps Codex's file-change and child telemetry with a local model. | to verify (AC-87); second choice until then |
| **Claude Code** | Would need `ANTHROPIC_BASE_URL` and a placeholder token in the harness environment. Overseer never forwards `ANTHROPIC_*` or token variables (AC-16), so this is **not in scope**; see [Open questions](#open-questions). | out of scope |

### The spike comes first (AC-139)

Done on 2026-09-26 with OpenCode 1.15.13, Ollama 0.34.2 and `qwen3-coder:30b-64k` (23.7 GiB
loaded, inside a budget of 42.7 GiB at the time), in an isolated OpenCode profile and two
disposable repositories. Drivers: [`test/spike/opencode-serve.js`](../../test/spike/opencode-serve.js)
and [`test/spike/opencode-acp.js`](../../test/spike/opencode-acp.js). Evidence:
[`docs/verification/evidence/ac-139/`](../verification/evidence/ac-139/).

**Decision: local runs use `opencode serve`.** It answers every question below. `opencode acp` is
not used: it cannot interrupt a running turn, and a turn that delegated to a child never finished.

| Question | `opencode serve` (HTTP and an event stream on loopback) | `opencode acp` (JSON-RPC on stdin and stdout) |
| --- | --- | --- |
| Start a session with a model and an agent | `POST /session` with `agent`, `model` and `permission` rules; the model and agent can also change per prompt | `session/new`, then `session/set_config_option` for `model` and `mode` |
| Send a prompt | `POST /session/{id}/prompt_async` (answers 204 at once; the turn reports through events) | `session/prompt` (answers when the turn ends) |
| A permission request | event `permission.asked`: id, session, `permission` (`edit`, `bash`), patterns, and for edits the file path and a diff; also listed by `GET /permission` | request `session/request_permission` with the tool call, its diff and three options |
| Allow and Deny | `POST /permission/{id}/reply` with `once` or `reject`; nothing is written before the answer; Deny blocks the write | answer with the option `once` or `reject`; same results |
| Rules per session | yes, in `POST /session`; no file is written | no, only from the profile's configuration |
| Interrupt | `POST /session/{id}/abort`: a 45 s command ended 56 ms later, the message carries `MessageAbortedError`, and the session took the next prompt | **none**: `session/cancel` answers "Method not found" as a notification and as a request, and the 45 s command ran to its end |
| Resume after a restart | a restarted server still has the session and continues it with its history | `session/load` in a new process works |
| Child sessions | event `session.created` with `parentID`, their events on the same stream, `GET /session/{id}/children` | only an id inside the task tool call; a turn that delegated in plan mode never finished (300 s) and no request reached the client |
| File activity and usage | tool parts with the file path, `file.edited`, `session.diff`; tokens and cost on the assistant message | tool call updates; token counts on the prompt result |
| Several worktrees | one server serves any directory (`?directory=`); events and pending requests are scoped to it, `/global/event` carries all | one process per directory |

What the adapter must do, learned the hard way:

- **Rules travel with the session**, so Overseer writes no rules into any file. The profile's
  configuration only names the local provider.
- **Only `once` and `reject` are sent.** A `once` answer is not remembered: the next command in
  the same turn asks again, which is what Ask first means. `always` is never sent.
- **The `question` tool is denied** by a session rule. Without it a model may ask the user a
  multiple-choice question and the session waits for an answer Overseer has no card for.
- **Providers are pinned.** A fresh profile still offers eight online OpenCode Zen models.
  Overseer's profile sets `enabled_providers: ["ollama"]` and points `model` and `small_model` at
  the local pick, so a local run can never go online, not even to name a session.
- **A tool call written as text is a failure to detect.** Once in the 49 prompts of the spike
  the verified model wrote `<function=bash>…` as plain text and the turn ended without running
  anything. The adapter
  reports it as `tool_call_as_text` and repeats the turn once; the catalogue check (AC-87) runs
  each model three times.
- **Interrupting is a request, not a signal.** The run's process stays up between turns and after
  an interrupt, as with `codex-app` and Claude Code.

**How it fits the daemon (as built).** A new harness id, `opencode-serve`, beside the unchanged one-shot
`opencode`. The run's supervisor starts a small bridge, a subcommand of `overseerd`
(`overseerd opencode-bridge`), which starts `opencode serve` on a loopback port of its own, prints
every server event as one JSON line on its standard output, and turns lines on its standard input
(prompt, permission answer, interrupt) into requests. The daemon's existing tail, parser and
control path stay as they are; the bridge needs only a plain HTTP client for loopback. One bridge
and one server per turn, like every other harness, so a turn's process tree is stopped as a unit
and a crash affects one run; the session itself lives in the profile and is continued by the next
turn's server. The bridge picks a free port itself and puts the server behind a password only it
knows, so no other process on the machine can drive the agent. It also does the remembering the
daemon's line parser cannot: which messages are the user's own, which sessions are this run's
children, and which belong to something else on the same server.

Before a local turn is launched the daemon chooses the model if none was named, gives it a context,
passes it through the memory guard, loads it under the watchdog, and writes the profile's
configuration. A refusal ends the run as failed with its reason; nothing is launched.

Fixtures recorded from the chosen transport, for the adapter tests:
`fixtures/transcripts/opencode-1.15.13-serve-{allow,deny,interrupt,children}-local.jsonl`.

### Permission modes carry over (AC-138)

A handoff keeps the run's permission mode and never loosens it. Local models have nothing to do
with this: OpenCode has its own permission system (allow, ask and deny rules per tool), a built-in
`plan` agent that denies edits, and two session transports a client can answer requests over
(`opencode serve` and `opencode acp`; the spike chose the first). The gap is in Overseer's adapter, which uses the one-shot
`opencode run` transport: it passes no agent and no rules, and it has no channel for a permission
request to come back, so asks are rejected. The same was true of Codex before the app-server
transport ([compatibility](../compatibility.md)).

| The run's mode | Claude Code | Codex | OpenCode (local), after this gate |
| --- | --- | --- | --- |
| Plan only | `--permission-mode plan` | sandbox read-only | the `plan` agent (edits denied) |
| Ask first | `--permission-mode manual` with the permission prompt tool | `codex-app` approval requests | session rules `edit: ask`, `bash: ask`; requests appear as Allow and Deny cards in the chat |
| Accept edits | `--permission-mode acceptEdits` | sandbox workspace-write | session rules `edit: allow`, `bash: ask` |
| Auto | `--permission-mode auto` | sandbox workspace-write | the `build` agent as shipped (it still asks before leaving the worktree) |

- **Rules travel with the session** (`POST /session`), and every mode also denies the `question`
  tool. Overseer writes no rules into any file, and the user's own OpenCode configuration is never
  edited or read.
- **Never looser.** If the target cannot honour the run's mode, Overseer does not transition on its
  own: the run waits (as with Continuity off) and the chat offers the move with the difference
  stated, for example *The local agent cannot ask before running commands yet. Continue locally in
  Accept edits?*
- **Settled by the spike (AC-139).** All four modes carry over through `opencode serve`. The
  fallback above remains for a machine whose OpenCode is too old to have the server, or where the
  server does not start.

The local account is the existing `local` provider ("OpenCode (local models)", `daemon/src/accounts.rs`),
renamed **Local (Ollama)**, created automatically when Ollama is found, with no sign-in. Its status
row shows the Ollama version, the number of installed models and the current budget.

## Downloads and installing Ollama

Downloading needs a connection, so it happens while online or degraded, never offline. Two settings
gate it, both off by default and both offered in the first-use notice.

**Models** (`continuity.allowModelDownloads`):
- `POST /api/pull` with streaming progress; a system card in the chat shows *Downloading
  qwen2.5-coder:14b · 3.2 of 9.0 GB* with **Cancel**. Partial downloads resume.
- Disk space is checked first; a refused pull says how much is missing.
- The first pull ever asks once in a dialog with the size; later pulls rely on the setting.
- **Prefetch** (`continuity.prefetch`, off until asked): while online, keep the best-fitting
  eligible model for this machine downloaded, so going offline works without a download. Overseer
  offers it once, when downloads are first allowed, naming the model and its size (*Keep
  qwen3-coder:30b ready for offline? 17 GB*); declining leaves it off, and it can be turned on later
  in settings. When on, it is recomputed when the catalogue, the settings or the machine's memory
  change, and never runs while a paid run is streaming.

**Ollama** (`continuity.allowOllamaInstall`):
- Homebrew when present (`brew install --cask ollama`); otherwise the official macOS archive from
  `ollama.com/download`, whose code signature must verify as Ollama's Developer ID (`codesign
  --verify` and `spctl --assess`) before it is opened. A failed check deletes the download and
  reports it.
- Overseer starts `ollama serve` itself only when nothing answers on `127.0.0.1:11434`, as a
  supervised child bound to loopback, and stops it after `continuity.ollamaIdleMinutes` (default 30)
  without local runs. An Ollama the user runs (the menu-bar app) is used as is and never stopped.
- As built: the archive is unpacked into Overseer's own folder (`<data>/ollama/Ollama.app`), never
  into `/Applications`; the user's own copy is preferred when there is one. Starting the server
  needs the same setting as installing. The server is kept by its process id in the daemon's store
  and outlives a restart of the daemon, as the agents do; before it is stopped the process is
  checked to be that program still. An install that a waiting run needs happens in the background,
  and the run looks again when it is due.
- Linux uses the distribution package or the official install script, behind the same setting
  (AC-41 remains deferred).

Downloads come from the Ollama registry only (`registry.ollama.ai`, or `continuity.registry` for a
mirror). Nothing is executed from a download except the verified Ollama application.

## Transition (handoff)

A handoff moves work from a **predecessor** run to a **successor** run in the same task and the same
workspace. It is the one mechanism behind failover, going local, and switching back.

1. **Stop the predecessor's turn** if it is still running (the same interrupt the user's Stop uses);
   a turn that already failed with a network-class error needs nothing.
2. **Snapshot** the workspace (a run-start snapshot, as for any turn), so the review's *Latest run*
   base is the moment of the handoff and the successor's edits show on their own.
3. **Pick** the target: the best working online provider's harness and a signed-in account, or the
   local harness with the budget's model (the pick and its reasons are an event). Among several
   accounts of that provider, the one with the most quota left in usage reporting (AC-62), then the
   most recently used; the model is the one last picked for that harness in the composer, otherwise
   the harness default.
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
   (model per turn) and its permission mode mapped as in
   [Permission modes carry over](#permission-modes-carry-over-ac-138), never loosened, `predecessor_run_id` set and `handoff_reason` recorded
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

As built (2026-09-26):

- A failed turn does not end its run. The run is **parked** (`waiting_for_connection`, or
  `waiting_for_memory` after the critical-pressure valve) with its message kept, and one scheduler
  looks at every parked run when it is due, and at once when the connection state has changed.
- A sign-in failure (`auth` class) while the state is offline is treated as the connection's
  failure: a sign-in cannot be checked without a connection.
- `run.targets` says where a run's work could go now without moving anything; `run.handoff` moves
  it (`to`: `local`, `back` or a provider). A move whose permission mode would be looser needs
  `accept_mode` with the mode named, so the difference is accepted knowingly.
- An OpenCode without the headless server (`opencode serve --help` does not describe the command)
  can only run one-shot, where nothing can be asked. That is looser than every mode but Auto, so
  such a move is offered with the difference stated and is automatic only for a run already in
  Auto. It passes the memory guard like every local run.

## Wait and retry (Continuity off, or no target)

- The failed turn's run becomes **`waiting_for_connection`**, a new active lifecycle state. It is
  entered only on a network-class turn failure or a stall while the state is offline (below); it never
  comes from silence alone (AC-06).
- **Backoff.** Retry after 5 s, then doubling to a cap of 2 minutes (`continuity.retryCapSeconds`),
  with ±20% jitter, for at most **36 hours** (`continuity.retryForHours`, owner decision), or until
  the user stops the run. Each attempt is preceded by a check: the harness is relaunched only when
  the system says connected and the probe for its provider passes, so paid accounts are not hammered
  with doomed requests. Each attempt is an event (`retry`, attempt n, next in s).
- **After 36 hours** the run is marked `failed` with the reason *no connection for 36 hours*; it keeps
  its pending message and offers **Retry now** and **Use a local model now**, and only then becomes a
  Needs-you item, because a decision is needed.
- **Exactly once.** The pending prompt is stored on the turn; a relaunch resumes the harness's own
  session (`--resume`, `exec resume`, `--session`) and sends that prompt. A turn that already produced
  a `turn_done` is never re-sent.
- **Stall.** A harness that keeps retrying internally shows no error. If the state is offline and a
  running turn has produced no event for `continuity.stallSeconds` (default 90), Overseer interrupts
  it (recorded as Overseer's action, not the harness's) and applies the policy as if the turn had
  failed.
- **Reconnecting in vain** (as built, from the live check). The real Codex never fails such a turn:
  with its hosts unreachable it prints *Reconnecting... waiting for network* every few seconds for as
  long as it is left, so neither the failed-turn rule nor the silence rule ever fires. When a run's
  provider is unreachable (the state is offline, or degraded with that provider's probe failing) and
  the run has produced nothing but network-class errors since its last progress for 30 seconds
  (`OVERSEER_TEST_RECONNECT_MS` in tests; never longer than `stallSeconds`), Overseer interrupts the
  turn the same way. The `stall` event carries the reason, and the parked run reads *no progress
  while OpenAI could not be reached; the turn was interrupted by Overseer*. A turn that reconnects
  while its provider still answers the probe is left alone.
- **Shown as** one quiet system card, updated in place: *Offline. Retrying the connection every 2
  minutes (attempt 4). Nothing is lost; your message is sent when the connection returns.* with
  **Use a local model now** and **Stop**. Runs in this state are not dimmed and are not per-run items
  in Needs you; one global Needs-you item reads *Offline · 3 agents waiting* with **Use local models**
  and **Keep waiting**. New agents started while offline offer local models only, with the online
  harnesses labelled *offline*.

## Back online

- Local and failed-over runs **finish their current turn**; nothing is interrupted to switch back.
- **New agents** default to their online harness and account again as soon as the state is online.
- Per `continuity.returnOnline`: `offer` (default) shows *Back online. This agent is still on a local
  model.* with **Switch back to Claude Code** and **Stay local**; `auto` switches back at the next
  turn and says so; `stay` keeps local runs local. Switching back is a handoff (reason `back_online`)
  whose prompt summarises the local work; when the original harness's session still exists, the
  successor resumes it instead of starting fresh.
- Runs in `waiting_for_connection` resume on their next scheduled attempt, at once when the check
  passes.

## On by default, explained once

Continuity is on by default. Because it can change which model does the work and may download
gigabytes once downloads are allowed, Overseer explains it **once per machine**, the first time an
agent is started with Continuity on, while the user is looking, not at the moment of a failure:

> **Continuity is on.** If the connection drops, Overseer keeps this work going on another provider or
> on a local model, and says so in the chat. Model downloads: off · Ollama install: off.
> **Got it** · **Allow downloads** · **Settings**

- One card above the composer (and in New Task); dismissing it records `continuity.notice_shown` in the
  daemon's `meta` table, so it appears once across windows and reinstalls.
- Turning Continuity off later is the one **Continuity** switch in the Accounts view and in settings.
- The first actual transition is announced in the chat as described above; the notice is not repeated
  there.

## Several local agents

- Ollama keeps one copy of a loaded model; two local runs on the same tag share it and Ollama
  serialises or parallelises their requests (`OLLAMA_NUM_PARALLEL`). Overseer shows *Queued behind 1
  local agent* on the waiting tile rather than pretending both stream.
- A second, different model loads only if the sum of both fits the budget; otherwise the new run uses
  the already-loaded model when it is eligible, or waits with *Waiting for local model* shown.
- If available memory falls under the headroom while a model is loaded, no turn is stopped for it
  (only the critical-pressure valve in [Memory safety](#memory-safety-ac-140) pauses runs); the next
  turn's pick shrinks (smaller context first, then a smaller model) with one note in the chat.
- A smaller copy can only be loaded after the larger one is unloaded. Overseer unloads a copy only
  when it loaded that copy itself (it keeps a record of its own loads) and no other local agent is
  working on it. Any other copy, one the user loaded in their own Ollama or one another agent is
  working on, is shared as it is, which loads nothing.
- Local runs report usage as tokens with cost 0 and the provider mark **Local**; no quota windows.

## Settings

The daemon owns and enforces these, because the policy must work with VS Code closed. VS Code
settings are the editing surface; the extension pushes them to the daemon (`settings.set`), which
persists them in its `meta` table and validates ranges. `overseerd ctl continuity.status` prints the
state, the budget and the current pick.

| Setting (`overseer.continuity.*`) | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` | Continuity: fail over and go local on its own. Off: wait and retry. |
| `providerOrder` | `["openai", "anthropic"]` | Preference among working providers for failover; providers with accounts that are not listed follow in the order their accounts were added; `local` is always last. |
| `allowModelDownloads` | `false` | Pull models from the Ollama registry when a pick or prefetch needs one. |
| `allowOllamaInstall` | `false` | Install Ollama (Homebrew or the verified official archive) when none is found. |
| `prefetch` | `false` | While online and downloads are allowed, keep the best-fitting eligible model downloaded. Offered once when downloads are first allowed. |
| `ramCeilingPercent` | `40` (maximum `50`) | Share of total memory one local model may take. |
| `ramHeadroomGiB` | `max(4, 10% of total)` | Free memory that must remain after loading. |
| `contextTarget` | `65536` | Preferred context length. |
| `contextFloor` | `16384` | Smallest context Overseer will run a coding agent at. |
| `preferredModels` | `[]` | Ordered tags tried before the catalogue. |
| `allowUnverifiedModels` | `false` | Let automatic picks use installed `tools` models not verified in the catalogue. |
| `localHarness` | `"opencode"` | `opencode` or `codex` (once verified). |
| `returnOnline` | `"offer"` | `offer`, `auto` or `stay`. |
| `retryCapSeconds` | `120` | Longest wait between retries. |
| `retryForHours` | `36` | Give up waiting after this long (the run then fails with the reason and offers Retry now). |
| `stallSeconds` | `90` | Silence while offline before Overseer interrupts a turn. A turn that only reconnects with its provider unreachable is interrupted after 30 seconds, or after `stallSeconds` when that is shorter. |
| `probes` | `true` | Credential-free reachability probes. Off: the system and the agents drive the state. |
| `ollamaIdleMinutes` | `30` | Stop an Overseer-started Ollama after this idle time. |
| `registry` | `""` | Mirror for model downloads (empty: Ollama's registry). |

## Protocol and data

- **Methods:** `connection.status`; `local.inventory`; `local.pick` (dry run: the pick and every
  rejected candidate with its reason); `local.pull`, `local.pull_cancel`; `local.install_ollama`;
  `local.cleanup` (derived tags); `settings.get`, `settings.set`; `run.handoff` (**Use a local model
  now**, **Switch back**); `run.retry_now`.
- **Events:** `connection` (state, reason, system answer, per-provider health); `local_pick` (model,
  context, estimate, measured, budget terms, rejected candidates); `local_download` (progress, done,
  cancelled, failed); `handoff` (predecessor, successor, reason); `retry` (attempt, next_in_ms, check
  result); `status` with `waiting_for_connection` and `handed_off`.
- **Store:** `runs.predecessor_run_id`, `runs.handoff_reason`; `turns.attempts` and
  `turns.pending_prompt`; table `local_models_measured(tag, ctx, bytes, ollama_version, at)`;
  settings and `continuity.notice_shown` in `meta`.
- **Lifecycle:** `ACTIVE` gains `waiting_for_connection` and `waiting_for_memory`; terminal states
  gain `handed_off`.
  Statuses still come only from real signals (AC-06); the retry scheduler, the 36-hour limit and the
  stall interrupt are Overseer's own recorded actions.
- **Platform modules:** `daemon/src/net.rs` (connection, one trait, macOS and Linux) and
  `daemon/src/sys.rs` (memory, one trait, macOS and Linux). Both keep the daemon's rule of no
  macOS-only API outside a platform boundary (AC-04).

## UI

Follows the Gate J principles (less text, quiet until it needs you) and the Gate K layout.

- **Status bar:** `$(eye) Overseer 2 active` gains `· $(cloud-offline) Offline` or
  `· $(warning) OpenAI unreachable`; hover shows the reason, what the system said and the last check.
- **Side bar:** one line under the Agents header while degraded or offline; runs waiting for a
  connection show a cloud-offline icon, not the red failure icon; handed-off predecessors fold under
  their successor. The **Local (Ollama)** account row shows *3 models · budget 51 GiB*; the
  **Continuity** switch sits in the Accounts view.
- **Composer and New Task:** the model chip lists local models under **Local** with a fit badge:
  *fits at 64k*, *fits at 16k*, *too big (31 GiB > 25 GiB budget)*, *not installed · 9 GB*. Offline,
  online harnesses are shown with an *offline* label and disabled with the reason. The first-use
  notice appears here once.
- **Chat:** the system cards above, each once, updated in place; the details disclosure carries the
  budget arithmetic and the alternatives. Local runs show the Local provider mark (AC-65 rules: a
  neutral codicon until a licensed Ollama mark is recorded).
- **Needs you:** one global item while agents wait offline; a per-run item only after the 36-hour
  limit; transitions are not attention items.
- **Grid:** waiting tiles show the retry countdown quietly; local tiles carry the Local mark.

As built (2026-09-26), in `extension/src/continuity.js` (the host), `extension/media/continuity.js`
(the webviews) and `extension/media/continuity-text.js` (the words, shared with the unit tests):

- **Status bar:** its own item, `$(cloud)` alone while online; `$(cloud) OpenAI unreachable` while
  degraded; `$(cloud) Offline · 2 waiting` on the warning background while offline. Its tooltip is
  the sentence, what the system said, each provider's health and what Continuity does now. It opens
  a pick with the connection, the Continuity switch, the waiting agents, Check now, Local models,
  the two Allow switches and the settings.
- **Side bar:** the Agents view's message says *Overseer is offline: no network (system). 1 agent
  waiting.* while not online, and nothing while online. Waiting agents keep their provider's mark
  with a `☁` badge; a handed-off agent folds under the agent that took over as *Earlier: Codex ·
  handed off · the connection was lost*. One Needs-you row, *2 agents waiting for a connection*,
  stands for every waiting agent. The Continuity switch is in the status bar's pick and in the
  settings, not in Accounts.
- **Composer and New Task:** the Agent menu ends with **Local models · Ollama**: *Best fit ·
  qwen3-coder:30b*, then each installed model with its badge (*fits at 64k*, *fits at 32k · failed
  its check*, *too big · 77.2 GiB of 51.2 GiB*), and the models the catalogue knows but that are
  not installed behind one entry. The badge is the guard's own answer (`local.models`). Offline, the
  online agents are disabled with the reason, a line above the field says so, and one click moves
  to the best local model; back online, the last online agent is the default again. A local agent
  needs no account. The first-use notice sits above the field, once per machine, with the two Allow
  switches and *Turn Continuity off*.
- **Chat and tiles:** the announcements are one quiet line each. A waiting agent shows one card:
  the title, *Your message is kept*, *Next check in 40 s · waiting 2 min · gives up after 36 hours*,
  and **Use a local model now** (or *Continue with Claude Code*), **Retry now**, **Stop**; a move
  that would ask less often is offered with the difference and made after a confirmation. Back
  online, a local or failed-over agent shows **Switch back to Codex** and **Stay here**.
  When downloads are first allowed, one message offers to keep the best-fitting model ready
  (*Keep qwen3-coder:30b ready for offline? (17.3 GiB download)*); Not now leaves prefetch off, and
  the daemon records the offer so it is made once. Downloads in progress show above the composer
  and in the Local models pick, with Cancel. The turn
  of a waiting agent never reads *Failed*. Tiles show the same, compact. The settings are 19
  `overseer.continuity.*` entries with the daemon's ranges; VS Code pushes what the user sets and
  mirrors what the daemon has.

## Security and privacy

- Probes send no credentials and no payload beyond a TLS handshake or an empty `HEAD`; the system
  check sends nothing.
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
- **Windows:** the platform modules have a slot for it; nothing is implemented.
- **Other local runtimes** (LM Studio, llama.cpp servers): the inventory is written behind a small
  interface, but only Ollama is implemented.
- **Metered connections:** macOS can report an expensive path; if the daemon can read it cheaply,
  prefetch pauses on it. Not required for the gate.

## Open questions

None for the owner. Decided on 2026-09-26: the default (on), the name (Continuity), the provider
order, the retry limit, prefetch (off until asked), the catalogue's starting family, permission
modes on handoff, the failover account and model, the way back online, the stall interrupt, the
36-hour ending, waiting when no verified model fits, no quota-driven failover in this gate, no
Claude Code on local models, memory safety, and one goal with the spike first.

Left to the implementation, with the owner's agreement:

| Question | Approach |
| --- | --- |
| How permission requests appear over OpenCode's session transports | Settled by the spike (AC-139): `opencode serve`; see [the decision](#the-spike-comes-first-ac-139). |
| Codex `--oss` as the local harness | Tested during catalogue verification (AC-87); OpenCode stays the default and Codex is offered as `localHarness: codex` if it passes. |
| Derived tags versus per-request `num_ctx` | Derived tags: the OpenAI-compatible route OpenCode uses cannot carry `num_ctx`. |
| Change notifications versus polling for the system check | Polling every 5 seconds first; the SystemConfiguration and NetworkManager subscriptions if the delay is noticeable in AC-83. |

## Order of work (one goal)

The owner's decision: all of Gate L is **one goal**, and its first step is the spike. The steps
below are the order inside that goal; each is independently verifiable. All of it is built in its
own worktree and lands through a pull request.

| Step | Criteria | Outcome |
| --- | --- | --- |
| 0. Find out | AC-139 | The spike: how OpenCode's session transports carry permission requests, written down as a decision with a recorded fixture. Nothing else starts before it. |
| 1. See | AC-83, AC-85, AC-86, AC-87, AC-88, AC-98, AC-140 | Connection state from the system, probes and agents; memory read from the machine; budget and fit; the memory safety guard; verified Qwen coder catalogue; settings owned by the daemon; Continuity on by default with its one-time notice. Nothing changes routing yet. |
| 2. Choose local | AC-89, AC-90, AC-94, AC-138 | Local models are a first-class choice online, with downloads, prefetch and the Ollama install behind settings, and the OpenCode adapter honours permission modes. |
| 3. Keep working | AC-84, AC-91, AC-92, AC-93, AC-96 | Failover, transition to local, wait and retry for up to 36 hours, back online, several local agents. |
| 4. Confirm | AC-95, AC-97 | Honest UI at every state; the owner turns the network off during a real run and sees the transition. |

## Implementation goal

Written on 2026-09-26 at the owner's request. Built on 2026-09-26 and 2026-09-27 in pull request
[#9](https://github.com/beelol/overseer/pull/9), marked ready for review once every criterion that does not
wait for the owner was verified; what waits for the owner is the Wi-Fi step of AC-83 and the offline
session of AC-97. (AC-84's live check ran on 2026-09-27 once Claude Code was signed in; it found and
fixed the reconnecting-in-vain gap above.) Both owner steps are scripted so that the daemon's own record
becomes the evidence: `node test/local/wifi-live.js` watches a real daemon while the owner turns Wi-Fi
off and on and measures the gap; `node test/local/owner-session.js start` opens an isolated VS Code with
the branch's VSIX for the owner's session, and `report` writes what the daemon recorded afterwards.

> Implement Continuity (Gate L) in `beelol/overseer`: AC-139 first, then AC-83 to AC-98, AC-138 and
> AC-140, as written in `docs/overseer-rfc.md` (Gate L) and designed in `docs/rfcs/offline-mode.md`.
> All of it happens in its own worktree, on its own branch, and lands only through its own new pull
> request: code, tests, evidence, and the ledger and RFC updates. Nothing is pushed to main and
> nothing to any other branch or pull request.
>
> 1. **Find out (AC-139).** Learn how OpenCode's session transports (`opencode acp`, `opencode serve`)
>    carry permission requests and their answers, interrupt, resume, model and agent choice, and
>    child sessions, using a local model within the memory budget. Write the decision into the RFC
>    and record a fixture. Nothing else starts before this is written down.
> 2. **See (AC-83, AC-85, AC-86, AC-87, AC-88, AC-98, AC-140).** Connection state, local inventory,
>    memory budget and fit, the memory safety guard, the verified Qwen coder catalogue, the settings,
>    and the first-use notice.
> 3. **Choose local (AC-89, AC-90, AC-94, AC-138).** Downloads, the Ollama install, local models in
>    the composer, and permission modes through OpenCode.
> 4. **Keep working (AC-84, AC-91, AC-92, AC-93, AC-96).** Failover, transition to local, wait and
>    retry, back online, several local agents.
> 5. **Confirm (AC-95, AC-97).** The honest offline UI and the owner's offline session.
>
> Open the pull request early, as a draft, right after the spike, and keep it current: bring main
> into the branch at the start of every step and before every push, resolve conflicts at once, and
> never leave the pull request conflicted or stale. Stay clear of the other work in flight: put
> Continuity in new modules, keep edits to shared files small and additive, check the open pull
> requests before touching a file they change, and never push to another session's branch.
>
> Memory safety comes before everything. Never load a local model whose measured or estimated size
> is above the budget, by any path. Never load `qwen3.5:122b`. During catalogue verification load
> one model at a time, smallest first, unload each after its check, and record memory before and
> after.
>
> Keep the checklist, the README list and the verification ledger current through
> `docs/verification/records.py`. Check a box only with reproducible evidence. Never weaken, rename
> or delete a criterion to finish; a scope change needs the owner's recorded decision. When a
> criterion is blocked, record the blocker and the next action in its record and continue with the
> others. A partial milestone is progress, not completion.

### Pull request and work in flight

The owner's condition (2026-09-26): the work must happen in its own pull request, kept up to date
and out of conflict with everything else in flight.

- **Everything in the pull request.** Code, tests, evidence, the spike's write-up, and every ledger
  and RFC update the goal makes land through this pull request. The goal pushes nothing to main and
  nothing to another branch or pull request. Boxes are checked on main only when the pull request
  merges.

- **One new pull request**, opened as a draft right after the spike, so the work is visible from the
  start. It is marked ready when every criterion that does not wait for the owner is verified. The
  description follows the `open-pr` skill and is updated as steps land.
- **Kept current.** Main is brought into the branch at the start of every step, before every push,
  and whenever main gains a commit that touches the daemon or the extension. Main is merged into
  the branch rather than the branch rebased, so review comments keep their place and nothing is
  force-pushed once the pull request is open. `cargo test` and the affected UI scenarios are rerun
  after each sync.
- **Never conflicted.** A conflict is resolved in the same sitting it appears. Generated files (the
  README list, the ledger README, the `AC-NN.md` records) are never merged by hand: take main's
  side and rerun `docs/verification/records.py`.
- **Shaped to stay out of the way.** Continuity lives in new files: `daemon/src/net.rs`, `sys.rs`,
  `continuity.rs` (state and policy), `local.rs` (Ollama inventory, catalogue, downloads),
  `handoff.rs`, and `extension/src/continuity.js` with its webview script. Shared files
  (`daemon.rs`, `adapters.rs`, `server.rs`, `store.rs`, `extension.js`, `composer.js`,
  `package.json`) get small additive edits: one dispatch line, one hook call, one settings block.
  No reformatting and no moving of code that Continuity does not own.
- **Look before touching.** At the start of every step the session lists the open pull requests and
  the files they change. A shared file that an open pull request changes is edited last and as
  little as possible, or after that pull request merges. The session never pushes to another
  session's branch or pull request.
- **The order helps.** Steps 0 and 1 are almost entirely new daemon modules. The steps that change
  the composer and the chat come later, when the UI work in flight has most likely merged.
- **New run states must not break other surfaces.** `waiting_for_connection`, `waiting_for_memory`
  and `handed_off` are checked in the terminal UI (`tui/`) and the grid: an unknown state reads as
  plain text, never as a crash or as "failed".

In flight on 2026-09-26 (a snapshot; the session rechecks at every step):

| Pull request | Touches | What it means for Continuity |
| --- | --- | --- |
| #8 Gate K follow-ups | composer, account names, search, grid | The composer work (AC-94, AC-98) builds on it after it merges. |
| #5 Reactor audio cues | 3 daemon files, `extension/package.json`, extension source | Settings block and daemon hooks sit next to Continuity's; keep both additive. |
| #3 Swarm mode (draft) | 30 daemon files, 27 daemon tests | The largest overlap (`daemon.rs`, `store.rs`, `server.rs`). The owner reviews it through inline comments; do not push to it. New modules keep Continuity clear of it. |
| #2 Auto mode RFC (draft) | one RFC on task-aware routing | Read it before building failover (AC-84). Handoff stays a mechanism routing can reuse; Continuity decides only on connection state. |
| #6 TUI audio controls (draft) | `tui/` | No overlap expected. |
| Gate M and Gate N (criteria on main, not started) | the whole UI surface; the phone remote | Continuity's UI builds on the merged Gate K layout and keeps to its own cards and badges. |

### Start gate and authority

Granted by the owner on 2026-09-26 for this goal:

- **Model downloads for verification:** the Qwen coders that are not installed (`qwen2.5-coder` 32b,
  7b, 3b and 1.5b), about 28 GB.
- **Ollama install test:** in an isolated folder, with the owner's own Ollama untouched.
- **Light paid turns:** ChatGPT accounts on `gpt-5.6-luna` at low effort only; Claude lightly. One
  attempt per step, no retry loops against paid accounts.
- **Commits, a branch and one new pull request** in `beelol/overseer`, kept current with main by
  merging main into the branch. Nothing from this goal goes to main except through that pull
  request.

Not granted: purchases, login changes, automatic merges, editing the user's own OpenCode or Ollama
configuration, and **turning the network off**. The implementing session needs the network itself,
so the Wi-Fi steps of AC-83 and AC-97 are the owner's: the session asks the exact question, ends
its turn, continues independent work, and reads the daemon's event log afterwards. It never blocks
on a foreground wait. As built, the two scripts above only read Wi-Fi power and the daemon's events;
neither changes the network.

## Acceptance

AC-83 to AC-98 and AC-138 to AC-140 in the main RFC are the acceptance criteria. Their Verify clauses cover, in short:

- the three connection states from the system's answer, fixture-controlled probes and harness errors,
  including Wi-Fi turned off on a real machine, a captive portal, a DNS failure, and a 429 that is
  never counted as offline;
- failover from an unreachable provider to a working one in the owner's order, and to local only when
  nothing online works;
- memory read from the kernel on macOS (and `/proc` on Linux) matching the system's own tools; the
  budget formula against machine profiles (16, 32, 64 and 128 GiB); estimates within 15% of measured;
- every Qwen coder this machine can run passing or failing the write check, recorded;
- settings enforced by the daemon with VS Code closed; no download or install without its setting;
- a download with progress and cancel; prefetch off until asked; an Ollama install with signature
  verification; loopback only;
- a real transition to OpenCode + Ollama with the picked model when the system reports no network,
  announced in the owner's words, in the same worktree, with the run tree showing predecessor and
  successor;
- wait and retry with backoff, exactly one delivery of the pending message, the harness's own resume
  when the connection returns, and an honest failure with Retry now after 36 hours;
- back online: new agents default to online, Switch back and Stay local both work;
- local models offered in the composer with fit badges; several local agents sharing one model;
- Continuity on by default with the one-time notice, and off with one switch;
- the spike's written decision, transcripts and fixture;
- no model above the budget loadable by any path, the load watchdog and the critical-pressure valve;
- permission modes carried over on handoff and never loosened, with Plan only, Ask first, Accept
  edits and Auto working through OpenCode;
- the owner's dated confirmation after disconnecting during a real run.
