#!/usr/bin/env node
// SYNTHETIC Claude Code stream-json fixture (not a live harness). Emits the documented
// message shapes for FIXTURE_MODE, reading control responses from stdin.
//   nested:      Agent -> child -> grandchild, with a duplicated event and the grandchild's
//                messages delivered before its parent's tool_use (delayed parent)
//   permission:  asks can_use_tool for Write, waits for allow/deny, writes the file if allowed
//   ratelimit / quota / auth: emits the corresponding error result formats
//   prose:       says it delegated but never launches a child
//   background:  interim result while a background Agent runs, then a Write permission request
//   background-early: the background Agent finishes before the interim result; Claude then
//                continues with a new turn that asks for Write permission (live 2.1.x order)
const fs = require('fs');
const path = require('path');
const readline = require('readline');
if (!process.argv.includes('-p')) { console.log('claude-fixture 0.0.0 (synthetic)'); process.exit(0); }
const mode = process.env.CLAUDE_FIXTURE_MODE || process.env.FIXTURE_MODE || 'nested';
const sid = 'fixture-session-1';
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
const assistant = (content, parent = null) => out({ type: 'assistant', session_id: sid, parent_tool_use_id: parent, message: { role: 'assistant', content } });
const user = (content, parent = null) => out({ type: 'user', session_id: sid, parent_tool_use_id: parent, message: { role: 'user', content } });
const result = (isError, text) => out({ type: 'result', subtype: isError ? 'error_during_execution' : 'success', is_error: isError, result: text, session_id: sid, usage: { input_tokens: 1, output_tokens: 1 }, num_turns: 1 });
const rl = readline.createInterface({ input: process.stdin });
const lines = [];
let waiting;
rl.on('line', l => { let m; try { m = JSON.parse(l); } catch { return; } lines.push(m); if (waiting) waiting(); });
const next = pred => new Promise(resolve => { const check = () => { const i = lines.findIndex(pred); if (i >= 0) { const [m] = lines.splice(i, 1); waiting = undefined; resolve(m); } }; waiting = check; check(); });
const sleep = ms => new Promise(r => setTimeout(r, ms));

(async () => {
  await next(m => m.type === 'user');
  out({ type: 'system', subtype: 'init', session_id: sid, model: 'fixture', cwd: process.cwd(), tools: ['Agent', 'Write'] });
  if (mode === 'nested') {
    // Grandchild traffic arrives before the child's Agent tool_use is reported (delayed parent).
    assistant([{ type: 'tool_use', id: 'toolu_grand', name: 'Agent', input: { description: 'grandchild task', prompt: 'hi' } }], 'toolu_child');
    assistant([{ type: 'text', text: 'grandchild says hi' }], 'toolu_grand');
    assistant([{ type: 'tool_use', id: 'toolu_child', name: 'Agent', input: { description: 'child task', prompt: 'delegate' } }]);
    assistant([{ type: 'tool_use', id: 'toolu_child', name: 'Agent', input: { description: 'child task', prompt: 'delegate' } }]); // duplicate
    user([{ type: 'tool_result', tool_use_id: 'toolu_grand', content: 'hi' }], 'toolu_child');
    assistant([{ type: 'text', text: 'child done' }], 'toolu_child');
    user([{ type: 'tool_result', tool_use_id: 'toolu_child', content: 'done' }]);
    assistant([{ type: 'text', text: 'all done' }]);
    result(false, 'all done');
  } else if (mode === 'permission') {
    const file = path.join(process.cwd(), 'perm.txt');
    // Like the live CLI: the tool_use is reported first, then the permission request.
    assistant([{ type: 'tool_use', id: 'toolu_write', name: 'Write', input: { file_path: file, content: 'allowed\n' } }]);
    out({ type: 'control_request', request_id: 'req-1', request: { subtype: 'can_use_tool', tool_name: 'Write', input: { file_path: file, content: 'allowed\n' } } });
    const reply = await next(m => m.type === 'control_response' || (m.type === 'control_request' && m.request?.subtype === 'interrupt'));
    if (reply.type === 'control_request') { result(true, 'interrupted'); await sleep(50); process.exit(130); }
    const decision = reply.response.response;
    if (decision.behavior === 'allow') { fs.writeFileSync(file, decision.updatedInput.content); user([{ type: 'tool_result', tool_use_id: 'toolu_write', content: 'File created successfully at: ' + file }]); assistant([{ type: 'text', text: 'wrote perm.txt' }]); result(false, 'wrote'); }
    else { user([{ type: 'tool_result', tool_use_id: 'toolu_write', content: 'Permission denied: ' + decision.message, is_error: true }]); assistant([{ type: 'text', text: 'permission denied: ' + decision.message }]); result(false, 'denied'); }
  } else if (mode === 'background') {
    assistant([{ type: 'tool_use', id: 'toolu_bg', name: 'Agent', input: { description: 'background child', prompt: 'hi' } }]);
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [{ task_id: 't1', task_type: 'local_agent' }] });
    out({ type: 'system', subtype: 'task_started', session_id: sid, task_id: 't1', tool_use_id: 'toolu_bg', is_backgrounded: true });
    result(false, 'launched in the background; waiting');
    await sleep(500);
    out({ type: 'system', subtype: 'task_notification', session_id: sid, task_id: 't1', tool_use_id: 'toolu_bg', status: 'completed' });
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [] });
    const file = path.join(process.cwd(), 'bg.txt');
    out({ type: 'control_request', request_id: 'req-bg', request: { subtype: 'can_use_tool', tool_name: 'Write', input: { file_path: file, content: 'after background\n' } } });
    const reply = await next(m => m.type === 'control_response');
    const decision = reply.response.response;
    if (decision.behavior === 'allow') fs.writeFileSync(file, decision.updatedInput.content);
    result(false, 'done');
  } else if (mode === 'background-early') {
    assistant([{ type: 'tool_use', id: 'toolu_bg', name: 'Agent', input: { description: 'background child', prompt: 'hi' } }]);
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [{ task_id: 't1', task_type: 'local_agent' }] });
    out({ type: 'system', subtype: 'task_started', session_id: sid, task_id: 't1', tool_use_id: 'toolu_bg', is_backgrounded: true });
    user([{ type: 'tool_result', tool_use_id: 'toolu_bg', content: 'Async agent launched successfully.' }]);
    assistant([{ type: 'text', text: 'hi' }], 'toolu_bg');
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [] });
    out({ type: 'system', subtype: 'task_notification', session_id: sid, task_id: 't1', tool_use_id: 'toolu_bg', status: 'completed' });
    assistant([{ type: 'text', text: 'Waiting for it to complete...' }]);
    result(false, 'Waiting for it to complete...');
    await sleep(300);
    out({ type: 'system', subtype: 'init', session_id: sid, model: 'fixture', cwd: process.cwd(), tools: ['Agent', 'Write'] });
    const file = path.join(process.cwd(), 'bg.txt');
    assistant([{ type: 'tool_use', id: 'toolu_w', name: 'Write', input: { file_path: file, content: 'after background\n' } }]);
    out({ type: 'control_request', request_id: 'req-bg', request: { subtype: 'can_use_tool', tool_name: 'Write', input: { file_path: file, content: 'after background\n' } } });
    const reply = await next(m => m.type === 'control_response');
    const decision = reply.response.response;
    if (decision.behavior === 'allow') fs.writeFileSync(file, decision.updatedInput.content);
    result(false, 'done');
  } else if (mode === 'ratelimit') {
    out({ type: 'assistant', session_id: sid, error: 'rate_limit', message: { role: 'assistant', content: [{ type: 'text', text: 'API Error: Request rejected (429) · rate limited' }] } });
    result(true, 'API Error: Request rejected (429) · rate limited');
  } else if (mode === 'quota') {
    result(true, "Claude usage limit reached. Your limit will reset at 5pm.");
  } else if (mode === 'auth') {
    out({ type: 'assistant', session_id: sid, error: 'authentication_failed', message: { role: 'assistant', content: [{ type: 'text', text: 'Failed to authenticate: OAuth session expired and could not be refreshed' }] } });
    result(true, 'Failed to authenticate: OAuth session expired and could not be refreshed');
  } else if (mode === 'prose') {
    assistant([{ type: 'text', text: 'I delegated this to a sub-agent and it finished.' }]);
    result(false, 'I delegated this to a sub-agent and it finished.');
  }
  await sleep(100);
  process.exit(0);
})();
