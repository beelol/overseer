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
//   native-quota: emits scoped structured rate-limit windows and a model-family rejection
//   native-quota-invalid: a newer malformed meter must invalidate older apparent capacity
//   native-quota-block-invalid: a malformed follow-up cannot clear a scoped rejection
//   overseer:    answers as Talk to Overseer (AC-107) from the agents' state in the prompt: a summary
//                for "what is everyone doing?", and for "tell <agent> to <task>" a proposal block
//   swarm:       a Swarm member on the proposed native path (swarm.native_director): with the
//                director's tools it follows CLAUDE_FIXTURE_SWARM_SCRIPT, with a worker's tools
//                CLAUDE_FIXTURE_SWARM_WORKERS; every step goes through the daemon's MCP tools and is
//                written to the script's trace file. Scripted choices, not model reasoning.
// `--version` prints CLAUDE_FIXTURE_VERSION as Claude Code does when it is set; `--help` lists the
// flags the daemon's director qualification looks for unless CLAUDE_FIXTURE_HELP=bare.
const fs = require('fs');
const path = require('path');
const readline = require('readline');
if (process.argv.includes('auth') && process.argv.includes('status')) {
  let email = 'fixture@example.test';
  if (process.env.CLAUDE_FIXTURE_AUTH_COUNTER_FILE || process.env.CLAUDE_FIXTURE_AUTH_PER_PROFILE === '1') {
    const marker = process.env.CLAUDE_FIXTURE_AUTH_PER_PROFILE === '1'
      ? path.join(process.env.CLAUDE_CONFIG_DIR, 'fixture-auth-count')
      : process.env.CLAUDE_FIXTURE_AUTH_COUNTER_FILE;
    const count = Number(fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8') : '0');
    fs.writeFileSync(marker, String(count + 1));
    const switchAfter = Number(process.env.CLAUDE_FIXTURE_AUTH_SWITCH_AFTER || '1');
    if (count >= switchAfter) email = 'switched@example.test';
    const delay = Number(count === 0 ? process.env.CLAUDE_FIXTURE_AUTH_INITIAL_DELAY_MS || 0
      : process.env.CLAUDE_FIXTURE_AUTH_PREFLIGHT_DELAY_MS || 0);
    if (delay > 0) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delay);
  }
  // A profile folder holding a `signed-out` file reads as signed out (Voice Mode's problem rows, AC-168).
  console.log(JSON.stringify({ loggedIn: !(process.env.CLAUDE_CONFIG_DIR && fs.existsSync(path.join(process.env.CLAUDE_CONFIG_DIR, 'signed-out'))),
    authMethod: process.env.CLAUDE_FIXTURE_AUTH_MODE === 'api-key' ? 'api-key' : 'claude.ai',
    email, orgId: 'fixture-org',
    // CLAUDE_FIXTURE_PLAN_FILE: the reported plan, so a test can change it between identity reads.
    subscriptionType: (process.env.CLAUDE_FIXTURE_PLAN_FILE && fs.existsSync(process.env.CLAUDE_FIXTURE_PLAN_FILE)
      && fs.readFileSync(process.env.CLAUDE_FIXTURE_PLAN_FILE, 'utf8').trim()) || 'fixture' }));
  process.exit(0);
}
if (process.argv.includes('--help')) {
  const flags = process.env.CLAUDE_FIXTURE_HELP === 'bare' ? ['-p, --print', '--output-format <format>']
    : ['-p, --print', '--output-format <format>', '--input-format <format>', '--mcp-config <configs...>',
      '--strict-mcp-config', '--allowedTools <tools...>', '--disallowedTools <tools...>',
      '--permission-prompt-tool <tool>', '--model <model>'];
  console.log('Usage: claude [options] [prompt]\n\nOptions:\n' + flags.map(f => '  ' + f).join('\n'));
  process.exit(0);
}
if (!process.argv.includes('-p')) {
  console.log(process.env.CLAUDE_FIXTURE_VERSION ? `${process.env.CLAUDE_FIXTURE_VERSION} (Claude Code)` : 'claude-fixture 0.0.0 (synthetic)');
  process.exit(0);
}
// CLAUDE_FIXTURE_MODE_FILE lets one test session give each task its own mode (read at start).
const modeFile = process.env.CLAUDE_FIXTURE_MODE_FILE;
// Overseer's own run is the one whose prompt carries the agents' state: it is always Overseer,
// whatever mode the agents' processes run in (the mode is settled once the first prompt is read).
let mode = (modeFile && fs.existsSync(modeFile) && fs.readFileSync(modeFile, 'utf8').trim()) || process.env.CLAUDE_FIXTURE_MODE || process.env.FIXTURE_MODE || 'nested';
const sid = 'fixture-session-1';
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
const assistant = (content, parent = null) => out({ type: 'assistant', session_id: sid, parent_tool_use_id: parent, message: { role: 'assistant', content } });
const user = (content, parent = null) => out({ type: 'user', session_id: sid, parent_tool_use_id: parent, message: { role: 'user', content } });
// CLAUDE_FIXTURE_METER_FILE: an account meter shared by every run of this fixture (JSON
// {used, weekly, resets_at, first, last, step}: fractions of each window and the 5-hour reset in
// epoch seconds). Each turn emits a native rate_limit_event after its first model response
// (used + first) and another before its result (used + last); the turn's whole draw (step) is
// added afterwards. As with live Claude Code, neither in-run reading brackets the turn: part of
// its draw lands before the first and after the last.
const meterFile = process.env.CLAUDE_FIXTURE_METER_FILE;
const meterEvent = part => {
  if (!meterFile) return;
  const m = JSON.parse(fs.readFileSync(meterFile, 'utf8'));
  const five = Math.min(1, m.used + m[part]);
  out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed', resetsAt: m.resets_at, rateLimitType: 'five_hour',
    unifiedWindows: { five_hour: { utilization: five, resetsAt: m.resets_at }, seven_day: { utilization: m.weekly, resetsAt: m.resets_at + 6 * 86400 } } },
    uuid: 'fixture-meter-' + part, session_id: sid });
  if (part === 'last') { m.used = Math.min(1, m.used + m.step); fs.writeFileSync(meterFile, JSON.stringify(m)); }
};
const result = (isError, text) => { meterEvent('last'); out({ type: 'result', subtype: isError ? 'error_during_execution' : 'success', is_error: isError, result: text, session_id: sid, usage: { input_tokens: 1, output_tokens: 1 }, num_turns: 1 }); };
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
  meterEvent('first');
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
  } else if (mode === 'ordinary-failure') {
    assistant([{ type: 'text', text: 'The requested check failed.' }]);
    result(true, 'fixture assertion failed during requested work');
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
  } else if (mode === 'native-quota') {
    const hourly = Math.floor(Date.now() / 1000) + 3600;
    const weekly = Math.floor(Date.now() / 1000) + 7 * 86400;
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'rejected',
      rateLimitType: 'seven_day_opus', resetsAt: weekly,
      unifiedWindows: { five_hour: { utilization: 0.2, resetsAt: hourly },
        seven_day: { utilization: 0.3, resetsAt: weekly } },
      providerNote: 'secret-quota-sentinel' } });
    result(false, 'native quota observation emitted');
  } else if (mode === 'native-quota-invalid') {
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.2 } });
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 1.2, providerNote: 'secret-invalid-meter' } });
    result(false, 'invalid meter observed');
  } else if (mode === 'native-quota-regressed') {
    const reset = Math.floor(Date.now() / 1000) + 3600;
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.8, resetsAt: reset } });
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.2, resetsAt: reset,
      providerNote: 'secret-regressed-meter' } });
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.25, resetsAt: reset } });
    result(false, 'native meter decreased without a reset');
  } else if (mode === 'native-quota-partial') {
    const hourly = Math.floor(Date.now() / 1000) + 3600;
    const weekly = Math.floor(Date.now() / 1000) + 7 * 86400;
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.2, resetsAt: hourly,
      unifiedWindows: { seven_day: { utilization: 0.4, resetsAt: weekly } } } });
    await sleep(20);
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 0.3, resetsAt: hourly,
      providerNote: 'secret-partial-meter' } });
    result(false, 'partial native meter observed');
  } else if (mode === 'native-quota-block-invalid') {
    const weekly = Math.floor(Date.now() / 1000) + 7 * 86400;
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'rejected',
      rateLimitType: 'seven_day_opus', resetsAt: weekly } });
    out({ type: 'rate_limit_event', rate_limit_info: { status: 'allowed',
      rateLimitType: 'five_hour', utilization: 1.2 } });
    result(false, 'later meter malformed');
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
    // A slow orchestrator (Voice Mode's holding line, AC-165): nothing at all for a while.
    if (process.env.FIXTURE_OVERSEER_DELAY_MS) await sleep(Number(process.env.FIXTURE_OVERSEER_DELAY_MS));
    // Talk to Overseer. With an MCP server configured (Gate S: --mcp-config), the fixture speaks
    // MCP like the live harness: it calls the daemon's roster tool and proposes through the
    // propose tool, reporting each call as a tool_use. Without one, it answers from the state
    // sent with the message and proposes in a fenced overseer-actions block (AC-107's fallback).
    const content = first.message.content;
    const text = Array.isArray(content) ? content.filter(c => c.type === 'text').map(c => c.text).join('\n') : String(content);
    const state = /Agents \(JSON\):\n([\s\S]*?)\n<\/overseer-state>/.exec(text);
    let agents = state ? JSON.parse(state[1]) : [];
    const stateAgents = agents; // with their repositories (the roster read over MCP has none)
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
        if (/approve|owner/i.test(f.text)) {
          // Words in a finding that ask for more than the level allows: the daemon, not the fixture, says no.
          try { lines.push(await call('propose', { actions: [{ action: 'archive', agent: f.subject }] })); } catch (e) { lines.push('refused: ' + e.message); }
        }
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
    // A spoken request (Voice Mode, Gate R): choose among the daemon's candidates, as the prompt
    // asks; with none, ask one short question and propose nothing. A correction's words give the
    // new task ("I meant wait for the review"); its names were already applied by the daemon.
    // Messages queued while Overseer was busy arrive together: each request is answered on its own.
    const voiceReqs = [...said.matchAll(/Request (V-\d+): (.*)$/gm)].filter(m => !/what is everyone doing|what did .+ change in/i.test(m[2]));
    if (voiceReqs.length && mcp) {
      const replies = [];
      for (const voiceReq of voiceReqs) {
        let words = voiceReq[2].trim();
        // New agents: "…, and someone should write the note", "three agents should each …".
        const words0 = words;
        const count = { one: 1, two: 2, three: 3, four: 4, five: 5, nine: 9 };
        const several = /\b(one|two|three|four|five|nine|\d+) (?:new )?agents? should (?:each )?(.+?)[.!?]*$/i.exec(words0);
        const someone = /,?\s*(?:and )?(?:someone|somebody) should (.+?)[.!?]*$/i.exec(words0);
        const starts = [];
        const repoOf = stateAgents.find(a => a.repo)?.repo;
        if (several && repoOf) { const n = count[several[1].toLowerCase()] || Number(several[1]); for (let i = 0; i < n; i++) starts.push({ action: 'start', repo: repoOf, title: `${several[2].split(' ').slice(0, 3).join(' ')} ${i + 1}`, prompt: `Please ${several[2]}.`, confidence: 'high' }); words = words0.slice(0, several.index).trim(); }
        else if (someone && repoOf) { starts.push({ action: 'start', repo: repoOf, title: someone[1].split(' ').slice(0, 3).join(' '), prompt: `Please ${someone[1]}.`, confidence: 'high' }); words = words0.slice(0, someone.index).trim(); }
        const marks = [...said.slice(0, voiceReq.index).matchAll(/\(Candidates from the daemon: (?:(none named)|(.*?)\. Choose among them)/g)];
        const mark = marks[marks.length - 1];
        const list = mark && mark[2] ? mark[2].split('; ').map(c => /^(.*) \(([^()]+)\): (.+)$/.exec(c)).filter(Boolean).map(m => ({ title: m[1], id: m[2], why: m[3] })) : [];
        const taskOf = s => { const m = /\b(?:to|should(?: both| also| all)?) (.+?)[.!?]*$/i.exec(s); return m ? m[1] : null; };
        const fix = /.*\(correction[^:]*: (.*)\)$/.exec(words);
        let task = taskOf(words.replace(/\s*\(correction[^)]*\)/g, ''));
        if (fix) {
          const c = fix[1].replace(/^(i meant|i mean|actually|instead|no)[, ]+/i, '').trim();
          if (!/^(not |tell |the |that )/i.test(c)) task = c.replace(/[.!?]+$/, '');
          else task = taskOf(c) || task;
        }
        if (!task && /^(yes|go ahead|ok|okay)\b/i.test(words)) task = 'go ahead';
        if (starts.length && !list.length) {
          await call('propose', { actions: starts }).catch(e => replies.push('refused: ' + e.message));
          replies.push(`Starting ${starts.length === 1 ? 'one agent' : starts.length + ' agents'}.`);
          continue;
        }
        if (!list.length) replies.push('Who should I tell?');
        else if (/^archive\b/i.test(words)) {
          await call('propose', { actions: list.map(c => ({ action: 'archive', agent: c.id, confidence: 'high', why: c.why })) });
          replies.push(`Archiving ${list.map(c => c.title).join(' and ')}.`);
        } else if (!task) replies.push(`What should I tell ${list.map(c => c.title).join(' and ')}?`);
        else {
          await call('propose', { actions: [...list.map(c => ({ action: 'message', agent: c.id, text: `Please ${task}.`, confidence: 'high', why: c.why })), ...starts] });
          replies.push(`Telling ${list.map(c => c.title).join(' and ')} to ${task}${starts.length ? ', and starting one agent' : ''}.`);
        }
      }
      reply = replies.join(' ');
      assistant([{ type: 'text', text: reply }]);
      result(false, reply);
      mcp.close();
      await sleep(100);
      process.exit(0);
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
  } else if (mode === 'swarm') {
    const mcp = await mcpClient();
    let n = 0;
    const call = async (name, args) => {
      const id = `toolu_mcp_${++n}`;
      assistant([{ type: 'tool_use', id, name: `mcp__overseer__${name}`, input: args }]);
      out({ type: 'control_request', request_id: `req-mcp-${n}`, request: { subtype: 'can_use_tool', tool_name: `mcp__overseer__${name}`, input: args } });
      const reply = await next(m => m.type === 'control_response' && m.response?.request_id === `req-mcp-${n}`);
      if (reply.response.response.behavior !== 'allow') throw new Error('tool refused');
      let r;
      for (let tries = 0; ; tries++) {
        // The shim reaches the daemon per call; while the daemon restarts a call fails and is retried.
        r = await mcp.call(name, args);
        const text = r.content.map(c => c.text || '').join('');
        if (!(r.isError && /cannot reach overseerd/.test(text)) || tries > 150) break;
        await sleep(200);
      }
      user([{ type: 'tool_result', tool_use_id: id, content: r.content, is_error: !!r.isError }]);
      const text = r.content.map(c => c.text || '').join('');
      if (r.isError) return { error: text };
      try { return JSON.parse(text); } catch { return { text }; }
    };
    // The member's own token, read from its MCP configuration only to show it is not in the prompt.
    const ownToken = () => { const at = process.argv.indexOf('--mcp-config'); return JSON.parse(fs.readFileSync(process.argv[at + 1], 'utf8')).mcpServers.overseer.env.OVERSEER_MCP_TOKEN; };
    const waitFile = async (file, ms) => { const end = Date.now() + ms; while (!fs.existsSync(file)) { if (Date.now() > end) throw new Error('timed out waiting for ' + file); await sleep(50); } };
    if (!mcp || !mcp.tools.length) {
      assistant([{ type: 'text', text: 'no swarm tools' }]);
      result(false, 'no swarm tools');
    } else if (mcp.tools.includes('swarm_plan')) {
      const script = JSON.parse(fs.readFileSync(process.env.CLAUDE_FIXTURE_SWARM_SCRIPT, 'utf8'));
      const trace = (step, fields = {}) => fs.appendFileSync(script.trace, JSON.stringify({ step, role: 'director', ...fields }) + '\n');
      trace('started', { tools: mcp.tools, delegation_denied: process.argv.join(' ').includes('--disallowedTools Agent,Task'), token_in_prompt: firstText.includes(ownToken()) });
      const planned = await call('swarm_plan', { jobs: script.jobs, estimate: script.estimate });
      if (planned.error) throw new Error('plan: ' + planned.error);
      trace('planned', { revision: planned.revision, benefit: planned.benefit });
      for (const offer of script.offers || []) {
        const offered = { job_id: offer.job, brief: offer.brief || `Job ${offer.job}: ${offer.job}` };
        if (offer.target) offered.target = offer.target;
        if (offer.requirements) offered.requirements = offer.requirements;
        const r = await call('swarm_dispatch', offered);
        trace('offered', { job: offer.job, target: offer.target || r.target, status: r.status, reason: r.reason, error: r.error, decision: r.decision });
      }
      const accepted = {};
      for (const job of script.dispatch) {
        // The director states requirements; Auto's selector chooses the account (a fixture target is named).
        const args = { job_id: job.job, brief: job.brief || `Job ${job.job}: ${job.job}` };
        if (job.target) args.target = job.target;
        if (job.requirements) args.requirements = job.requirements;
        const r = await call('swarm_dispatch', args);
        trace('launched', { job: job.job, target: job.target || r.target, route: r.route, status: r.status, reason: r.reason, error: r.error, attempt: r.attempt_id, worker: r.worker_run_id, shared_booking: r.shared_booking });
        if (r.status !== 'launched') throw new Error('dispatch ' + job.job + ': ' + JSON.stringify(r));
      }
      trace('dispatched', { active: (await call('swarm_status', {})).app_slots_in_use });
      if (script.gate) await waitFile(script.gate, 120000);
      const routed = new Set();
      const deadline = Date.now() + 90000;
      let done = false;
      while (Date.now() < deadline && !done) {
        const inbox = await call('swarm_inbox', {});
        for (const m of inbox.messages || []) {
          const route = (script.route || {})[m.message_id];
          if (route && !routed.has(m.message_id)) {
            const sent = await call('swarm_message', { job_id: route.job, type: 'advisory', message_id: route.message_id, payload: route.payload });
            routed.add(m.message_id);
            trace('routed', { message: m.message_id, to: route.job, error: sent.error });
          }
          if (m.type === 'question') {
            const sent = await call('swarm_message', { job_id: m.job_id, attempt_id: m.attempt_id, type: 'advisory', payload: { answer: script.answer || 'yes' } });
            trace('answered', { job: m.job_id, question: m.payload.question, error: sent.error });
          }
          if (m.type === 'result' && !accepted[m.job_id]) {
            const evidence = m.payload.artifact_ids;
            const decision = await call('swarm_decide', { job_id: m.job_id, decision: 'accept', evidence });
            if (decision.error || decision.status !== 'accepted') throw new Error('decide ' + m.job_id + ': ' + JSON.stringify(decision));
            accepted[m.job_id] = evidence;
            trace('accepted', { job: m.job_id, evidence, from: m.attempt_id });
          }
        }
        const state = await call('swarm_status', {});
        if (Object.keys(accepted).length === script.dispatch.length && state.registered_attempts === 0 && inbox.status === 'idle') {
          const checks = script.dispatch.map(j => ({ job_id: j.job, outcome: 'passed', evidence: accepted[j.job] }));
          const completed = await call('swarm_complete', { summary: script.complete.summary, verification: script.complete.verification, checks });
          trace('completed', { status: completed.status, error: completed.error });
          done = true;
        } else await sleep(150);
      }
      if (!done) throw new Error('director timed out: ' + JSON.stringify(Object.keys(accepted)));
      assistant([{ type: 'text', text: 'Swarm complete.' }]);
      result(false, 'Swarm complete.');
    } else {
      const script = JSON.parse(fs.readFileSync(process.env.CLAUDE_FIXTURE_SWARM_WORKERS, 'utf8'));
      const job = (/Job ([A-Za-z0-9_-]+):/.exec(firstText) || [])[1];
      const plan = script.jobs[job];
      const trace = (step, fields = {}) => fs.appendFileSync(script.trace, JSON.stringify({ step, role: 'worker', job, ...fields }) + '\n');
      trace('started', { tools: mcp.tools, delegation_denied: process.argv.join(' ').includes('--disallowedTools Agent,Task'), token_in_prompt: firstText.includes(ownToken()) });
      if (script.gate) await waitFile(script.gate, 120000);
      for (const probe of plan.probe || []) {
        const r = await call(probe.tool, probe.args || {});
        trace('probed', { tool: probe.tool, error: r.error, status: r.status });
      }
      await call('swarm_progress', { text: `working on ${job}` });
      if (plan.discovery) {
        const r = await call('swarm_discovery', plan.discovery);
        trace('discovered', { message: plan.discovery.message_id, error: r.error });
      }
      const awaitMessage = async pred => {
        const end = Date.now() + 60000;
        while (Date.now() < end) {
          const inbox = await call('swarm_inbox', {});
          const found = (inbox.messages || []).find(pred);
          if (found) return found;
          await sleep(150);
        }
        throw new Error('no director message for ' + job);
      };
      if (plan.ask) {
        await call('swarm_ask', { question: plan.ask });
        const answer = await awaitMessage(m => m.payload && m.payload.answer);
        const applied = await call('swarm_applied', { message_id: answer.message_id });
        trace('answer', { answer: answer.payload.answer, applied: applied.phase, error: applied.error });
      }
      if (plan.wait_for) {
        const got = await awaitMessage(m => m.message_id === plan.wait_for);
        const applied = await call('swarm_applied', { message_id: got.message_id });
        trace('received', { message: got.message_id, applied: applied.phase, error: applied.error });
      }
      const submitted = await call('swarm_result', { summary: plan.summary || `${job} done`, evidence: plan.evidence, ...(plan.audit_outcome ? { audit_outcome: plan.audit_outcome } : {}) });
      trace('submitted', { artifact_ids: submitted.artifact_ids, error: submitted.error });
      assistant([{ type: 'text', text: `submitted ${job}` }]);
      result(false, `submitted ${job}`);
    }
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
