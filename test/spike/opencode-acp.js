#!/usr/bin/env node
// AC-139 spike: how `opencode acp` (Agent Client Protocol, JSON-RPC 2.0 over stdio, one JSON object
// per line) carries permission requests and answers, interrupt, resume, model and agent (mode)
// choice, child sessions and usage. LOCAL MODEL ONLY (Ollama). Spawns the agent itself with the
// environment it is given, and writes every message in both directions to a transcript (OUT).
//
//   REPO=/path/to/disposable/repo OUT=acp-transcript.jsonl OPENCODE=~/.opencode/bin/opencode \
//   node test/spike/opencode-acp.js [allow deny plan config interrupt kill children load]
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const REPO = process.env.REPO;
const OUT = process.env.OUT || 'acp-transcript.jsonl';
const OPENCODE = process.env.OPENCODE || 'opencode';
const STATE = process.env.STATE || path.join(path.dirname(OUT), 'acp-state.json');
const t0 = Date.now();
const results = [];
const record = (kind, data) => fs.appendFileSync(OUT, JSON.stringify({ t: Date.now() - t0, kind, ...data }) + '\n');
function check(name, ok, detail) {
  results.push({ name, ok: !!ok }); record('check', { name, ok: !!ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}
const exists = f => fs.existsSync(path.join(REPO, f));

function start() {
  const child = spawn(OPENCODE, ['acp'], { cwd: REPO, env: process.env, stdio: ['pipe', 'pipe', 'pipe'] });
  const agent = { child, next: 1, pending: new Map(), updates: [], onPermission: null, permissions: [] };
  let buf = '';
  child.stdout.on('data', d => {
    buf += d.toString();
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
      if (!line) continue;
      let msg; try { msg = JSON.parse(line); } catch { record('stdout-text', { line: line.slice(0, 500) }); continue; }
      const chunk = msg.method === 'session/update' && /chunk$/.test(msg.params?.update?.sessionUpdate || '');
      if (chunk) record('in-chunk', { sessionUpdate: msg.params.update.sessionUpdate, size: line.length }); else record('in', { msg });
      if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined) && agent.pending.has(msg.id)) {
        const p = agent.pending.get(msg.id); agent.pending.delete(msg.id);
        msg.error ? p.reject(Object.assign(new Error(msg.error.message), { rpc: msg.error })) : p.resolve(msg.result);
      } else if (msg.method === 'session/update') {
        agent.updates.push(msg.params);
      } else if (msg.method === 'session/request_permission') {
        agent.permissions.push(msg.params);
        Promise.resolve(agent.onPermission ? agent.onPermission(msg.params) : { outcome: { outcome: 'cancelled' } }).then(result => send(agent, { jsonrpc: '2.0', id: msg.id, result }));
      } else if (msg.method && msg.id !== undefined) {
        // Any other request from the agent (fs/*, terminal/*): refuse, and record that it asked.
        send(agent, { jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'not supported by the spike client' } });
      }
    }
  });
  child.stderr.on('data', d => record('stderr', { text: d.toString().slice(0, 800) }));
  child.on('exit', (code, signal) => record('exit', { code, signal }));
  return agent;
}
function send(agent, msg) { record('out', { msg }); agent.child.stdin.write(JSON.stringify(msg) + '\n'); }
function call(agent, method, params, ms = 300000) {
  const id = agent.next++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { agent.pending.delete(id); reject(new Error(`timeout: ${method}`)); }, ms);
    agent.pending.set(id, { resolve: v => { clearTimeout(timer); resolve(v); }, reject: e => { clearTimeout(timer); reject(e); } });
    send(agent, { jsonrpc: '2.0', id, method, params });
  });
}
const notify = (agent, method, params) => send(agent, { jsonrpc: '2.0', method, params });
const text = t => [{ type: 'text', text: t }];
const pick = (params, kinds) => params.options.find(o => kinds.includes(o.kind)) || params.options[0];
const said = (agent, sid) => agent.updates.filter(u => u.sessionId === sid && u.update.sessionUpdate === 'agent_message_chunk').map(u => u.update.content?.text || '').join('');

async function open(agent) {
  const init = await call(agent, 'initialize', { protocolVersion: 1, clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false } });
  record('note', { initialize: init });
  return init;
}

const scenarios = {
  async allow(agent, init) {
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    check('acp: session/new returns a session id, its modes and its models', !!s.sessionId, `modes=${JSON.stringify((s.modes?.availableModes || []).map(m => m.id))} current=${s.modes?.currentModeId} model=${s.models?.currentModelId} of ${s.models?.availableModels?.length}`);
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['allow_once']).optionId } });
    const before = agent.permissions.length;
    const r = await call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Use the write tool to create a file named acp-allowed.txt in the current directory containing exactly: hello from acp. Then reply with the single word done.') });
    const asked = agent.permissions.slice(before);
    check('acp: a permission request arrives as a session/request_permission request', asked.length > 0, asked[0] ? `toolCall=${asked[0].toolCall?.toolCallId} kind=${asked[0].toolCall?.kind} options=${JSON.stringify(asked[0].options.map(o => o.kind))}` : 'none');
    check('acp: Allow (allow_once) lets the write through', exists('acp-allowed.txt'), `stopReason=${r.stopReason}; ${asked.length} request(s)`);
    check('acp: the prompt result reports usage', !!(r.usage || r._meta), JSON.stringify(r.usage || r._meta || null));
    fs.writeFileSync(STATE, JSON.stringify({ allow: s.sessionId, loadSession: !!init.agentCapabilities?.loadSession }));
  },
  async deny(agent) {
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['reject_once']).optionId } });
    const r = await call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Use the write tool to create a file named acp-denied.txt in the current directory containing exactly: should not exist. If the tool is rejected, do not try again; reply with the single word refused.') });
    check('acp: Deny (reject_once) blocks the write', !exists('acp-denied.txt'), `stopReason=${r.stopReason}`);
  },
  async plan(agent) {
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    const mode = (s.configOptions || []).find(o => o.category === 'mode');
    const hasPlan = (mode?.options || []).some(o => o.value === 'plan');
    let set; try { set = await call(agent, 'session/set_config_option', { sessionId: s.sessionId, configId: mode.id, value: 'plan' }); } catch (e) { set = { error: e.message }; }
    const now = (set.configOptions || []).find(o => o.category === 'mode')?.currentValue;
    check('acp: the plan agent is offered as a mode and can be selected', hasPlan && now === 'plan', `mode is now ${now}`);
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['reject_once']).optionId } });
    await call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Create a file named acp-plan.txt in the current directory containing exactly: plan mode wrote this. If you cannot, reply with the single word blocked.') });
    check('acp: the plan mode changes no file', !exists('acp-plan.txt'));
  },
  async config(agent) {
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    const opts = s.configOptions || [];
    check('acp: session/new reports config options instead of modes and models', opts.length > 0, opts.map(o => `${o.id} (${o.category}) = ${o.currentValue} of ${o.options?.length}`).join('; '));
    const model = opts.find(o => o.category === 'model');
    const mode = opts.find(o => o.category === 'mode');
    const local = model?.options?.find(o => /^ollama\//.test(o.value));
    let r1; try { r1 = await call(agent, 'session/set_config_option', { sessionId: s.sessionId, configId: model.id, value: local.value }); } catch (e) { r1 = { error: e.message, rpc: e.rpc }; }
    check('acp: the model is chosen with session/set_config_option', !r1?.error, JSON.stringify(r1).slice(0, 160));
    const online = (model?.options || []).filter(o => !/^ollama\//.test(o.value)).map(o => o.value);
    check('acp: only local models are offered in the isolated profile', online.length === 0, `${online.length} online model(s) offered: ${online.slice(0, 4).join(', ')}`);
    let r2 = { skipped: 'no mode option' };
    if (mode) { try { r2 = await call(agent, 'session/set_config_option', { sessionId: s.sessionId, configId: mode.id, value: 'plan' }); } catch (e) { r2 = { error: e.message, rpc: e.rpc }; } }
    check('acp: the agent (plan) is chosen with a config option', mode && !r2?.error, `mode option: ${mode ? mode.id + ' ' + JSON.stringify(mode.options.map(o => o.value)) : 'none'} -> ${JSON.stringify(r2).slice(0, 120)}`);
  },
  async interrupt(agent) {
    // A turn that is reliably long: a 45 s shell command, allowed when it asks.
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['allow_once']).optionId } });
    const before = agent.updates.length;
    const p = call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Run exactly this shell command with the bash tool and nothing else: sleep 45. Then reply with the single word done.') });
    await new Promise((resolve, reject) => { const t = setInterval(() => { if (agent.updates.slice(before).some(u => u.update.sessionUpdate === 'tool_call_update' && u.update.status === 'in_progress' && /sleep 45/.test(JSON.stringify(u.update)))) { clearInterval(t); resolve(); } }, 100); setTimeout(() => { clearInterval(t); reject(new Error('the command never started')); }, 240000); });
    const at = Date.now();
    notify(agent, 'session/cancel', { sessionId: s.sessionId });
    let asRequest; try { asRequest = await call(agent, 'session/cancel', { sessionId: s.sessionId }, 15000); } catch (e) { asRequest = { error: e.message }; }
    const r = await p;
    const took = Date.now() - at;
    check('acp: session/cancel (notification, then request) ends the turn long before the 45 s command would', took < 10000 && r.stopReason === 'cancelled', `stopReason=${r.stopReason} ${took} ms after the cancel; as a request: ${JSON.stringify(asRequest).slice(0, 120)}`);
    const b2 = agent.updates.length;
    await call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Reply with exactly: resumed after interrupt') });
    const after = agent.updates.slice(b2).filter(u => u.update.sessionUpdate === 'agent_message_chunk').map(u => u.update.content?.text || '').join('');
    check('acp: the same session takes a new prompt afterwards', /resumed after interrupt/i.test(after));
  },
  async kill(agent) {
    // The fallback Overseer already uses for `opencode run`: signal the process, then load the
    // session in a new one.
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['allow_once']).optionId } });
    const before = agent.updates.length;
    call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Run exactly this shell command with the bash tool and nothing else: sleep 45. Then reply with the single word done.') }).catch(() => {});
    await new Promise((resolve, reject) => { const t = setInterval(() => { if (agent.updates.slice(before).some(u => u.update.sessionUpdate === 'tool_call_update' && u.update.status === 'in_progress' && /sleep 45/.test(JSON.stringify(u.update)))) { clearInterval(t); resolve(); } }, 100); setTimeout(() => { clearInterval(t); reject(new Error('the command never started')); }, 240000); });
    const at = Date.now();
    const exited = new Promise(resolve => agent.child.once('exit', (code, signal) => resolve({ code, signal })));
    agent.child.kill('SIGINT');
    const how = await Promise.race([exited, new Promise(r => setTimeout(() => r({ timeout: true }), 15000))]);
    check('acp: SIGINT ends the agent process', !how.timeout, `${JSON.stringify(how)} after ${Date.now() - at} ms`);
    const fresh = start(); await open(fresh);
    let l; try { l = await call(fresh, 'session/load', { sessionId: s.sessionId, cwd: REPO, mcpServers: [] }); } catch (e) { l = { error: e.message }; }
    fresh.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['reject_once']).optionId } });
    const b2 = fresh.updates.length;
    if (!l.error) await call(fresh, 'session/prompt', { sessionId: s.sessionId, prompt: text('Reply with exactly: resumed after kill') });
    const after = fresh.updates.slice(b2).filter(u => u.update.sessionUpdate === 'agent_message_chunk').map(u => u.update.content?.text || '').join('');
    check('acp: a new process loads the session and continues it', !l.error && /resumed after kill/i.test(after), l.error || after.slice(0, 80));
    // Later scenarios continue on the new process.
    Object.assign(agent, fresh, { onPermission: null });
  },
  async children(agent) {
    const s = await call(agent, 'session/new', { cwd: REPO, mcpServers: [] });
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['allow_once']).optionId } });
    const before = agent.updates.length;
    await call(agent, 'session/prompt', { sessionId: s.sessionId, prompt: text('Use the task tool exactly once with subagent_type general, description "say hi", and prompt "Reply with exactly: hi from child". Then reply with the single word delegated.') });
    const ups = agent.updates.slice(before);
    const task = ups.filter(u => /^tool_call/.test(u.update.sessionUpdate)).filter(u => /task/i.test(JSON.stringify(u.update)));
    const childIds = [...new Set(JSON.stringify(task).match(/ses_[A-Za-z0-9]+/g) || [])].filter(x => x !== s.sessionId);
    const foreign = [...new Set(ups.map(u => u.sessionId))].filter(x => x !== s.sessionId);
    check('acp: the task tool call is reported', task.length > 0, `${task.length} tool_call update(s)`);
    check('acp: the child session id is visible to the client', childIds.length > 0 || foreign.length > 0, `ids in the tool call: ${JSON.stringify(childIds)}; updates from other sessions: ${JSON.stringify(foreign)}`);
  },
  async load(agent) {
    const st = JSON.parse(fs.readFileSync(STATE, 'utf8'));
    let r; try { r = await call(agent, 'session/load', { sessionId: st.allow, cwd: REPO, mcpServers: [] }); } catch (e) { r = { error: e.message, rpc: e.rpc }; }
    check('acp: a session from an earlier process can be loaded (session/load)', !r?.error, `loadSession capability=${st.loadSession}; ${JSON.stringify(r).slice(0, 200)}`);
    if (r?.error) return;
    const before = agent.updates.length;
    agent.onPermission = p => ({ outcome: { outcome: 'selected', optionId: pick(p, ['reject_once']).optionId } });
    await call(agent, 'session/prompt', { sessionId: st.allow, prompt: text('Which file did you create earlier in this conversation? Reply with only its name.') });
    const after = agent.updates.slice(before).filter(u => u.update.sessionUpdate === 'agent_message_chunk').map(u => u.update.content?.text || '').join('');
    check('acp: the loaded session continues with its history', /acp-allowed\.txt/.test(after), after.slice(0, 120));
  },
};

(async () => {
  if (!REPO) throw new Error('REPO is required');
  const want = process.argv.slice(2).length ? process.argv.slice(2) : ['allow', 'deny', 'plan', 'config', 'interrupt', 'kill', 'children'];
  record('meta', { opencode: OPENCODE, scenarios: want });
  const agent = start();
  const init = await open(agent);
  check('acp: initialize answers with the agent capabilities', init.protocolVersion !== undefined, `protocolVersion=${init.protocolVersion} capabilities=${JSON.stringify(init.agentCapabilities)}`);
  for (const name of want) {
    console.log(`\n== ${name}`);
    record('scenario', { name, state: 'start' });
    try { await scenarios[name](agent, init); } catch (e) { check(`acp: scenario ${name} completed`, false, `${e.message} ${JSON.stringify(e.rpc || '')}`); }
    record('scenario', { name, state: 'end' });
  }
  const failed = results.filter(r => !r.ok).length;
  record('summary', { passed: results.length - failed, failed });
  console.log(`\n${results.length - failed} passed, ${failed} failed`);
  agent.child.stdin.end(); agent.child.kill('SIGTERM');
  setTimeout(() => process.exit(0), 300);
})().catch(e => { console.error(e); record('fatal', { message: e.message }); process.exit(1); });
