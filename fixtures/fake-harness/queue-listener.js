#!/usr/bin/env node
// SYNTHETIC speech process for queue protocol tests. No microphone, speaker, model or account.
const readline = require('readline');
console.log(JSON.stringify({ type: 'ready', input: 'sim' }));
readline.createInterface({ input: process.stdin }).on('line', line => {
  const m = JSON.parse(line);
  if (m.cmd === 'quit') process.exit(0);
});
