# AC-139 — OpenCode session transport spike (evidence)

Recorded on 2026-09-26 with OpenCode 1.15.13, Ollama 0.34.2 and `qwen3-coder:30b-64k` (a local
model; no account and no paid tokens), in an isolated OpenCode profile (its own `XDG_*` folders)
and two disposable Git repositories. Paths are redacted: `/WORKSPACE` and `/WORKSPACE2` are the
repositories, `/SPIKE` the scratch folder, `/HOME` the home folder, `USER` the account name.
Token deltas are counted, not stored. The decision is in
[the RFC](../../../rfcs/offline-mode.md#the-spike-comes-first-ac-139).

| File | What it is |
| --- | --- |
| [results.txt](results.txt) | Every check of the final runs, pass or fail, with its detail |
| [serve.jsonl](serve.jsonl) | `opencode serve`: allow, deny, plan (first attempt, which waited on a question), interrupt, children, second directory, usage |
| [serve-resume.jsonl](serve-resume.jsonl) | `opencode serve` after the server was restarted: the earlier session continues; plan with the question tool denied |
| [serve-tool-call-as-text.jsonl](serve-tool-call-as-text.jsonl) | An earlier turn in which the model wrote its tool call as text and nothing ran (1 of the 49 prompts sent during the spike) |
| [acp.jsonl](acp.jsonl) | `opencode acp`: allow, deny, plan (never finished after delegating), config options, interrupt (not supported), kill and load |
| [acp-children-load.jsonl](acp-children-load.jsonl) | `opencode acp`: child session and `session/load` in a new process |
| [memory.jsonl](memory.jsonl) | Memory and the budget before the model was loaded, while loaded, and after it was unloaded |

Reproduce: start `opencode serve --port 47931 --hostname 127.0.0.1` in a disposable repository with
an isolated profile whose `opencode.json` names the Ollama provider, then
`OC_BASE=http://127.0.0.1:47931 REPO=<repo> REPO2=<second repo> OUT=serve.jsonl node test/spike/opencode-serve.js`;
for the protocol server, `REPO=<repo> OUT=acp.jsonl node test/spike/opencode-acp.js` with the same
`XDG_*` variables. Check the memory budget before loading a model and unload it afterwards
(`keep_alive: 0`).
