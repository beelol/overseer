#!/usr/bin/env node
// Transport capture only. This fixture never calls a model or consumes credentials.
const fs = require('fs');
if (process.argv.includes('--version')) {
  console.log('mods-fixture 0.1 (not an installed native harness)');
  process.exit(0);
}
const claude = process.argv.includes('-p');
const target = claude ? process.env.MODS_CAPTURE_FILE : process.argv[2];
const stay = process.argv.includes('--stay');
if (!target) throw new Error('fixture needs its private capture path');
function capture(text) {
  fs.appendFileSync(target, JSON.stringify({ text, argv: process.argv.slice(2), transport: claude ? 'claude_fixture' : 'generic_fixture' }) + '\n');
  if (claude) {
    console.log(JSON.stringify({ type: 'system', subtype: 'init', session_id: 'mods-fixture', model: 'fixture' }));
    console.log(JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text: 'Captured fixture input.' }] } }));
    console.log(JSON.stringify({ type: 'result', subtype: 'success', is_error: false, result: 'Captured fixture input.', session_id: 'mods-fixture' }));
  } else {
    console.log('Captured fixture input.');
  }
  if (!stay) process.exit(0);
}
if (claude) {
  require('readline').createInterface({ input: process.stdin }).on('line', line => {
    const message = JSON.parse(line);
    if (message.type === 'user') {
      const body = message.message.content;
      capture(typeof body === 'string' ? body : body.filter(p => p.type === 'text').map(p => p.text).join('\n'));
    }
  });
} else {
  let buffer = '', timer;
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => {
    buffer += chunk;
    clearTimeout(timer);
    timer = setTimeout(() => { const input = buffer; buffer = ''; capture(input); }, 60);
  });
}
