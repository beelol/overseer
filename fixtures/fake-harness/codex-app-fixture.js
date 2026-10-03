#!/usr/bin/env node
// SYNTHETIC Codex app-server fixture (JSON-RPC over stdio, shapes from the codex 0.155
// generated schema). Not a live harness. Flow: initialize -> thread/start|resume ->
// turn/start -> an unsupported server request (must be answered with an error) -> a
// command approval request; accept creates approved.txt, decline does not; turn/interrupt
// ends the turn as interrupted.
const fs = require('fs');
const path = require('path');
const readline = require('readline');
const { spawn } = require('child_process');
if (process.argv[2] === 'login' && process.argv[3] === 'status') {
  const auth = path.join(process.env.CODEX_HOME || path.join(process.env.HOME || '', '.codex'), 'auth.json');
  console.log(fs.existsSync(auth) ? 'Logged in using ChatGPT' : 'Not logged in');
  process.exit(fs.existsSync(auth) ? 0 : 1);
}
if (process.argv[2] !== 'app-server') {
  // FIXTURE_VERSION_FILE lets a test upgrade the harness between runs.
  const file = process.env.FIXTURE_VERSION_FILE;
  const upgraded = file && fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim() : '';
  console.log(upgraded || 'codex-app-fixture 0.0.0 (synthetic)');
  process.exit(0);
}
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
const mark = event => { if (process.env.FIXTURE_TRACE_FILE) fs.appendFileSync(process.env.FIXTURE_TRACE_FILE, event + '\n'); };
const rl = readline.createInterface({ input: process.stdin });
let thread = process.env.FIXTURE_MODE?.startsWith('managed') ? 'thr-fixture-' + process.pid : 'thr-fixture-1';
let turn = 'turn-1', approvalId = 7, sawUnsupportedError = false;
let pendingTurnTimer = null;
const supportsMetadata = () => process.env.FIXTURE_MODE?.startsWith('metadata') || process.env.FIXTURE_MODE?.startsWith('managed');

// Exercise the real per-process MCP bridge from inside the parent harness
// fixture. The fixture supplies decisions in place of a model; the daemon
// still owns route choice, admission, child execution, and result delivery.
async function delegateBrowserThenDiagnose() {
  const overrides = new Map();
  for (let i = 3; i + 1 < process.argv.length; i += 2) {
    if (process.argv[i] !== '-c') continue;
    const value = process.argv[i + 1];
    const equal = value.indexOf('=');
    if (equal > 0) overrides.set(value.slice(0, equal), JSON.parse(value.slice(equal + 1)));
  }
  const command = overrides.get('mcp_servers.overseer_auto.command');
  const args = overrides.get('mcp_servers.overseer_auto.args');
  if (typeof command !== 'string' || !Array.isArray(args) || args[0] !== 'auto-mcp') {
    throw new Error('the Auto MCP server was not injected into the parent process');
  }
  const bridge = spawn(command, args, { stdio: ['pipe', 'pipe', 'ignore'] });
  const pending = new Map();
  let nextId = 1;
  const failPending = error => {
    for (const request of pending.values()) { clearTimeout(request.timer); request.reject(error); }
    pending.clear();
  };
  const lines = readline.createInterface({ input: bridge.stdout });
  lines.on('line', line => {
    let message; try { message = JSON.parse(line); } catch { return; }
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    clearTimeout(request.timer);
    message.error ? request.reject(new Error(message.error.message)) : request.resolve(message.result);
  });
  bridge.on('error', failPending);
  bridge.on('exit', () => failPending(new Error('Auto MCP server exited before its response')));
  const call = (method, params) => new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Auto MCP response timed out')); }, 20000);
    pending.set(id, { resolve, reject, timer });
    bridge.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
  const tool = async (name, args) => {
    const response = await call('tools/call', { name, arguments: args });
    if (response.isError) throw new Error(response.content?.[0]?.text ?? 'Auto tool failed');
    return JSON.parse(response.content[0].text);
  };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const submit = async args => {
    for (let attempt = 0; attempt < 100; attempt++) {
      const response = await tool('auto_submit', args);
      if (response.state === 'dispatched') return response;
      if (response.state !== 'launch_pending') throw new Error(`Auto child was not dispatched: ${response.state}`);
      await sleep(50);
    }
    throw new Error('Auto child launch did not settle');
  };
  const result = async id => {
    for (let attempt = 0; attempt < 200; attempt++) {
      const response = await tool('auto_result', { child_run_id: id });
      if (response.state === 'ready') return response;
      if (response.state !== 'pending') throw new Error(`Auto child did not return text: ${response.state}`);
      await sleep(50);
    }
    throw new Error('Auto child result did not settle');
  };
  try {
    await call('initialize', { protocolVersion: '2025-03-26', capabilities: {},
      clientInfo: { name: 'parent-fixture', version: '1' } });
    fs.writeFileSync(path.join(process.cwd(), 'parent-context.txt'), 'from parent\n');
    const browser = await submit({ work_unit_id: 'fixture-browser', title: 'browser check',
      prompt: 'browser check', min_tier: 'general', required_tools: ['browser/navigate'],
      task_class: 'browser_check' });
    const browserResult = await result(browser.run.id);
    const diagnosis = await submit({ work_unit_id: 'fixture-diagnosis', title: 'diagnosis',
      prompt: `Diagnose ${browserResult.text}`, min_tier: 'frontier', required_tools: [],
      task_class: 'difficult_diagnosis' });
    const diagnosisResult = await result(diagnosis.run.id);
    return `${browserResult.text}; ${diagnosisResult.text}`;
  } finally {
    bridge.stdin.end();
    bridge.kill();
  }
}

rl.on('line', line => {
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.method === 'initialize') out({ id: m.id, result: { userAgent: 'fixture' } });
  else if (m.method === 'account/read' && (process.env.FIXTURE_MODE === 'managed-silent-metadata' ||
      (process.env.FIXTURE_SILENT_ON_AUTO_MCP === '1' && process.argv.some(arg => arg.includes('mcp_servers.overseer_auto.'))))) {
    mark('metadata_silent');
  } else if (m.method === 'account/read' && supportsMetadata()) {
    const keyAuth = process.env.FIXTURE_MODE === 'metadata-key' ||
      (process.env.FIXTURE_KEY_ON_AUTO_MCP === '1' && process.argv.some(arg => arg.includes('mcp_servers.overseer_auto.'))) ||
      (process.env.FIXTURE_AUTH_FILE && fs.readFileSync(process.env.FIXTURE_AUTH_FILE, 'utf8').trim() === 'key');
    out({ id: m.id, result: { requiresOpenaiAuth: !keyAuth,
      account: { type: keyAuth ? 'apiKey' : 'chatgpt', email: 'private@example.invalid', planType: 'pro' } } });
  } else if (m.method === 'account/rateLimits/read' && supportsMetadata()) {
    if (process.env.FIXTURE_MODE === 'managed-no-quota' ||
        (process.env.FIXTURE_NO_QUOTA_ON_AUTO_MCP === '1' && process.argv.some(arg => arg.includes('mcp_servers.overseer_auto.')))) {
      out({ id: m.id, error: { code: -32601, message: 'unsupported metadata method' } });
      return;
    }
    const profileKey = process.env.CODEX_HOME ? path.basename(path.dirname(process.env.CODEX_HOME)) : '';
    const profileValue = (directory, fallback) => {
      const file = directory && profileKey ? path.join(directory, profileKey) : '';
      return file && fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim() : fallback;
    };
    const accountId = process.env.FIXTURE_ACCOUNT_ID_ON_AUTO_MCP &&
      process.argv.some(arg => arg.includes('mcp_servers.overseer_auto.'))
      ? process.env.FIXTURE_ACCOUNT_ID_ON_AUTO_MCP
      : profileValue(process.env.FIXTURE_ACCOUNT_IDS_DIR,
        process.env.FIXTURE_ACCOUNT_ID_FILE ? fs.readFileSync(process.env.FIXTURE_ACCOUNT_ID_FILE, 'utf8').trim() : 'private-account-id');
    const quotaMode = profileValue(process.env.FIXTURE_QUOTA_MODES_DIR,
      process.env.FIXTURE_QUOTA_MODE_FILE ? fs.readFileSync(process.env.FIXTURE_QUOTA_MODE_FILE, 'utf8').trim() : '');
    // A numeric mode is the reported percentage used (a moving meter).
    const quotaUsed = quotaMode === 'exhausted' ? 100
      : /^\d+(\.\d+)?$/.test(quotaMode) ? Number(quotaMode) : 35;
    const planType = process.env.FIXTURE_PLAN_TYPE_FILE
      ? fs.readFileSync(process.env.FIXTURE_PLAN_TYPE_FILE, 'utf8').trim() : 'pro';
    mark('metadata_started');
    const reply = () => {
      mark('metadata_done');
      out({ id: m.id, result: { accountId, ordinaryUsageAllowed: true,
        rateLimitsByLimitId: { codex: { limitId: 'codex', planType, normalModelSlug: null,
          primary: quotaMode === 'unknown' ? null : { usedPercent: quotaUsed, windowDurationMins: 300, resetsAt: 1800003600 }, secondary: null,
          credits: { balance: 'secret-credit-sentinel' } } } } });
    };
    if (process.env.FIXTURE_QUOTA_DELAY_MS) setTimeout(reply, Number(process.env.FIXTURE_QUOTA_DELAY_MS));
    else if (process.env.FIXTURE_MODE === 'metadata-delay') setTimeout(reply, 700);
    else reply();
  } else if (m.method === 'account/usage/read' && (process.env.FIXTURE_MODE?.startsWith('metadata-usage') || process.env.FIXTURE_MODE?.startsWith('managed'))) {
    mark('thread_usage_read');
    const usage = process.env.FIXTURE_MODE === 'metadata-usage-null' ? null : {
      threadId: m.params.threadId, estimatedUsageCreditsMicros: 2500000,
      estimatedUsageUsdMicros: 1234, secret: 'private-credit-sentinel',
      groups: [{ model: 'gpt-6-sol', reasoningEffort: 'medium', speed: 'default',
        estimatedUsageCreditsMicros: 2500000, inputTokens: 100, outputTokens: 20,
        cachedInputTokens: 10, netNewInputTokens: 90, totalTokens: 120,
        prompt: 'secret-prompt-sentinel' }]
    };
    const reply = () => out({ id: m.id, result: { summary: { lifetimeTokens: 999999 }, dailyUsageBuckets: null, threadUsage: usage } });
    if (process.env.FIXTURE_USAGE_DELAY_MS) setTimeout(reply, Number(process.env.FIXTURE_USAGE_DELAY_MS));
    else reply();
  } else if (m.method === 'model/list' && ['metadata-models', 'managed-models'].includes(process.env.FIXTURE_MODE)) {
    mark('model_read');
    if (process.env.FIXTURE_MODEL_DELAY_MS && !m.params?.cursor) {
      setTimeout(() => out({ id: m.id, result: { data: [
        { model: 'gpt-6-astra', isDefault: true, hidden: false, defaultReasoningEffort: 'medium',
          supportedReasoningEfforts: [{ reasoningEffort: 'medium' }, { reasoningEffort: 'high' }], inputModalities: ['text', 'image'] }
      ], nextCursor: 'page2' } }), Number(process.env.FIXTURE_MODEL_DELAY_MS));
      return;
    }
    if (m.params?.cursor === 'page2') {
      out({ id: m.id, result: { data: [
        { model: 'gpt-6-sol', isDefault: false, hidden: false, defaultReasoningEffort: 'medium',
          supportedReasoningEfforts: [{ reasoningEffort: 'low' }, { reasoningEffort: 'medium' }],
          inputModalities: ['text', 'image'], description: 'secret-model-sentinel' }
      ], nextCursor: null } });
    } else {
      out({ id: m.id, result: { data: [
        { model: 'gpt-6-astra', isDefault: true, hidden: false, defaultReasoningEffort: 'medium',
          supportedReasoningEfforts: [{ reasoningEffort: 'medium' }, { reasoningEffort: 'high' }],
          inputModalities: ['text', 'image'] }
      ], nextCursor: 'page2' } });
    }
  } else if (m.method === 'mcpServerStatus/list' && process.env.FIXTURE_MODE?.startsWith('managed')) {
    const toolMode = process.env.FIXTURE_TOOL_MODE_FILE ? fs.readFileSync(process.env.FIXTURE_TOOL_MODE_FILE, 'utf8').trim() : 'available';
    mark('tool_preflight:' + toolMode);
    mark('tool_cwd:' + process.cwd());
    if (process.env.FIXTURE_MODE === 'managed-models' && process.env.FIXTURE_TOOL_READ_ACTION === 'exhaust_quota') {
      fs.writeFileSync(process.env.FIXTURE_QUOTA_MODE_FILE, 'exhausted');
    } else if (process.env.FIXTURE_MODE === 'managed-models' && process.env.FIXTURE_TOOL_READ_ACTION === 'switch_account') {
      fs.writeFileSync(process.env.FIXTURE_ACCOUNT_ID_FILE, 'account-B');
    } else if (process.env.FIXTURE_MODE === 'managed-models' && process.env.FIXTURE_TOOL_READ_ACTION === 'remove_program'
      && process.env.FIXTURE_REMOVE_MARKER && fs.existsSync(process.env.FIXTURE_REMOVE_MARKER)) {
      // The harness is uninstalled after discovery: its launch is rejected before any work.
      fs.unlinkSync(process.argv[1]);
    }
    if (toolMode === 'error') out({ id: m.id, error: { code: -32601, message: 'private-tool-error-sentinel' } });
    else out({ id: m.id, result: { data: toolMode === 'missing' ? [] : [{ name: 'browser', runtimeStatus: 'connected', toolsError: null,
      authStatus: toolMode === 'logged-out' ? 'notLoggedIn' : 'unsupported',
      tools: { navigate: { name: 'navigate', description: 'secret-tool-sentinel', inputSchema: {} } } }], nextCursor: null } });
  } else if (m.method === 'mcpServerStatus/list' && process.env.FIXTURE_MODE === 'metadata-models') {
    mark('tool_read');
    mark('tool_cwd:' + process.cwd());
    if (m.params?.cursor === 'tools2') {
      out({ id: m.id, result: { data: [{ name: 'offline', runtimeStatus: 'failed', toolsError: 'connection failed',
        authStatus: 'unknown', tools: { navigate: { name: 'navigate', inputSchema: {} } } }], nextCursor: null } });
    } else {
      out({ id: m.id, result: { data: [{ name: 'browser', runtimeStatus: 'connected', toolsError: null,
        authStatus: 'unsupported', tools: {
          navigate: { name: 'navigate', inputSchema: {}, description: 'secret-tool-sentinel' },
          snapshot: { name: 'snapshot', inputSchema: {} }
        } }], nextCursor: 'tools2' } });
    }
  } else if (m.method === 'thread/start' && ['metadata', 'metadata-key'].includes(process.env.FIXTURE_MODE)) {
    process.exit(88);
  }
  else if (m.method === 'thread/start' || m.method === 'thread/resume') {
    mark('thread_started');
    mark('thread_approval:' + (m.params.approvalPolicy ?? 'none'));
    mark('thread_sandbox:' + (m.params.sandbox ?? 'none'));
    if (m.params.threadId) thread = m.params.threadId;
    out({ id: m.id, result: { thread: { id: thread }, model: 'fixture' } });
    out({ method: 'thread/started', params: { thread: { id: thread } } });
  } else if (m.method === 'turn/start') {
    mark('turn_effort:' + (m.params.effort ?? 'none'));
    mark('turn_model:' + (m.params.model ?? 'none'));
    turn = 'turn-' + Date.now();
    out({ id: m.id, result: { turn: { id: turn, status: 'inProgress' } } });
    out({ method: 'turn/started', params: { threadId: thread, turn: { id: turn } } });
    if (process.env.FIXTURE_MODE?.startsWith('managed')) {
      const prompt = m.params.input?.[0]?.text ?? '';
      if (prompt === 'fixture: delegate browser then diagnose') {
        const parentTurn = turn;
        delegateBrowserThenDiagnose().then(text => {
          out({ method: 'item/completed', params: { threadId: thread, turnId: parentTurn,
            item: { type: 'agentMessage', id: 'auto-parent-result', text } } });
          out({ method: 'turn/completed', params: { threadId: thread,
            turn: { id: parentTurn, status: 'completed', error: null } } });
        }).catch(error => {
          out({ method: 'turn/completed', params: { threadId: thread,
            turn: { id: parentTurn, status: 'failed', error: { message: error.message } } } });
        });
        return;
      }
      if (prompt === 'edit then 503') {
        fs.writeFileSync(path.join(process.cwd(), 'partial-edit.txt'), 'written before failure\n');
      }
      if (prompt === 'fixture: external effect then wait') {
        const effectFile = process.env.FIXTURE_EXTERNAL_EFFECT_FILE;
        if (!effectFile) throw new Error('external-effect fixture path missing');
        fs.appendFileSync(effectFile, 'effect\n');
        // Leave the model turn unresolved until the test loses both process
        // and supervisor; replay must never execute this branch again.
        pendingTurnTimer = setTimeout(() => {}, 60_000);
        return;
      }
      if (prompt === 'simulate direct 429' || prompt === 'simulate direct 429 with Retry-After'
          || prompt === 'simulate direct 429 with short Retry-After'
          || prompt === 'simulate direct 503' || prompt === 'simulate direct 503 with short Retry-After'
          || prompt === 'edit then 503') {
        const is429 = prompt.startsWith('simulate direct 429');
        out({ method: 'turn/completed', params: { threadId: thread,
          turn: { id: turn, status: 'failed', error: { message: is429
            ? 'HTTP 429 Too Many Requests' : 'HTTP 503 Service Unavailable',
            ...(prompt === 'simulate direct 429 with Retry-After' ? { retryAfterMs: 300000 } : {}),
            ...(prompt === 'simulate direct 429 with short Retry-After'
              || prompt === 'simulate direct 503 with short Retry-After' ? { retryAfterMs: 250 } : {}) } } } });
        return;
      }
      let text;
      if (prompt === 'seed context') {
        fs.writeFileSync(path.join(process.cwd(), 'parent-context.txt'), 'from parent\n');
        text = 'parent ready';
      } else if (prompt === 'browser check') {
        text = fs.existsSync(path.join(process.cwd(), 'parent-context.txt'))
          ? 'browser result: parent context found' : 'browser result: context missing';
        fs.writeFileSync(path.join(process.cwd(), 'browser-report.txt'), text + '\n');
      } else {
        text = prompt.includes('browser result: parent context found') ? 'continued with browser result' : 'missing child result';
      }
      const finish = () => {
        pendingTurnTimer = null;
        if (process.env.FIXTURE_EMIT_USAGE === '1') {
          out({ method: 'thread/tokenUsage/updated', params: { threadId: thread,
            tokenUsage: { inputTokens: 42, outputTokens: 7 } } });
        }
        out({ method: 'item/completed', params: { threadId: thread, turnId: turn,
          item: { type: 'agentMessage', id: 'message-1', text } } });
        out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
      };
      const turnDelay = Number(process.env.FIXTURE_TURN_DELAY_MS ??
        (process.env.FIXTURE_MODE === 'managed-delay' ? 6000 : 0));
      const parentGate = prompt === 'hold parent' && process.env.FIXTURE_HOLD_PARENT_GATE;
      if (parentGate) {
        // An explicit fixture barrier makes account overlap deterministic, even if metadata
        // reads or the test thread are delayed. Other prompts retain their normal behavior.
        const deadline = Date.now() + 30000;
        const poll = () => {
          if (fs.existsSync(parentGate)) { finish(); return; }
          if (Date.now() >= deadline) {
            pendingTurnTimer = null;
            out({ method: 'turn/completed', params: { threadId: thread,
              turn: { id: turn, status: 'failed', error: { message: 'fixture parent gate expired' } } } });
            return;
          }
          pendingTurnTimer = setTimeout(poll, 10);
        };
        poll();
      } else if (turnDelay > 0 && turnDelay <= 10000 &&
          (prompt === 'browser check' || prompt === 'hold parent' || prompt.startsWith('Continue the same task'))) {
        pendingTurnTimer = setTimeout(finish, turnDelay);
      } else finish();
      return;
    }
    if (process.env.FIXTURE_MODE?.startsWith('metadata-usage')) {
      out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
      return;
    }
    if (process.env.FIXTURE_MODE === 'quota-regressed') {
      const reset = Math.floor(Date.now() / 1000) + 3600;
      for (const usedPercent of [80, 20, 25]) {
        out({ method: 'account/rateLimits/updated', params: { rateLimits: {
          limitId: 'codex', primary: { usedPercent, windowDurationMins: 300, resetsAt: reset },
          credits: { balance: 'secret-regressed-credit' }
        } } });
      }
      out({ method: 'turn/completed', params: { threadId: thread,
        turn: { id: turn, status: 'completed', error: null } } });
      return;
    }
    if (process.env.FIXTURE_MODE === 'quota-partial') {
      const hourly = Math.floor(Date.now() / 1000) + 3600;
      const weekly = Math.floor(Date.now() / 1000) + 7 * 86400;
      out({ method: 'account/rateLimits/updated', params: { rateLimits: {
        limitId: 'codex',
        primary: { usedPercent: 20, windowDurationMins: 300, resetsAt: hourly },
        secondary: { usedPercent: 40, windowDurationMins: 10080, resetsAt: weekly }
      } } });
      setTimeout(() => {
        out({ method: 'account/rateLimits/updated', params: { rateLimits: {
          limitId: 'codex', primary: { usedPercent: 30, windowDurationMins: 300,
            resetsAt: hourly }, credits: { balance: 'secret-partial-credit' }
        } } });
        out({ method: 'turn/completed', params: { threadId: thread,
          turn: { id: turn, status: 'completed', error: null } } });
      }, 20);
      return;
    }
    if (process.env.FIXTURE_MODE === 'quota') {
      out({ method: 'account/rateLimits/updated', params: { rateLimits: {
        limitId: 'codex', primary: { usedPercent: 40, windowDurationMins: 300, resetsAt: 1800003600 },
        secondary: { usedPercent: 100, windowDurationMins: 10080, resetsAt: 1800500000 },
        credits: { balance: 'secret-credit-sentinel' }
      } } });
      out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
      return;
    }
    if (process.env.FIXTURE_MODE === 'tree') {
      // A child thread spawned by the root, which spawns a grandchild; child-thread items and
      // its own turn/completed carry the child's threadId.
      out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'collabAgentToolCall', id: 'c1', tool: 'spawnAgent', status: 'completed', senderThreadId: thread, receiverThreadIds: ['thr-child'], prompt: 'child task', agentsStates: { 'thr-child': { status: 'running' } } } } });
      out({ method: 'item/completed', params: { threadId: 'thr-child', turnId: 't-child', item: { type: 'agentMessage', id: 'cm', text: 'child output' } } });
      out({ method: 'item/completed', params: { threadId: 'thr-child', turnId: 't-child', item: { type: 'collabAgentToolCall', id: 'c2', tool: 'spawnAgent', status: 'completed', senderThreadId: 'thr-child', receiverThreadIds: ['thr-grand'], prompt: 'grandchild task', agentsStates: { 'thr-grand': { status: 'completed', message: 'hi' } } } } });
      out({ method: 'turn/completed', params: { threadId: 'thr-child', turn: { id: 't-child', status: 'completed', error: null } } });
    }
    if (process.env.FIXTURE_MODE === 'session-two-commands' || process.env.FIXTURE_MODE === 'session-repeat-command') {
      approvalId = 7;
      out({ id: approvalId, method: 'item/commandExecution/requestApproval', params: {
        kind: 'command', threadId: thread, turnId: turn, itemId: 'session-cmd1',
        command: 'touch first-approved.txt', cwd: process.cwd(), startedAtMs: Date.now(),
        availableDecisions: ['accept', 'acceptForSession', 'decline', 'cancel'], reason: 'first write' } });
      return;
    }
    out({ id: 'srv-1', method: 'currentTime/read', params: {} });
    out({ method: 'item/started', params: { threadId: thread, turnId: turn, item: { type: 'commandExecution', id: 'cmd1', command: 'touch approved.txt', status: 'inProgress', exitCode: null } } });
    out({ id: approvalId, method: 'item/commandExecution/requestApproval', params: { kind: 'command', threadId: thread, turnId: turn, itemId: 'cmd1', command: 'touch approved.txt', cwd: process.cwd(), reason: 'needs write' } });
  } else if (m.id === 'srv-1' && m.error) {
    sawUnsupportedError = true;
  } else if (m.id === approvalId && m.result) {
    if (process.env.FIXTURE_MODE === 'session-two-commands' || process.env.FIXTURE_MODE === 'session-repeat-command') {
      fs.appendFileSync(path.join(process.cwd(), 'native-answers.jsonl'), JSON.stringify(m) + '\n');
      const accepted = ['accept', 'acceptForSession'].includes(m.result.decision);
      if (accepted) fs.writeFileSync(path.join(process.cwd(), approvalId === 7 ? 'first-approved.txt' : 'second-approved.txt'), 'approved\n');
      out({ method: 'serverRequest/resolved', params: { threadId: thread, requestId: approvalId } });
      if (approvalId === 7) {
        approvalId = 8;
        out({ id: approvalId, method: 'item/commandExecution/requestApproval', params: {
          kind: 'command', threadId: thread, turnId: turn, itemId: 'session-cmd2',
          command: process.env.FIXTURE_MODE === 'session-repeat-command' ? 'touch first-approved.txt' : 'touch second-approved.txt',
          cwd: process.cwd(), startedAtMs: Date.now(),
          availableDecisions: ['accept', 'acceptForSession', 'decline', 'cancel'], reason: 'second write' } });
      } else out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
      return;
    }
    const accepted = m.result.decision === 'accept';
    if (accepted) fs.writeFileSync(path.join(process.cwd(), 'approved.txt'), 'approved\n');
    out({ method: 'serverRequest/resolved', params: { threadId: thread, requestId: approvalId } });
    out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'commandExecution', id: 'cmd1', command: 'touch approved.txt', status: accepted ? 'completed' : 'declined', exitCode: accepted ? 0 : null } } });
    if (accepted) out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'fileChange', id: 'fc1', status: 'completed', changes: [{ path: path.join(process.cwd(), 'approved.txt'), kind: { type: 'add' } }] } } });
    out({ method: 'item/completed', params: { threadId: thread, turnId: turn, item: { type: 'agentMessage', id: 'msg1', text: (accepted ? 'done' : 'declined') + (sawUnsupportedError ? ' (unsupported request was refused)' : '') } } });
    out({ method: 'thread/tokenUsage/updated', params: { threadId: thread, tokenUsage: { total: { inputTokens: 1, outputTokens: 1 } } } });
    out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
  } else if (m.method === 'turn/interrupt') {
    if (pendingTurnTimer) { clearTimeout(pendingTurnTimer); pendingTurnTimer = null; }
    out({ id: m.id, result: {} });
    out({ method: 'turn/completed', params: { threadId: thread, turn: { id: m.params.turnId, status: 'interrupted', error: null } } });
  }
});
rl.on('close', () => process.exit(0));
