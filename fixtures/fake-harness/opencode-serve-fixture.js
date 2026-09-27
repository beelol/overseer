#!/usr/bin/env node
// SYNTHETIC OpenCode server (not the real harness, and no model): answers `--version` and
// `serve --hostname H --port P` with the parts of OpenCode 1.15's HTTP API that Overseer's bridge
// uses, in the shapes recorded by the AC-139 spike. The prompt is a script, steps separated by ";":
//
//   write <file> <text>     the write tool (permission `edit`)
//   bash <command>          the bash tool (permission `bash`); the command really runs in the cwd
//   sleep <seconds>         the bash tool running `sleep <seconds>`; an abort ends it
//   say <text>              a reply
//   recall                  a reply naming the files this session wrote (also in earlier turns)
//   child <text>            the task tool: a child session that replies <text>
//   question                the question tool (permission `question`); waits for ever when allowed
//   astext <file> <text>    a reply that is a tool call written as text; after Overseer's nudge the
//                           file is written for real
//   astext-always           the same, and again after the nudge
//   fail <message>          the session fails with <message>
//
// Permission rules come from the session (POST or PATCH /session); the last rule naming a
// permission wins. Without a rule the `build` agent allows and the `plan` agent denies edits.
// Every request is appended to $XDG_DATA_HOME/opencode/fixture-requests.jsonl, and sessions are
// kept in fixture-sessions.json beside it, so a later server continues them.
'use strict';
const fs = require('fs');
const http = require('http');
const path = require('path');
const { spawn } = require('child_process');

const args = process.argv.slice(2);
if (args[0] === '--version') { console.log('1.15.13-fixture'); process.exit(0); }
if (args[0] !== 'serve') { console.error('the fixture only serves'); process.exit(2); }
const port = Number(args[args.indexOf('--port') + 1] || 0);
const data = path.join(process.env.XDG_DATA_HOME || '/tmp', 'opencode');
fs.mkdirSync(data, { recursive: true });
const storeFile = path.join(data, 'fixture-sessions.json');
const logFile = path.join(data, 'fixture-requests.jsonl');
const store = fs.existsSync(storeFile) ? JSON.parse(fs.readFileSync(storeFile, 'utf8')) : { sessions: {}, n: 0 };
const save = () => fs.writeFileSync(storeFile, JSON.stringify(store));
const password = process.env.OPENCODE_SERVER_PASSWORD || '';
let config = null;
try { config = JSON.parse(fs.readFileSync(path.join(process.env.XDG_CONFIG_HOME || '/nonexistent', 'opencode/opencode.json'), 'utf8')); } catch { /* none */ }
fs.appendFileSync(logFile, JSON.stringify({ start: true, cwd: process.cwd(), password: !!password, config }) + '\n');

const streams = new Set();
let n = 0;
const id = prefix => `${prefix}_fixture${String(++store.n).padStart(4, '0')}${Date.now().toString(36)}`;
function emit(type, properties) {
  const line = `data: ${JSON.stringify({ id: `evt_${++n}`, type, properties })}\n\n`;
  for (const s of streams) s.write(line);
}
const pending = new Map();   // permission id -> resolve(reply)
const running = new Map();   // session id -> { aborted, kill }
const sleep = ms => new Promise(r => setTimeout(r, ms));

function action(session, permission) {
  const rules = session.rules || [];
  const rule = [...rules].reverse().find(r => r.permission === permission && r.pattern === '*');
  if (rule) return rule.action;
  if (session.agent === 'plan' && permission === 'edit') return 'deny';
  if (permission === 'question') return 'allow';
  return 'allow';
}

async function ask(sid, permission, patterns, metadata, callID, messageID) {
  const pid = id('per');
  const reply = new Promise(resolve => pending.set(pid, { resolve, sessionID: sid, permission, patterns, metadata, always: [], tool: { messageID, callID } }));
  emit('permission.asked', { id: pid, sessionID: sid, permission, patterns, metadata, always: [], tool: { messageID, callID } });
  const r = await reply;
  emit('permission.replied', { sessionID: sid, requestID: pid, reply: r });
  return r;
}

async function turn(sid, body) {
  const session = store.sessions[sid];
  const state = { aborted: false, kill: null };
  running.set(sid, state);
  const text = body.parts.map(p => p.text || '').join('\n');
  const model = body.model || { providerID: 'ollama', modelID: 'fixture' };
  if (body.agent) session.agent = body.agent;
  const user = id('msg');
  emit('message.updated', { sessionID: sid, info: { id: user, sessionID: sid, role: 'user', time: { created: Date.now() } } });
  emit('message.part.updated', { sessionID: sid, part: { id: id('prt'), sessionID: sid, messageID: user, type: 'text', text }, time: Date.now() });
  emit('session.status', { sessionID: sid, status: { type: 'busy' } });
  const msg = id('msg');
  const info = () => ({ id: msg, sessionID: sid, role: 'assistant', parentID: user, modelID: model.modelID, providerID: model.providerID, mode: session.agent, agent: session.agent, path: { cwd: process.cwd(), root: process.cwd() }, cost: 0, tokens: { total: 120, input: 100, output: 20, reasoning: 0, cache: { read: 0, write: 0 } }, time: { created: Date.now() } });
  emit('message.updated', { sessionID: sid, info: { ...info(), tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } } } });
  const part = (p) => emit('message.part.updated', { sessionID: sid, part: { id: p.id || id('prt'), sessionID: sid, messageID: msg, ...p }, time: Date.now() });
  const say = t => { const pid = id('prt'); part({ id: pid, type: 'text', text: '', time: { start: Date.now() } }); part({ id: pid, type: 'text', text: t, time: { start: Date.now(), end: Date.now() } }); };
  const tool = async (name, input, permission, patterns, metadata, run) => {
    const callID = id('call'), pid = id('prt');
    const at = Date.now();
    part({ id: pid, type: 'tool', tool: name, callID, state: { status: 'pending', input: {}, raw: '' } });
    part({ id: pid, type: 'tool', tool: name, callID, state: { status: 'running', input, time: { start: at } } });
    const fail = error => part({ id: pid, type: 'tool', tool: name, callID, state: { status: 'error', input, error, time: { start: at, end: Date.now() } } });
    const act = action(session, permission);
    if (act === 'deny') return fail(`The user has specified a rule which prevents you from using this specific tool call. (${permission})`);
    if (act === 'ask' && (await ask(sid, permission, patterns, metadata, callID, msg)) === 'reject') return fail('The user rejected permission to use this specific tool call.');
    if (state.aborted) return fail('Tool execution aborted');
    const output = await run(callID);
    if (state.aborted) return fail('Tool execution aborted');
    part({ id: pid, type: 'tool', tool: name, callID, state: { status: 'completed', input, output: output.text, title: output.title || name, metadata: output.metadata || {}, time: { start: at, end: Date.now() } } });
  };
  const write = async (file, content) => {
    const abs = path.resolve(process.cwd(), file);
    await tool('write', { filePath: abs, content }, 'edit', [file], { filepath: abs, diff: `Index: ${abs}\n===\n--- ${abs}\n+++ ${abs}\n@@ -0,0 +1,1 @@\n+${content}\n` }, async () => {
      fs.mkdirSync(path.dirname(abs), { recursive: true });
      fs.writeFileSync(abs, content + '\n');
      session.files = [...new Set([...(session.files || []), file])]; save();
      emit('file.edited', { file: abs });
      return { text: 'Wrote file successfully.', title: file, metadata: { filepath: abs, exists: false } };
    });
  };
  const bash = async command => tool('bash', { command, description: 'fixture' }, 'bash', [command], {}, () => new Promise(resolve => {
    const child = spawn('/bin/sh', ['-c', command], { cwd: process.cwd() });
    let out = '';
    state.kill = () => child.kill('SIGKILL');
    child.stdout.on('data', d => { out += d; }); child.stderr.on('data', d => { out += d; });
    child.on('close', code => { state.kill = null; resolve({ text: out, title: command, metadata: { exit: code } }); });
  }));
  let failed = null, nudged = /wrote a tool call out as text/.test(text);
  const steps = nudged ? (session.afterNudge || ['say done']) : text.split(';').map(s => s.trim()).filter(Boolean);
  for (const step of steps) {
    if (state.aborted) break;
    const [verb, ...rest] = step.split(' ');
    const arg = rest.join(' ');
    await sleep(15);
    if (verb === 'write') await write(rest[0], rest.slice(1).join(' '));
    else if (verb === 'bash') await bash(arg);
    else if (verb === 'sleep') await bash(`sleep ${arg}`);
    else if (verb === 'say') say(arg);
    else if (verb === 'recall') say((session.files || []).join(', ') || 'nothing yet');
    else if (verb === 'astext' || verb === 'astext-always') {
      session.afterNudge = verb === 'astext' ? [`write ${arg}`, 'say done'] : ['astext-always']; save();
      say(`<function=write>\n<parameter=filePath>\n${rest[0] || 'x.txt'}\n</parameter>\n</function>\n</tool_call>`);
    } else if (verb === 'question') {
      await tool('question', { questions: [{ header: 'Action', options: [] }] }, 'question', ['*'], {}, () => new Promise(() => {}));
    } else if (verb === 'child') {
      const cid = id('ses');
      store.sessions[cid] = { title: `${arg} (@general subagent)`, parentID: sid, agent: 'general', rules: session.rules }; save();
      await tool('task', { description: arg, prompt: arg, subagent_type: 'general' }, 'task', ['general'], {}, async () => {
        const cinfo = { id: cid, slug: 'fixture-child', projectID: 'fixture', directory: process.cwd(), parentID: sid, title: store.sessions[cid].title, version: '1.15.13-fixture', time: { created: Date.now() } };
        emit('session.created', { sessionID: cid, info: cinfo });
        const cu = id('msg'), ca = id('msg');
        emit('message.updated', { sessionID: cid, info: { id: cu, sessionID: cid, role: 'user', time: { created: Date.now() } } });
        emit('message.part.updated', { sessionID: cid, part: { id: id('prt'), sessionID: cid, messageID: cu, type: 'text', text: arg }, time: Date.now() });
        emit('session.status', { sessionID: cid, status: { type: 'busy' } });
        emit('message.updated', { sessionID: cid, info: { id: ca, sessionID: cid, role: 'assistant', time: { created: Date.now() }, tokens: { total: 0, input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } }, cost: 0 } });
        emit('message.part.updated', { sessionID: cid, part: { id: id('prt'), sessionID: cid, messageID: ca, type: 'text', text: arg, time: { start: Date.now(), end: Date.now() } }, time: Date.now() });
        emit('message.updated', { sessionID: cid, info: { id: ca, sessionID: cid, role: 'assistant', time: { created: Date.now(), completed: Date.now() }, tokens: { total: 50, input: 40, output: 10, reasoning: 0, cache: { read: 0, write: 0 } }, cost: 0, providerID: model.providerID, modelID: model.modelID, agent: 'general' } });
        emit('session.idle', { sessionID: cid });
        return { text: `<task id="${cid}" state="completed">\n<task_result>\n${arg}\n</task_result>\n</task>`, title: arg, metadata: { parentSessionId: sid, sessionId: cid } };
      });
    } else if (verb === 'fail') { failed = { name: 'APIError', data: { message: arg } }; break; }
  }
  if (state.aborted) failed = { name: 'MessageAbortedError', data: { message: 'Aborted' } };
  if (failed) emit('session.error', { sessionID: sid, error: failed });
  emit('message.updated', { sessionID: sid, info: { ...info(), time: { created: Date.now(), completed: Date.now() }, finish: failed ? undefined : 'stop', ...(failed ? { error: failed } : {}) } });
  running.delete(sid);
  emit('session.status', { sessionID: sid, status: { type: 'idle' } });
  emit('session.idle', { sessionID: sid });
}

const server = http.createServer((req, res) => {
  const send = (code, body) => { const t = body === undefined ? '' : JSON.stringify(body); res.writeHead(code, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(t) }); res.end(t); };
  if (password && req.headers.authorization !== 'Basic ' + Buffer.from(`opencode:${password}`).toString('base64')) return send(401, { error: 'unauthorized' });
  const url = new URL(req.url, 'http://x');
  let raw = '';
  req.on('data', d => { raw += d; });
  req.on('end', () => {
    const body = raw ? JSON.parse(raw) : null;
    if (url.pathname !== '/event') fs.appendFileSync(logFile, JSON.stringify({ method: req.method, path: url.pathname, body }) + '\n');
    const m = (re) => re.exec(url.pathname);
    let x;
    if (url.pathname === '/global/health') return send(200, { healthy: true, version: '1.15.13-fixture' });
    if (url.pathname === '/event') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
      res.write(`data: ${JSON.stringify({ id: 'evt_0', type: 'server.connected', properties: {} })}\n\n`);
      streams.add(res);
      res.on('close', () => streams.delete(res));
      return;
    }
    if (req.method === 'POST' && url.pathname === '/session') {
      const sid = id('ses');
      store.sessions[sid] = { title: body.title, agent: body.agent || 'build', rules: body.permission || [], files: [] }; save();
      const info = { id: sid, slug: 'fixture', projectID: 'fixture', directory: process.cwd(), title: body.title, version: '1.15.13-fixture', time: { created: Date.now() }, permission: body.permission };
      emit('session.created', { sessionID: sid, info });
      return send(200, info);
    }
    if ((x = m(/^\/session\/([^/]+)$/))) {
      const s = store.sessions[x[1]];
      if (!s) return send(404, { _tag: 'NotFoundError', message: 'session not found' });
      if (req.method === 'PATCH') { if (body.permission) s.rules = body.permission; save(); }
      return send(200, { id: x[1], title: s.title, directory: process.cwd(), permission: s.rules });
    }
    if (req.method === 'POST' && (x = m(/^\/session\/([^/]+)\/prompt_async$/))) {
      if (!store.sessions[x[1]]) return send(404, { _tag: 'NotFoundError' });
      turn(x[1], body).catch(e => emit('session.error', { sessionID: x[1], error: { name: 'UnknownError', data: { message: String(e) } } }));
      res.writeHead(204); return res.end();
    }
    if (req.method === 'POST' && (x = m(/^\/session\/([^/]+)\/abort$/))) {
      const r = running.get(x[1]);
      if (r) { r.aborted = true; if (r.kill) r.kill(); for (const [pid, p] of pending) if (p.sessionID === x[1]) { pending.delete(pid); p.resolve('reject'); } }
      return send(200, true);
    }
    if (req.method === 'GET' && url.pathname === '/permission') return send(200, [...pending].map(([pid, p]) => ({ id: pid, sessionID: p.sessionID, permission: p.permission, patterns: p.patterns, metadata: p.metadata, always: [], tool: p.tool })));
    if (req.method === 'POST' && (x = m(/^\/permission\/([^/]+)\/reply$/))) {
      const p = pending.get(x[1]);
      if (!p) return send(404, { _tag: 'PermissionNotFoundError', requestID: x[1], message: 'not found' });
      pending.delete(x[1]); p.resolve(body.reply);
      return send(200, true);
    }
    send(404, { error: `the fixture does not answer ${req.method} ${url.pathname}` });
  });
});
server.listen(port, '127.0.0.1', () => console.log(`opencode server listening on http://127.0.0.1:${server.address().port}`));
for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => { for (const r of running.values()) if (r.kill) r.kill(); process.exit(0); });
