# Zero friction (the owner's overnight goal, 2026-09-28)

What the owner said: "what we have is cool, I just want 0 friction for opening it, telling it to do something, changing course, and following an agent then choosing to manual edit whenever, with simple toggles and automatic behaviour." It must be much better than Codex or Claude Code on their own for managing lots of agents, ideally just by talking to it. Voice may be muted for now, but nothing may regress.

## What must be true
- **Open it:** one command opens the whole layout (AC-250); home talks to Overseer first (AC-236); the input is always on screen (AC-247).
- **Tell it to do something:** Overseer picks the harness, model and account (AC-237), starts agents with its context (AC-231), and asks when it can't place something (AC-216).
- **Change course:** redirect, hold or stop by one sentence, typed or spoken (AC-252); waiting agents can always be answered (AC-241); permission modes by conversation (AC-230).
- **Follow an agent, then Manual edit:** clicking an agent puts you in its head, Follow with inline diffs by default and Diffs only or Manual edit as toggles (AC-233); pop Follow out to another screen (AC-251); Overseer moves you around VS Code (AC-226).
- **Automatic behaviour:** you can always tell it's working (AC-228); stuck and limited agents come back to Overseer (AC-239); Overseer checks finished work (AC-238); you hear about things outside VS Code (AC-240).
- **One view for talking to Overseer**, with voice merged in (AC-227).
- **No regressions:** `scripts/test-all` passes on main after every merge; test windows never reach the owner's screen (AC-249).

## How it runs
1. **Research first** (Sonnet): a friction audit of managing many agents by talking, against Claude Code, Codex and the best orchestration tools, grounded in using Overseer itself; it adds criteria where AC-216 to AC-252 miss something.
2. **Then build** (Opus), in waves that don't touch the same files, each its own branch and pull request, merged by the everything goal after `scripts/test-all`.
3. **The owner is away overnight:** anything that needs them is left partial with the gap stated and goes on the README's owner list; nothing waits on them.
