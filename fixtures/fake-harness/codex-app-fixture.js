#!/usr/bin/env node
// SYNTHETIC Codex app-server fixture (JSON-RPC over stdio, shapes from the codex 0.155
// generated schema). Not a live harness. Flow: initialize -> thread/start|resume ->
// turn/start -> an unsupported server request (must be answered with an error) -> a
// command approval request; accept creates approved.txt, decline does not; turn/interrupt
// ends the turn as interrupted.
const fs = require('fs');
const path = require('path');
const readline = require('readline');
if (process.argv[2] !== 'app-server') { console.log('codex-app-fixture 0.0.0 (synthetic)'); process.exit(0); }
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
const rl = readline.createInterface({ input: process.stdin });
let thread = 'thr-fixture-1', turn = 'turn-1', approvalId = 7, sawUnsupportedError = false;
rl.on('line', line => {
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.method === 'initialize') out({ id: m.id, result: { userAgent: 'fixture' } });
  else if (m.method === 'thread/start' || m.method === 'thread/resume') {
    if (m.params.threadId) thread = m.params.threadId;
    out({ id: m.id, result: { thread: { id: thread }, model: 'fixture' } });
    out({ method: 'thread/started', params: { thread: { id: thread } } });
  } else if (m.method === 'turn/start') {
    turn = 'turn-' + Date.now();
    out({ id: m.id, result: { turn: { id: turn, status: 'inProgress' } } });
    out({ method: 'turn/started', params: { threadId: thread, turn: { id: turn } } });
    out({ id: 'srv-1', method: 'currentTime/read', params: {} });
    out({ method: 'item/started', params: { threadId: thread, turnId: turn, item: { type: 'commandExecution', id: 'cmd1', command: 'touch approved.txt', status: 'inProgress', exitCode: null } } });
    out({ id: approvalId, method: 'item/commandExecution/requestApproval', params: { kind: 'command', threadId: thread, turnId: turn, itemId: 'cmd1', command: 'touch approved.txt', cwd: process.cwd(), reason: 'needs write' } });
  } else if (m.id === 'srv-1' && m.error) {
    sawUnsupportedError = true;
  } else if (m.id === approvalId && m.result) {
    const accepted = m.result.decision === 'accept';
    if (accepted) fs.writeFileSync(path.join(process.cwd(), 'approved.txt'), 'approved\n');
    out({ method: 'serverRequest/resolved', params: { threadId: thread, requestId: approvalId } });
    out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'commandExecution', id: 'cmd1', command: 'touch approved.txt', status: accepted ? 'completed' : 'declined', exitCode: accepted ? 0 : null } } });
    if (accepted) out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'fileChange', id: 'fc1', status: 'completed', changes: [{ path: path.join(process.cwd(), 'approved.txt'), kind: { type: 'add' } }] } } });
    out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'agentMessage', id: 'msg1', text: (accepted ? 'done' : 'declined') + (sawUnsupportedError ? ' (unsupported request was refused)' : '') } } });
    out({ method: 'thread/tokenUsage/updated', params: { threadId: thread, tokenUsage: { total: { inputTokens: 1, outputTokens: 1 } } } });
    out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
  } else if (m.method === 'turn/interrupt') {
    out({ id: m.id, result: {} });
    out({ method: 'turn/completed', params: { threadId: thread, turn: { id: m.params.turnId, status: 'interrupted', error: null } } });
  }
});
rl.on('close', () => process.exit(0));
