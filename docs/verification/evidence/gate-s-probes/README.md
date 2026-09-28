# Gate S — the long waits and the OpenCode probe

`daemon/tests/overseer_probes.rs`, kept out of the everyday suite (`#[ignore]`) because two of
them wait for real time. Run on 2026-09-28 on the owner's Mac, each daemon in an isolated
`OVERSEER_HOME`, with fixture harnesses and the repository's deterministic mock model: no account
was used and no paid turn was spent.

```
cargo test -p overseerd --test overseer_probes -- --ignored --nocapture
```

| Criterion | Probe | Seen |
| --- | --- | --- |
| AC-193: an idle subject causes no wake in ten minutes | A generic agent that prints one line and then works silently (`sleep 3600`) is watched (a new watcher on the Claude fixture); the watch is read every 30 s for ten minutes | `PROBE ac193: watch "wt-9f1a898f7fbe" on a working, silent subject: 0 wakes in 600.262495292s`: no wake, no watcher run started, the watch still open |
| AC-198: an hour of nine idle agents causes no turn | Nine Claude-fixture agents finish; check-ins are set to every third turn; Overseer's turns are counted every minute for an hour | `PROBE ac198: nine idle agents, check-ins every third turn: 0 Overseer turns in 3601.297168625s (the owner's one before)`; no turn started by itself today |
| AC-187: what OpenCode refuses; the label | The real OpenCode 1.15.13 in an isolated profile on the mock model (`fixtures/mock-openai/server.js`); a guardrail denying `src/` with hold on crossing, then "write src/probe.txt" | `PROBE ac187: OpenCode 1.15.13 wrote src/probe.txt (not refused); label watched; guardrail_crossed {"enforcement":"watched","guardrail":"g-7767c24d0537","held":true,"paths":["src/probe.txt"]} ; reported within 5.5655ms of the file appearing`: OpenCode has no per-path refusal, so the label reads watched; the daemon caught the write and held the agent |

The first run of the ten-minute probe stopped at its first check on a wrong expectation in the
test (a watch with no watcher yet reads `null`, not `""`); the test was corrected and the probe
run again in full (the line above).
