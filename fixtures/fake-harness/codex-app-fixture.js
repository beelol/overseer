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
const mark = event => { if (process.env.FIXTURE_TRACE_FILE) fs.appendFileSync(process.env.FIXTURE_TRACE_FILE, event + '\n'); };
const rl = readline.createInterface({ input: process.stdin });
let thread = process.env.FIXTURE_MODE?.startsWith('managed') ? 'thr-fixture-' + process.pid : 'thr-fixture-1';
let turn = 'turn-1', approvalId = 7, sawUnsupportedError = false;
let pendingTurnTimer = null;
const supportsMetadata = () => process.env.FIXTURE_MODE?.startsWith('metadata') || process.env.FIXTURE_MODE?.startsWith('managed');
rl.on('line', line => {
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.method === 'initialize') out({ id: m.id, result: { userAgent: 'fixture' } });
  else if (m.method === 'account/read' && process.env.FIXTURE_MODE === 'managed-silent-metadata') {
    mark('metadata_silent');
  } else if (m.method === 'account/read' && supportsMetadata()) {
    const keyAuth = process.env.FIXTURE_MODE === 'metadata-key' ||
      (process.env.FIXTURE_AUTH_FILE && fs.readFileSync(process.env.FIXTURE_AUTH_FILE, 'utf8').trim() === 'key');
    out({ id: m.id, result: { requiresOpenaiAuth: !keyAuth,
      account: { type: keyAuth ? 'apiKey' : 'chatgpt', email: 'private@example.invalid', planType: 'pro' } } });
  } else if (m.method === 'account/rateLimits/read' && supportsMetadata()) {
    if (process.env.FIXTURE_MODE === 'managed-no-quota') {
      out({ id: m.id, error: { code: -32601, message: 'unsupported metadata method' } });
      return;
    }
    const profileKey = process.env.CODEX_HOME ? path.basename(path.dirname(process.env.CODEX_HOME)) : '';
    const profileValue = (directory, fallback) => {
      const file = directory && profileKey ? path.join(directory, profileKey) : '';
      return file && fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim() : fallback;
    };
    const accountId = profileValue(process.env.FIXTURE_ACCOUNT_IDS_DIR,
      process.env.FIXTURE_ACCOUNT_ID_FILE ? fs.readFileSync(process.env.FIXTURE_ACCOUNT_ID_FILE, 'utf8').trim() : 'private-account-id');
    const quotaMode = profileValue(process.env.FIXTURE_QUOTA_MODES_DIR,
      process.env.FIXTURE_QUOTA_MODE_FILE ? fs.readFileSync(process.env.FIXTURE_QUOTA_MODE_FILE, 'utf8').trim() : '');
    const quotaUsed = quotaMode === 'exhausted' ? 100 : 35;
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
    if (process.env.FIXTURE_MODE === 'metadata-delay') setTimeout(reply, 700); else reply();
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
      if (prompt === 'edit then 503') {
        fs.writeFileSync(path.join(process.cwd(), 'partial-edit.txt'), 'written before failure\n');
      }
      if (prompt === 'simulate direct 429' || prompt === 'simulate direct 503' || prompt === 'edit then 503') {
        out({ method: 'turn/completed', params: { threadId: thread,
          turn: { id: turn, status: 'failed', error: { message: prompt === 'simulate direct 429'
            ? 'HTTP 429 Too Many Requests' : 'HTTP 503 Service Unavailable' } } } });
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
      if (turnDelay > 0 && turnDelay <= 10000 &&
          (prompt === 'browser check' || prompt === 'hold parent' || prompt.startsWith('Continue the same task'))) {
        pendingTurnTimer = setTimeout(finish, turnDelay);
      } else finish();
      return;
    }
    if (process.env.FIXTURE_MODE?.startsWith('metadata-usage')) {
      out({ method: 'turn/completed', params: { threadId: thread, turn: { id: turn, status: 'completed', error: null } } });
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
    if (pendingTurnTimer) { clearTimeout(pendingTurnTimer); pendingTurnTimer = null; }
    out({ id: m.id, result: {} });
    out({ method: 'turn/completed', params: { threadId: thread, turn: { id: m.params.turnId, status: 'interrupted', error: null } } });
  }
});
rl.on('close', () => process.exit(0));
