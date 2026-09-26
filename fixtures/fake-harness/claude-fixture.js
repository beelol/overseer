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
// CLAUDE_FIXTURE_MODE_FILE lets one test session give each task its own mode (read at start).
const modeFile = process.env.CLAUDE_FIXTURE_MODE_FILE;
const mode = (modeFile && fs.existsSync(modeFile) && fs.readFileSync(modeFile, 'utf8').trim()) || process.env.CLAUDE_FIXTURE_MODE || process.env.FIXTURE_MODE || 'nested';
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
