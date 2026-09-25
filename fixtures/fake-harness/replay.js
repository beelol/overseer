#!/usr/bin/env node
// Fixture harness: replays a recorded transcript (REPLAY_FILE) line by line with a delay,
// substituting /WORKSPACE with the cwd, and applies REPLAY_WRITE ("path=content") to the cwd.
// Labeled fixture coverage only; never counts as a live harness run.
const fs = require('fs');
const path = require('path');
if (!process.argv.includes('exec')) { console.log('replay-fixture 0.0.0 (not a real harness)'); process.exit(0); }
const file = process.env.REPLAY_FILE;
const delay = Number(process.env.REPLAY_DELAY_MS || 100);
const lines = fs.readFileSync(file, 'utf8').split('\n').filter(Boolean);
let i = 0;
process.on('SIGINT', () => { process.stdout.write(JSON.stringify({ type: 'turn.failed', error: { message: 'interrupted' } }) + '\n'); process.exit(130); });
const tick = () => {
  if (i === Math.floor(lines.length / 2) && process.env.REPLAY_WRITE) {
    const [rel, content] = process.env.REPLAY_WRITE.split('=');
    fs.mkdirSync(path.dirname(path.join(process.cwd(), rel)), { recursive: true });
    fs.appendFileSync(path.join(process.cwd(), rel), content + '\n');
  }
  if (i >= lines.length) process.exit(Number(process.env.REPLAY_EXIT || 0));
  process.stdout.write(lines[i++].split('/WORKSPACE').join(process.cwd()) + '\n');
  setTimeout(tick, delay);
};
tick();
