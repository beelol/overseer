#!/usr/bin/env node
// SYNTHETIC Codex and Claude Code for the Continuity tests (not live harnesses, no model and no
// account): one script stands in for both, and a control file says how each behaves right now.
//
//   $CONTINUITY_FIXTURE   a JSON file read at every start, e.g. {"codex": "network", "claude": "ok"}
//        ok        the turn completes; "write <file> <text>" in the prompt is carried out
//        network   the turn fails because the provider's host cannot be reached
//        outage    the turn fails because the provider answers 503
//        stall     the turn starts and then says nothing, until it is interrupted
//        early     the turn fails on the network before any session is reported
//        reconnect the turn never fails: it keeps saying it is reconnecting, as Codex does, until interrupted
//   $CONTINUITY_LOG       every start is appended here as one JSON line (who, arguments, prompt)
//
//   codex:  --version | login status | exec [resume <id>] --json ... -- <prompt>
//   claude: --version | auth status  | -p ... [--resume <id>] [--permission-mode <m>]   (stream-json)
//
// Both are always signed in (as "fixture"). Sessions get an id the first time and keep it.
'use strict';
const fs = require('fs');
const path = require('path');
const args = process.argv.slice(2);
if (args[0] === '--version') { console.log('continuity-fixture 0.0.0 (synthetic)'); process.exit(0); }
if (args[0] === 'login' && args[1] === 'status') { console.log('Logged in using ChatGPT'); process.exit(0); }
if (args[0] === 'auth' && args[1] === 'status') { console.log(JSON.stringify({ loggedIn: true, authMethod: 'claude.ai', email: 'fixture@example.invalid', subscriptionType: 'max' })); process.exit(0); }

const who = args.includes('-p') ? 'claude' : 'codex';
let control = {};
try { control = JSON.parse(fs.readFileSync(process.env.CONTINUITY_FIXTURE, 'utf8')); } catch { /* everything works */ }
const behaviour = control[who] || 'ok';
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
const flag = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const NETWORK = { codex: 'stream disconnected before completion: error sending request for url (https://chatgpt.com/backend-api/codex/responses): client error (Connect): dns error: failed to lookup address information', claude: 'API Error: Connection error.' };
const OUTAGE = { codex: 'unexpected status 503 Service Unavailable', claude: 'API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}' };

function act(prompt) {
  // The script is the pending message of a handoff prompt, else its task, else the prompt itself.
  // A handoff prompt may follow the guardrails the successor took over, so it is found on any line.
  const handed = /^You are continuing a task/m.test(prompt);
  const script = !handed ? prompt : ((/^The user's last message, not yet answered: (.*)$/m.exec(prompt) || /^Task: (.*)$/m.exec(prompt) || [])[1] || '');
  const done = [];
  for (const step of script.split(';').map(s => s.trim())) {
    const m = /^write (\S+) (.+)$/.exec(step);
    if (m) { fs.mkdirSync(path.dirname(path.resolve(m[1])), { recursive: true }); fs.writeFileSync(path.resolve(m[1]), m[2] + '\n'); done.push(m[1]); }
  }
  return done;
}
function record(prompt, session) {
  if (process.env.CONTINUITY_LOG) fs.appendFileSync(process.env.CONTINUITY_LOG, JSON.stringify({ who, behaviour, args, prompt, session, cwd: process.cwd(), at: Date.now() }) + '\n');
}
const stall = () => { process.on('SIGINT', () => process.exit(130)); process.on('SIGTERM', () => process.exit(143)); setInterval(() => {}, 1000); };

if (who === 'codex') {
  const resume = args[1] === 'resume' ? args[2] : undefined;
  const thread = resume || `fixture-thread-${process.pid}`;
  const prompt = args[args.indexOf('--') + 1] || '';
  record(prompt, thread);
  if (behaviour === 'early') { out({ type: 'error', message: NETWORK.codex }); out({ type: 'turn.failed', error: { message: NETWORK.codex } }); return setTimeout(() => process.exit(1), 30); }
  out({ type: 'thread.started', thread_id: thread });
  out({ type: 'turn.started' });
  if (behaviour === 'stall') return stall();
  if (behaviour === 'reconnect') { setInterval(() => out({ type: 'error', message: 'Reconnecting... waiting for network (Connection failed: error sending request)' }), 400); return stall(); }
  if (behaviour === 'network' || behaviour === 'outage') {
    const message = (behaviour === 'network' ? NETWORK : OUTAGE).codex;
    out({ type: 'error', message });
    out({ type: 'turn.failed', error: { message } });
    return setTimeout(() => process.exit(1), 30);
  }
  const files = act(prompt);
  for (const f of files) out({ type: 'item.completed', item: { id: `item_${f}`, type: 'file_change', changes: [{ path: path.resolve(f), kind: 'add' }], status: 'completed' } });
  out({ type: 'item.completed', item: { id: 'item_m', type: 'agent_message', text: `codex fixture: done${files.length ? ' (' + files.join(', ') + ')' : ''}` } });
  out({ type: 'turn.completed', usage: { input_tokens: 10, output_tokens: 5 } });
  return setTimeout(() => process.exit(0), 30);
}

// Claude Code, stream-json on both sides.
const session = flag('--resume') || `fixture-session-${process.pid}`;
require('readline').createInterface({ input: process.stdin }).once('line', line => {
  let prompt = '';
  try { const c = JSON.parse(line).message.content; prompt = typeof c === 'string' ? c : c.filter(x => x.type === 'text').map(x => x.text).join('\n'); } catch { prompt = line; }
  record(prompt, session);
  out({ type: 'system', subtype: 'init', session_id: session, model: flag('--model') || 'fixture', cwd: process.cwd(), tools: [], permissionMode: flag('--permission-mode') || 'default' });
  if (behaviour === 'stall') return stall();
  if (behaviour === 'network' || behaviour === 'outage') {
    const message = (behaviour === 'network' ? NETWORK : OUTAGE).claude;
    out({ type: 'assistant', session_id: session, parent_tool_use_id: null, message: { role: 'assistant', content: [{ type: 'text', text: message }] } });
    out({ type: 'result', subtype: 'error_during_execution', is_error: true, result: message, session_id: session, usage: {} });
    return setTimeout(() => process.exit(1), 50);
  }
  const files = act(prompt);
  for (const f of files) {
    out({ type: 'assistant', session_id: session, parent_tool_use_id: null, message: { role: 'assistant', content: [{ type: 'tool_use', id: `toolu_${f}`, name: 'Write', input: { file_path: path.resolve(f), content: 'fixture' } }] } });
    out({ type: 'user', session_id: session, parent_tool_use_id: null, message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: `toolu_${f}`, content: 'File created successfully' }] } });
  }
  out({ type: 'assistant', session_id: session, parent_tool_use_id: null, message: { role: 'assistant', content: [{ type: 'text', text: `claude fixture: done${files.length ? ' (' + files.join(', ') + ')' : ''}` }] } });
  out({ type: 'result', subtype: 'success', is_error: false, result: 'done', session_id: session, usage: { input_tokens: 10, output_tokens: 5 }, total_cost_usd: 0 });
  setTimeout(() => process.exit(0), 200);
});
process.stdin.on('end', () => setTimeout(() => process.exit(0), 50));
