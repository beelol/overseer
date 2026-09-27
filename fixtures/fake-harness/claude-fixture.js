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
//   overseer:    answers as Talk to Overseer (AC-107) from the agents' state in the prompt: a summary
//                for "what is everyone doing?", and for "tell <agent> to <task>" a proposal block
const fs = require('fs');
const path = require('path');
const readline = require('readline');
if (process.argv.includes('auth') && process.argv.includes('status')) { console.log(JSON.stringify({ loggedIn: true, authMethod: 'claude.ai', email: 'fixture@example.invalid', subscriptionType: 'max' })); process.exit(0); }
if (!process.argv.includes('-p')) { console.log('claude-fixture 0.0.0 (synthetic)'); process.exit(0); }
// CLAUDE_FIXTURE_MODE_FILE lets one test session give each task its own mode (read at start).
const modeFile = process.env.CLAUDE_FIXTURE_MODE_FILE;
// Overseer's own run is the one whose prompt carries the agents' state: it is always Overseer,
// whatever mode the agents' processes run in (the mode is settled once the first prompt is read).
let mode = (modeFile && fs.existsSync(modeFile) && fs.readFileSync(modeFile, 'utf8').trim()) || process.env.CLAUDE_FIXTURE_MODE || process.env.FIXTURE_MODE || 'nested';
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

/** The MCP server named in --mcp-config (Gate S), spoken to over stdio like the live harness does; null without one. */
async function mcpClient() {
  const at = process.argv.indexOf('--mcp-config');
  if (at < 0) return null;
  const config = JSON.parse(fs.readFileSync(process.argv[at + 1], 'utf8'));
  const [name, server] = Object.entries(config.mcpServers || {})[0] || [];
  if (!server) return null;
  const cp = require('child_process');
  const child = cp.spawn(server.command, server.args || [], { env: { ...process.env, ...(server.env || {}) }, stdio: ['pipe', 'pipe', 'ignore'] });
  const pending = new Map();
  let id = 0;
  require('readline').createInterface({ input: child.stdout }).on('line', l => { let m; try { m = JSON.parse(l); } catch { return; } const p = pending.get(m.id); if (p) { pending.delete(m.id); p(m); } });
  const request = (method, params) => new Promise((resolve, reject) => { const rid = ++id; pending.set(rid, m => m.error ? reject(new Error(m.error.message)) : resolve(m.result)); child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: rid, method, params }) + '\n'); });
  await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'claude-fixture', version: '0' } });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  const tools = (await request('tools/list', {})).tools.map(t => t.name);
  out({ type: 'system', subtype: 'mcp_ready', mcp_servers: [{ name, status: 'connected' }], tools: tools.map(t => `mcp__${name}__${t}`) });
  return { tools, call: (tool, args) => request('tools/call', { name: tool, arguments: args }), close: () => child.kill() };
}

(async () => {
  const first = await next(m => m.type === 'user');
  const firstText = Array.isArray(first.message.content) ? first.message.content.filter(c => c.type === 'text').map(c => c.text).join('\n') : String(first.message.content);
  if (firstText.includes('<overseer-state>')) {
    // CLAUDE_FIXTURE_OVERSEER_MODE_FILE forces Overseer's own turns into another mode (a failing harness).
    const forced = process.env.CLAUDE_FIXTURE_OVERSEER_MODE_FILE;
    mode = (forced && fs.existsSync(forced) && fs.readFileSync(forced, 'utf8').trim()) || 'overseer';
  }
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
    if (process.env.FIXTURE_PERMISSION_BARRIER) {
      const deadline = Date.now() + 30000;
      while (!fs.existsSync(process.env.FIXTURE_PERMISSION_BARRIER) && Date.now() < deadline) await sleep(10);
      if (!fs.existsSync(process.env.FIXTURE_PERMISSION_BARRIER)) throw new Error('permission fixture barrier timed out');
    }
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
  } else if (mode === 'background-nested') {
    // As a live Claude Code 2.1.246 run: a foreground subagent launches its own child in the
    // background (spawn depth 2), is told when it finishes, and returns; the main agent then
    // ends its turn. Claude starts no further top-level turn for the grandchild.
    assistant([{ type: 'tool_use', id: 'toolu_c', name: 'Agent', input: { description: 'child', prompt: 'hi' } }]);
    out({ type: 'system', subtype: 'task_started', session_id: sid, task_id: 'c1', tool_use_id: 'toolu_c', is_backgrounded: false, spawn_depth: 1 });
    assistant([{ type: 'tool_use', id: 'toolu_g', name: 'Agent', input: { description: 'grandchild', prompt: 'hi', run_in_background: true } }], 'toolu_c');
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [{ task_id: 'g1', task_type: 'local_agent' }] });
    out({ type: 'system', subtype: 'task_started', session_id: sid, task_id: 'g1', tool_use_id: 'toolu_g', is_backgrounded: true, spawn_depth: 2 });
    user([{ type: 'tool_result', tool_use_id: 'toolu_g', content: 'Async agent launched successfully.' }], 'toolu_c');
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [] });
    out({ type: 'system', subtype: 'task_notification', session_id: sid, task_id: 'g1', tool_use_id: 'toolu_g', status: 'completed', summary: 'hi' });
    out({ type: 'system', subtype: 'task_notification', session_id: sid, task_id: 'c1', tool_use_id: 'toolu_c', status: 'completed', summary: 'child done' });
    user([{ type: 'tool_result', tool_use_id: 'toolu_c', content: 'child done' }]);
    assistant([{ type: 'text', text: 'done' }]);
    result(false, 'done');
  } else if (mode === 'background-read-in-turn') {
    // As a live Claude Code 2.1.246 run: the main agent messages a finished subagent, which runs
    // again in the background (spawn depth 1) and is reported mid-turn; the main agent reads the
    // notice in its next model call and ends the turn. No further turn follows.
    assistant([{ type: 'tool_use', id: 'toolu_msg', name: 'SendMessage', input: { to: 'c1', message: 'Status check' } }]);
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [{ task_id: 'c1', task_type: 'local_agent' }] });
    out({ type: 'system', subtype: 'task_started', session_id: sid, task_id: 'c1', tool_use_id: 'toolu_c', is_backgrounded: true, spawn_depth: 1 });
    user([{ type: 'tool_result', tool_use_id: 'toolu_msg', content: 'Message queued.' }]);
    out({ type: 'system', subtype: 'background_tasks_changed', session_id: sid, tasks: [] });
    out({ type: 'system', subtype: 'task_notification', session_id: sid, task_id: 'c1', tool_use_id: 'toolu_c', status: 'completed', summary: 'status' });
    const file = path.join(process.cwd(), 'bg.txt');
    assistant([{ type: 'tool_use', id: 'toolu_w', name: 'Write', input: { file_path: file, content: 'after notice\n' } }]);
    fs.writeFileSync(file, 'after notice\n');
    user([{ type: 'tool_result', tool_use_id: 'toolu_w', content: 'File created successfully at: ' + file }]);
    assistant([{ type: 'text', text: 'done' }]);
    result(false, 'done');
  } else if (mode === 'showcase' || mode === 'showcase-permission') {
    // A realistic session for UI checks: reads, a search, an edit, a new file, a test run and a
    // Markdown summary (heading, list, table, code block, inline code, link, long path).
    const cwd = process.cwd();
    const readme = path.join(cwd, 'README.md');
    const session = path.join(cwd, 'src', 'auth', 'session-refresh-coordinator.ts');
    const tool = async (id, name, input, content, isError = false) => {
      assistant([{ type: 'tool_use', id, name, input }]);
      await sleep(30);
      user([{ type: 'tool_result', tool_use_id: id, content, is_error: isError }]);
    };
    assistant([{ type: 'text', text: "I'll start by reading how sign-in works today, then add a refresh coordinator so expired sessions renew once instead of every tab racing." }]);
    await tool('toolu_read', 'Read', { file_path: readme }, fs.readFileSync(readme, 'utf8'));
    await tool('toolu_grep', 'Grep', { pattern: 'refreshToken|validateSession', path: cwd, output_mode: 'files_with_matches' }, 'README.md\na.txt');
    await tool('toolu_glob', 'Glob', { pattern: 'src/**/*.ts' }, 'No files found');
    fs.mkdirSync(path.dirname(session), { recursive: true });
    const code = "export class SessionRefreshCoordinator {\n  private inflight?: Promise<string>;\n\n  refresh(fetchToken: () => Promise<string>): Promise<string> {\n    this.inflight ??= fetchToken().finally(() => { this.inflight = undefined; });\n    return this.inflight;\n  }\n}\n";
    fs.writeFileSync(session, code);
    await tool('toolu_write', 'Write', { file_path: session, content: code }, 'File created successfully at: ' + session);
    const before = fs.readFileSync(readme, 'utf8');
    fs.writeFileSync(readme, before + '\n## Sessions\n\nExpired sessions refresh once through `SessionRefreshCoordinator`.\n');
    await tool('toolu_edit', 'Edit', { file_path: readme, old_string: before.trim().split('\n').pop(), new_string: '## Sessions' }, 'The file ' + readme + ' has been updated.');
    await tool('toolu_bash', 'Bash', { command: 'npm test -- --grep "session refresh"', description: 'Run the session refresh tests' }, '\n  SessionRefreshCoordinator\n    ✓ shares one refresh between callers (4 ms)\n    ✓ clears the in-flight promise after it settles (1 ms)\n\n  2 passing (12 ms)\n');
    assistant([{ type: 'text', text: [
      '## Done: sessions refresh once',
      '',
      'Expired sessions now go through a single `SessionRefreshCoordinator`, so parallel requests share one refresh instead of each calling the token endpoint.',
      '',
      '- **New:** `src/auth/session-refresh-coordinator.ts`',
      '- **Docs:** added a *Sessions* section to `README.md`',
      '- **Tests:** 2 passing',
      '',
      '| Case | Before | After |',
      '| --- | --- | --- |',
      '| 3 tabs expire together | 3 refresh calls | 1 refresh call |',
      '| Refresh fails | stuck spinner | error surfaces once |',
      '',
      '```ts',
      'const token = await coordinator.refresh(() => api.refreshToken());',
      '```',
      '',
      'Full path for reference: ' + session + ' — see the [refresh token spec](https://datatracker.ietf.org/doc/html/rfc6749#section-6).',
    ].join('\n') }]);
    if (mode === 'showcase-permission') {
      const file = path.join(cwd, 'CHANGELOG.md');
      assistant([{ type: 'tool_use', id: 'toolu_changelog', name: 'Write', input: { file_path: file, content: '# Changelog\n\n- Sessions refresh once.\n' } }]);
      out({ type: 'control_request', request_id: 'req-showcase', request: { subtype: 'can_use_tool', tool_name: 'Write', input: { file_path: file, content: '# Changelog\n\n- Sessions refresh once.\n' } } });
      const reply = await next(m => m.type === 'control_response' || (m.type === 'control_request' && m.request?.subtype === 'interrupt'));
      if (reply.type === 'control_request') { result(true, 'interrupted'); await sleep(50); process.exit(130); }
      const decision = reply.response.response;
      if (decision.behavior === 'allow') { fs.writeFileSync(file, decision.updatedInput.content); user([{ type: 'tool_result', tool_use_id: 'toolu_changelog', content: 'File created successfully at: ' + file }]); }
      else user([{ type: 'tool_result', tool_use_id: 'toolu_changelog', content: 'Permission denied', is_error: true }]);
    }
    out({ type: 'result', subtype: 'success', is_error: false, result: 'Sessions refresh once.', session_id: sid, num_turns: 1, duration_ms: 48210, total_cost_usd: 0.0412,
      usage: { input_tokens: 18423, output_tokens: 1204, cache_read_input_tokens: 9321 } });
  } else if (mode === 'slow') {
    // Busy for a few seconds (steering tests); honours an interrupt; each turn echoes its prompt.
    const content = first.message.content;
    const text = Array.isArray(content) ? content.filter(c => c.type === 'text').map(c => c.text).join('\n') : content;
    assistant([{ type: 'text', text: 'working on: ' + text }]);
    const stop = next(m => m.type === 'control_request' && m.request?.subtype === 'interrupt').then(() => 'interrupt');
    const done = sleep(Number(process.env.FIXTURE_SLOW_MS || 5000)).then(() => 'done');
    if ((await Promise.race([stop, done])) === 'interrupt') { result(true, 'interrupted'); await sleep(50); process.exit(130); }
    assistant([{ type: 'text', text: 'finished: ' + text }]);
    result(false, 'finished');
  } else if (mode === 'limits' || mode === 'limits-low') {
    // Claude's rate_limit_event (shape as streamed by Claude Code 2.1.x): near or far from the 5-hour limit.
    const used = mode === 'limits' ? 0.95 : 0.12;
    const reset = Math.floor(Date.now() / 1000) + 3600;
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed', resetsAt: reset, rateLimitType: 'five_hour', unifiedWindows: { five_hour: { utilization: used, resetsAt: reset }, seven_day: { utilization: 0.4, resetsAt: reset + 86400 * 3 } } }, uuid: 'fixture', session_id: sid });
    assistant([{ type: 'text', text: 'done' }]);
    result(false, 'done');
  } else if (mode === 'echo') {
    // Reports what Overseer sent: arguments (effort, permission mode, model, resume) and the content kinds.
    const content = first.message.content;
    const kinds = Array.isArray(content) ? content.map(c => c.type + (c.source ? ':' + c.source.media_type + ':' + c.source.data.length : '')) : ['text'];
    const text = Array.isArray(content) ? content.filter(c => c.type === 'text').map(c => c.text).join('\n') : content;
    assistant([{ type: 'text', text: 'ECHO ' + JSON.stringify({ argv: process.argv.slice(2), kinds, text }) }]);
    result(false, 'echoed');
  } else if (mode === 'ratelimit') {
    out({ type: 'assistant', session_id: sid, error: 'rate_limit', message: { role: 'assistant', content: [{ type: 'text', text: 'API Error: Request rejected (429) · rate limited' }] } });
    result(true, 'API Error: Request rejected (429) · rate limited');
  } else if (mode === 'quota') {
    result(true, "Claude usage limit reached. Your limit will reset at 5pm.");
  } else if (mode === 'auth') {
    out({ type: 'assistant', session_id: sid, error: 'authentication_failed', message: { role: 'assistant', content: [{ type: 'text', text: 'Failed to authenticate: OAuth session expired and could not be refreshed' }] } });
    result(true, 'Failed to authenticate: OAuth session expired and could not be refreshed');
  } else if (mode === 'stop-live') {
    // Shaped like Claude Code 2.1.x when stopped mid-turn: an error result with no text.
    assistant([{ type: 'text', text: 'Writing a long list…' }]);
    const stop = next(m => m.type === 'control_request' && m.request?.subtype === 'interrupt').then(() => 'interrupt');
    const done = sleep(Number(process.env.FIXTURE_SLOW_MS || 8000)).then(() => 'done');
    if ((await Promise.race([stop, done])) === 'interrupt') { out({ type: 'result', subtype: 'error_during_execution', is_error: true, session_id: sid, usage: { input_tokens: 3, output_tokens: 0 }, num_turns: 1 }); await sleep(50); process.exit(130); }
    result(false, 'finished');
  } else if (mode === 'unparsed') {
    // A line the parser does not understand, between normal events.
    process.stdout.write('Warning: telemetry flush skipped (fixture)\n');
    assistant([{ type: 'text', text: 'done' }]);
    result(false, 'done');
  } else if (mode === 'failed-reason') {
    // A failed turn whose reason arrives once, as the result text.
    assistant([{ type: 'text', text: 'Trying the migration…' }]);
    result(true, 'Migration failed: relation users_v2 does not exist');
  } else if (mode === 'overseer') {
    // Talk to Overseer. With an MCP server configured (Gate S: --mcp-config), the fixture speaks
    // MCP like the live harness: it calls the daemon's roster tool and proposes through the
    // propose tool, reporting each call as a tool_use. Without one, it answers from the state
    // sent with the message and proposes in a fenced overseer-actions block (AC-107's fallback).
    const content = first.message.content;
    const text = Array.isArray(content) ? content.filter(c => c.type === 'text').map(c => c.text).join('\n') : String(content);
    const state = /Agents \(JSON\):\n([\s\S]*?)\n<\/overseer-state>/.exec(text);
    let agents = state ? JSON.parse(state[1]) : [];
    const said = text.replace(/^<overseer-state>[\s\S]*?<\/overseer-state>\s*/, '').trim();
    const mcp = process.env.CLAUDE_FIXTURE_NO_MCP ? null : await mcpClient();
    let n = 0;
    const call = async (name, args) => {
      const id = `toolu_mcp_${++n}`;
      assistant([{ type: 'tool_use', id, name: `mcp__overseer__${name}`, input: args }]);
      out({ type: 'control_request', request_id: `req-mcp-${n}`, request: { subtype: 'can_use_tool', tool_name: `mcp__overseer__${name}`, input: args } });
      const reply = await next(m => m.type === 'control_response' && m.response?.request_id === `req-mcp-${n}`);
      if (reply.response.response.behavior !== 'allow') throw new Error('tool refused');
      const r = await mcp.call(name, args);
      user([{ type: 'tool_result', tool_use_id: id, content: r.content, is_error: !!r.isError }]);
      return r.content.map(c => c.text || '').join('');
    };
    let reply;
    const tell = /tell (.+?) to (.+)/i.exec(said);
    // A check-in composed by the daemon: one check_in call per agent, from the JSON it sent; an
    // agent whose files left its area (or whose task names a part it left out) is drifting or
    // done-with-something-left-out; for drifting, propose what the prompt says the level allows.
    const checkIn = /Check-in \(JSON\):\n([\s\S]*?)\n\n/.exec(text);
    const questions = /Questions \(JSON\):\n([\s\S]*?)\n\n/.exec(text);
    const reports = /Reports \(JSON\):\n([\s\S]*?)\n\n/.exec(text);
    const findings = /Findings \(JSON\):\n([\s\S]*?)\n\n/.exec(text);
    // A watcher's finding: Overseer acts on the subject at its level. A stop is a hold (at Ask
    // first a proposal) and then a redirect (a proposal at Steer, done at Auto); a concern is a
    // message to the subject. Each goes in its own proposal so a quiet one is not held back.
    const actOnFindings = async (list, level) => {
      const lines = [];
      for (const f of list) {
        if (f.result === 'stop') {
          lines.push(await call('propose', { actions: [{ action: 'hold', agent: f.subject, reason: 'stop finding from ' + f.watcher_title + ': ' + f.text }] }));
          if (level !== 'ask_first') lines.push(await call('propose', { actions: [{ action: 'redirect', agent: f.subject, text: 'Stop: put the tests back and make them pass instead of deleting them.' }] }));
        } else {
          lines.push(await call('propose', { actions: [{ action: 'message', agent: f.subject, text: 'A watcher raised a concern: ' + f.text }] }));
        }
      }
      return lines;
    };
    const rallyAsk = /rally my agents(?: in (\S+))?/i.exec(said);
    // The rally's map: ask only the agents whose digests cannot answer (one report each, the cost
    // said first); once every digest answers, propose the areas in one proposal.
    const rally = async repo => {
      const map = JSON.parse(await call('rally', repo ? { repo } : {}));
      if (map.ask.length) {
        await call('propose', { actions: map.ask.map(id => ({ action: 'report', agent: id })) });
        return `Rally: ${map.agents.length} agents in ${path.basename(map.repository)}; ${map.ask.length} of them have no area and no report, so I asked them for a report (${map.cost}).`;
      }
      const areas = map.agents.filter(a => !a.area.length && a.suggested_area.length).map(a => ({ action: 'area', agent: a.id, paths: a.suggested_area }));
      if (areas.length) await call('propose', { actions: areas });
      const owns = map.agents.map(a => `${a.title} owns ${(a.area.length ? a.area : a.suggested_area).join(', ') || 'nothing yet'}${a.needs ? ` (needs ${a.needs})` : ''}`).join('; ');
      const overlap = map.overlaps.length ? ' Overlaps: ' + map.overlaps.map(o => o.path).join(', ') + '.' : ' No overlaps.';
      return `Map: ${owns}.${overlap}${areas.length ? ` I proposed ${areas.length} areas.` : ''}`;
    };
    if (rallyAsk && mcp) {
      const reply = await rally(rallyAsk[1]);
      assistant([{ type: 'text', text: reply }]);
      result(false, reply);
      mcp.close();
      await sleep(100);
      process.exit(0);
    }
    if (mcp && (questions || reports || findings) && !checkIn) {
      const lines = [];
      const level = (/The level is (\w+)\./.exec(text) || [])[1] || 'ask_first';
      if (findings) lines.push(...await actOnFindings(JSON.parse(findings[1]), level));
      for (const q of questions ? JSON.parse(questions[1]) : []) lines.push(await call('answer', { ask: q.id, text: 'From the roster: ' + q.question.replace(/\?$/, '') + ' — see the other agents\' digests.' }));
      if (reports) { const list = JSON.parse(reports[1]); lines.push(await rally(list[0]?.repository)); }
      reply = lines.join(' ');
      assistant([{ type: 'text', text: reply }]);
      result(false, reply);
      mcp.close();
      await sleep(100);
      process.exit(0);
    }
    if (checkIn && mcp) {
      const level = (/The level is (\w+)\./.exec(text) || [])[1] || 'ask_first';
      const items = JSON.parse(checkIn[1]);
      const lines = [];
      if (findings) lines.push(...await actOnFindings(JSON.parse(findings[1]), level));
      for (const q of questions ? JSON.parse(questions[1]) : []) lines.push(await call('answer', { ask: q.id, text: 'From the roster: ' + q.question.replace(/\?$/, '') + ' — see the other agents\' digests.' }));
      for (const it of items) {
        const outside = it.area && it.area.length ? it.changed.filter(p => !it.area.some(a => p === a || p.startsWith(a.replace(/\/$/, '') + '/'))) : [];
        const leftOut = (it.asked.join(' ').match(/\[leave out: ([^\]]+)\]/) || [])[1];
        const tripped = it.reasons.some(r => /guardrail|outside|circles|collides/.test(r));
        let result, reason;
        // Drifting first: an agent that left its area is drifting even when it has stopped.
        if (outside.length || tripped) { result = 'drifting'; reason = outside.length ? `wrote outside its area: ${outside.join(', ')}` : it.reasons.join('; '); }
        else if (it.status === 'completed' || it.status === 'failed') { result = 'done'; reason = `finished with ${it.changed.length} files changed`; }
        else { result = 'on_task'; reason = 'its changes stay within its task'; }
        await call('check_in', { agent: it.id, result, reason, left_out: result === 'done' && leftOut ? leftOut : '' });
        lines.push(`${it.title}: ${result}`);
        if (result === 'drifting') {
          const action = level === 'auto' ? { action: 'redirect', agent: it.id, text: 'Back to your task; leave the other files alone.' }
            : level === 'steer' ? { action: 'hold', agent: it.id, reason: 'drifting: ' + reason }
            : { action: 'redirect', agent: it.id, text: 'Back to your task; leave the other files alone.' };
          await call('propose', { actions: [action] });
        }
      }
      reply = 'Check-in: ' + lines.join('; ');
      assistant([{ type: 'text', text: reply }]);
      result(false, reply);
      mcp.close();
      await sleep(100);
      process.exit(0);
    }
    if (mcp) {
      const roster = await call('roster', {});
      // One line per agent: "<id> · <title> · <status> · …".
      agents = roster.split('\n').map(l => l.split(' · ')).filter(p => p.length >= 3).map(p => ({ id: p[0], title: p[1], status: p[2] }));
    }
    const changed = /what did (.+?) change in (\S+)/i.exec(said);
    if (/what is everyone doing/i.test(said)) reply = agents.length ? 'Here is what everyone is doing:\n\n' + agents.map(a => `- **${a.title}**: ${a.status}`).join('\n') : 'No agents are running.';
    else if (changed && mcp) {
      // "What did <agent> change in <file>?": the diff, read through the daemon's tool, quoted.
      const who = agents.find(a => a.title.toLowerCase().includes(changed[1].toLowerCase()));
      const diff = who ? await call('diff', { id: who.id, path: changed[2].replace(/[?.!]+$/, '') }) : '';
      reply = who ? `Here is what ${who.title} changed in ${changed[2]}:\n\n\`\`\`diff\n${diff}\n\`\`\`` : `I could not find an agent called ${changed[1]}.`;
    } else if (tell) {
      const who = agents.find(a => a.title.toLowerCase().includes(tell[1].toLowerCase()));
      const task = tell[2].replace(/[.!?]+$/, '');
      if (!who) reply = `I could not find an agent called ${tell[1]}.`;
      else if (mcp) { const outcome = await call('propose', { actions: [{ action: 'message', agent: who.id, text: `Please ${task}.` }] }); reply = `I proposed sending ${who.title} this message: "Please ${task}." ${outcome}`; }
      else reply = `I will send ${who.title} this follow-up: "Please ${task}."\n\n\`\`\`overseer-actions\n${JSON.stringify([{ action: 'message', agent: who.id, title: who.title, text: `Please ${task}.` }])}\n\`\`\``;
    } else reply = 'I can tell you what your agents are doing, or pass a message to one of them.';
    assistant([{ type: 'text', text: reply }]);
    result(false, reply);
    if (mcp) mcp.close();
  } else if (mode === 'channel') {
    // An agent with Overseer's channel (AC-190): it writes the files its prompt names (write:),
    // claims, reports and asks as the prompt says (claim:, report:, ask:; "report x3:" repeats),
    // answers a request for a report from Overseer with what git sees, and refers to a share.
    // Without --mcp-config (a lone agent) it has no channel and says so.
    const text = firstText;
    const mcp = await mcpClient();
    let n = 0;
    const call = async (name, args) => {
      const id = `toolu_mcp_${++n}`;
      assistant([{ type: 'tool_use', id, name: `mcp__overseer__${name}`, input: args }]);
      out({ type: 'control_request', request_id: `req-mcp-${n}`, request: { subtype: 'can_use_tool', tool_name: `mcp__overseer__${name}`, input: args } });
      const reply = await next(m => m.type === 'control_response' && m.response?.request_id === `req-mcp-${n}`);
      if (reply.response.response.behavior !== 'allow') throw new Error('tool refused');
      const r = await mcp.call(name, args);
      user([{ type: 'tool_result', tool_use_id: id, content: r.content, is_error: !!r.isError }]);
      return r.content.map(c => c.text || '').join('');
    };
    const writes = [...text.matchAll(/write: (\S+)/g)].map(m => m[1]);
    for (const rel of writes) {
      const file = path.join(process.cwd(), rel);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      const content = '// written by the fixture\n';
      const id = `toolu_w_${++n}`;
      assistant([{ type: 'tool_use', id, name: 'Write', input: { file_path: file, content } }]);
      fs.writeFileSync(file, content);
      user([{ type: 'tool_result', tool_use_id: id, content: 'File created successfully at: ' + file }]);
    }
    const lines = [];
    const claim = /claim: ([^;\n]+)/.exec(text);
    if (claim && mcp) lines.push(await call('claim', { paths: claim[1].split(',').map(s => s.trim()) }));
    const rep = /report(?: x(\d+))?: ([^;\n]+)/.exec(text);
    if (rep && mcp) { for (let i = 0; i < Number(rep[1] || 1); i++) lines.push(await call('report', { doing: rep[2].trim(), changed: writes, needs: '', blocked: '' })); }
    const ask = /ask: ([^;\n]+)/.exec(text);
    if (ask && mcp) lines.push(await call('ask', { question: ask[1].trim() }));
    if (/Report, with your report tool/.test(text) && mcp) {
      const changed = require('child_process').execSync('git status --porcelain', { encoding: 'utf8' }).split('\n').filter(Boolean).map(l => l.slice(3).trim());
      lines.push(await call('report', { doing: 'working in ' + path.basename(process.cwd()), changed, needs: '', blocked: '' }));
    }
    let reply;
    const shared = /Shared by Overseer from (.+?) \(/.exec(text);
    if (shared) reply = 'Read the share from ' + shared[1] + '; using it.';
    else if (/Withdrawn: what Overseer shared/.test(text)) reply = 'Dropped the withdrawn share.';
    else if (/Answer to your question/.test(text)) reply = 'Got the answer from Overseer.';
    else if (/Briefing from Overseer/.test(text) && !claim && !rep && !ask) reply = 'Noted the briefing.';
    else reply = mcp ? 'channel: ' + lines.join(' | ') : 'no channel';
    assistant([{ type: 'text', text: reply }]);
    result(false, reply);
    if (mcp) mcp.close();
  } else if (mode === 'watcher') {
    // A watcher (AC-193, AC-194): woken by the daemon with what changed in its subject, it files
    // one finding through the finding tool. Deleting tests is a stop; a hesitation is a concern;
    // a watch that checks runs the copy's test.sh and reports a failing one; else fine.
    const text = firstText;
    const mcp = await mcpClient();
    let n = 0;
    const call = async (name, args) => {
      const id = `toolu_mcp_${++n}`;
      assistant([{ type: 'tool_use', id, name: `mcp__overseer__${name}`, input: args }]);
      out({ type: 'control_request', request_id: `req-mcp-${n}`, request: { subtype: 'can_use_tool', tool_name: `mcp__overseer__${name}`, input: args } });
      const reply = await next(m => m.type === 'control_response' && m.response?.request_id === `req-mcp-${n}`);
      if (reply.response.response.behavior !== 'allow') throw new Error('tool refused');
      const r = await mcp.call(name, args);
      user([{ type: 'tool_result', tool_use_id: id, content: r.content, is_error: !!r.isError }]);
      return r.content.map(c => c.text || '').join('');
    };
    const changed = (/What changed since your last wake[^\n]*:\n([\s\S]*?)\n\nFile one finding/.exec(text) || [])[1] || '';
    let finding = { result: 'fine', text: 'nothing to report' };
    if (/delete/i.test(changed) && /tests?/i.test(changed)) finding = { result: 'stop', text: 'it is deleting tests so that the suite passes: ' + changed.split('\n').find(l => /delete/i.test(l)) };
    else if (/hmm|not sure/i.test(changed)) finding = { result: 'concern', text: 'it sounds unsure: ' + changed.split('\n').find(l => /hmm|not sure/i.test(l)) };
    if (/Your copy of the subject's worktree/.test(text) && fs.existsSync(path.join(process.cwd(), 'test.sh'))) {
      const id = `toolu_sh_${++n}`;
      assistant([{ type: 'tool_use', id, name: 'Bash', input: { command: 'sh test.sh' } }]);
      let output = '', failed = false;
      try { output = require('child_process').execSync('sh test.sh', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); } catch (e) { failed = true; output = String(e.stdout || '') + String(e.stderr || ''); }
      user([{ type: 'tool_result', tool_use_id: id, content: output, is_error: failed }]);
      if (failed) finding = { result: 'concern', text: 'the tests do not pass in its worktree: ' + (output.split('\n').find(l => /FAIL/.test(l)) || output.trim()) };
    }
    const outcome = mcp ? await call('finding', finding) : 'no channel';
    const reply = `finding: ${finding.result} (${outcome})`;
    assistant([{ type: 'text', text: reply }]);
    result(false, reply);
    if (mcp) mcp.close();
  } else if (mode === 'circles') {
    // The same command failing three times in a row (a free check of AC-189).
    for (let i = 1; i <= 3; i++) {
      assistant([{ type: 'tool_use', id: `toolu_fail_${i}`, name: 'Bash', input: { command: 'npm test' } }]);
      user([{ type: 'tool_result', tool_use_id: `toolu_fail_${i}`, content: 'Error: 1 failing', is_error: true }]);
    }
    assistant([{ type: 'text', text: 'the tests keep failing' }]);
    result(false, 'the tests keep failing');
  } else if (mode === 'prose') {
    assistant([{ type: 'text', text: 'I delegated this to a sub-agent and it finished.' }]);
    result(false, 'I delegated this to a sub-agent and it finished.');
  }
  await sleep(100);
  process.exit(0);
})();
