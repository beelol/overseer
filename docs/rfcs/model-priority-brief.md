# Brief: which model for which task (queued)

Status: queued by the owner on 2026-09-27, to start after the current work (Auto and Swarm on `claude/auto-swarm`, the phone app, Voice Mode, Gate T). This is the owner's brief, not yet an RFC.

## What the owner asked for

1. **Research first.** Every agent model at least from OpenAI and Anthropic, plus the main open-source ones (for example Qwen through Ollama). For each: how much subscription usage it draws per unit of work, and how effective it is, per task type.
2. **Then an RFC** on how Overseer uses this, with acceptance criteria pushed to main.
3. **Then build it,** criteria by criteria, with agents where it makes sense.

## The owner's observations (2026-09-27, to check in the research)

- The models change constantly, so a fixed table will go stale. In the long run the priority is probably telemetry-based: Overseer's own local measurements of usage and results (Auto's recorded quota observations and upper draws are the start of that).
- Usage differs a lot between models. Astra and Fable draw much more than others. Opus 5.5 draws a lot, but less than Fable. Opus 4.8 draws much less than Opus 5.5. Sonnet draws less again.
- **Auto:** the simple answer may be enough. It uses the model the owner prefers, and falls back to the next one when that is short.
- **Swarm:** it should choose the right agent for each task. That may mean asking the director (or the agent itself) which sub-agents fit the task.

## Possible Swarm setting (for the RFC to consider)

One setting, perhaps a slider:
- **Lowest usage:** the least draw that still does the job.
- **Highest efficiency:** the best result per unit of usage.
- **Max:** the strongest models regardless of usage or diminishing returns.

## Open questions for the research

- Is a model table needed at all, or can measured telemetry alone decide? How does Overseer start before it has measurements (a seed table, refreshed)?
- How is "effectiveness" measured without paid benchmark runs (outcomes of real tasks, reviews, retries)?
- How does this fit Auto's qualified upper draw (`daemon/src/upper_draw.rs` on `claude/auto-swarm`) and Swarm's category allocation?
