// AC-228: a card never shows an internal token or a raw error. Home's `plain` (the words every
// request stage, card result and proposal status go through) turns the daemon's raw texts into
// plain words. Run: node test/unit/plain-words.js
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const code = fs.readFileSync(path.resolve(__dirname, '../../extension/media/home.js'), 'utf8');
const window = { OverseerUI: { el: () => ({}) } };
vm.runInNewContext(code, { window });
const plain = window.OverseerHome.plain;
let failures = 0;
const check = (raw, ok, why) => { const out = plain(raw); const good = ok(out); if (!good) failures++; console.log(good ? 'ok  ' : 'FAIL', JSON.stringify(raw).slice(0, 70), '->', JSON.stringify(out), good ? '' : `(${why})`); };
// Tokens and raw errors the owner saw or the daemon can produce.
const TOKEN = /\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b|\b[a-z]+_[a-z_]+\b/;
const RAW = /\b(Error|anyhow|panicked|Caused by|stack backtrace)\b|\{"|:\d+:\d+/;
const clean = s => !TOKEN.test(s) && !RAW.test(s);
check('NOT_FOR_OVERSEER', s => s === 'Not meant for Overseer: kept as context.', 'the aside in plain words');
check('Done: cadence failed: Error: invalid cadence "auto mode": expected every N turns', s => clean(s) && /^Changing the check-ins did not work/.test(s), 'the failed cadence the owner saw');
check('already_answered: yes by owner (Done: sent)', s => clean(s) && /already answered/i.test(s), 'the second answer');
check('Not done: Phone is now waiting_for_user, not running as when this was proposed. Ask again.', s => clean(s) && /waiting for you/.test(s), 'a state token');
check('Error: {"code":-32601,"message":"unknown method"} at daemon/src/server.rs:120:9', s => clean(s), 'a raw JSON error with a location');
check('start failed: the claude harness is not installed on this Mac\nCaused by:\n  0: spawn', s => clean(s) && /not installed/.test(s), 'an error chain');
check('Sent.', s => s === 'Sent.', 'plain words stay as they are');
check('Starting Draft agent', s => s === 'Starting Draft agent', 'plain words stay as they are');
console.log(failures ? `${failures} failed` : 'every raw text became plain words');
process.exit(failures ? 1 : 0);
