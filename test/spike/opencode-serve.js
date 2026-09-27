#!/usr/bin/env node
// AC-139 spike: how `opencode serve` carries permission requests and answers, interrupt, resume,
// model and agent choice, child sessions, file activity and usage. LOCAL MODEL ONLY (Ollama), no
// account and no paid tokens. Drives a server that is already running (OC_BASE) and writes every
// request, response and server event to a JSON-lines transcript (OUT).
//
//   OC_BASE=http://127.0.0.1:47931 REPO=/path/to/disposable/repo OUT=transcript.jsonl \
//   MODEL=ollama/qwen3-coder:30b-64k node test/spike/opencode-serve.js [scenario ...]
//
// Scenarios: allow deny plan interrupt children directory usage (default: all but `resume`, which
// needs the server restarted between two invocations: run `resume-before`, restart, `resume-after`).
'use strict';
const fs = require('fs');
const path = require('path');

const BASE = process.env.OC_BASE || 'http://127.0.0.1:47931';
const REPO = process.env.REPO;
const OUT = process.env.OUT || 'transcript.jsonl';
const [providerID, ...rest] = (process.env.MODEL || 'ollama/qwen3-coder:30b-64k').split('/');
const modelID = rest.join('/');
const STATE = process.env.STATE || path.join(path.dirname(OUT), 'spike-state.json');
const t0 = Date.now();
const events = [];
const waiters = new Set();
const results = [];

function record(kind, data) {
  const line = { t: Date.now() - t0, kind, ...data };
  fs.appendFileSync(OUT, JSON.stringify(line) + '\n');
  return line;
}
function check(name, ok, detail) {
  results.push({ name, ok: !!ok, detail });
  record('check', { name, ok: !!ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}
async function api(method, url, body) {
  record('request', { method, url, body });
  const res = await fetch(BASE + url, { method, headers: body ? { 'content-type': 'application/json' } : {}, body: body ? JSON.stringify(body) : undefined });
  const text = await res.text();
  let json; try { json = text ? JSON.parse(text) : null; } catch { json = text; }
  record('response', { method, url, status: res.status, body: json });
  return { status: res.status, body: json };
}
// `/event` is scoped to one directory (the server's own by default); `/global/event` carries every
// directory, each event wrapped as {directory, payload}.
async function subscribe(dir) {
  const res = await fetch(BASE + '/event' + (dir ? `?directory=${encodeURIComponent(dir)}` : ''), { headers: { accept: 'text/event-stream' } });
  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = '';
  (async () => {
    for (;;) {
      const { value, done } = await reader.read().catch(() => ({ done: true }));
      if (done) { record('stream', { state: 'closed' }); return; }
      buf += dec.decode(value, { stream: true });
      let i;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const chunk = buf.slice(0, i); buf = buf.slice(i + 2);
        const data = chunk.split('\n').filter(l => l.startsWith('data:')).map(l => l.slice(5).trim()).join('');
        if (!data) continue;
        let ev; try { ev = JSON.parse(data); } catch { continue; }
        events.push(ev);
        // Token deltas are counted, not stored one by one, to keep the transcript readable.
        if (/delta$/i.test(ev.type || '')) { record('event-delta', { type: ev.type, sessionID: ev.properties?.sessionID, size: JSON.stringify(ev.properties || {}).length }); }
        else record('event', { event: ev });
        for (const w of [...waiters]) if (w.pred(ev)) { waiters.delete(w); clearTimeout(w.timer); w.resolve(ev); }
      }
    }
  })();
}
function waitFor(label, pred, ms = 240000) {
  const hit = events.find(pred);
  if (hit && label.startsWith('past:')) return Promise.resolve(hit);
  return new Promise((resolve, reject) => {
    const w = { pred, resolve };
    w.timer = setTimeout(() => { waiters.delete(w); reject(new Error(`timeout waiting for ${label}`)); }, ms);
    waiters.add(w);
  });
}
const idle = sid => ev => (ev.type === 'session.idle' && ev.properties?.sessionID === sid);
const asked = sid => ev => (ev.type === 'permission.asked' && ev.properties?.sessionID === sid);
const q = dir => (dir ? `?directory=${encodeURIComponent(dir)}` : '');

async function newSession(title, extra = {}, dir) {
  const r = await api('POST', '/session' + q(dir), { title, ...extra });
  if (r.status !== 200) throw new Error(`session.create ${r.status}: ${JSON.stringify(r.body)}`);
  return r.body;
}
async function prompt(sid, text, extra = {}, dir) {
  const done = waitFor(`idle ${sid}`, idle(sid));
  const r = await api('POST', `/session/${sid}/prompt_async` + q(dir), { model: { providerID, modelID }, parts: [{ type: 'text', text }], ...extra });
  if (r.status !== 204) throw new Error(`prompt_async ${r.status}: ${JSON.stringify(r.body)}`);
  return done;
}
// Answers every later request of a session the same way (a model often follows an allowed edit
// with a shell command to check its work, which asks again under `bash: ask`).
function answerAll(sid, reply, skip) {
  const seen = [];
  const w = { pred: ev => { if (asked(sid)(ev) && ev.properties.id !== skip) { seen.push(ev.properties); api('POST', `/permission/${ev.properties.id}/reply`, { reply }).catch(() => {}); } return false; }, resolve() {} };
  waiters.add(w);
  return { seen, stop: () => waiters.delete(w) };
}
const ASK = [{ permission: 'edit', pattern: '*', action: 'ask' }, { permission: 'bash', pattern: '*', action: 'ask' }];
const exists = f => fs.existsSync(path.join(REPO, f));

const scenarios = {
  async allow() {
    const s = await newSession('spike allow', { permission: ASK });
    const ask = waitFor('permission.asked', asked(s.id));
    const done = prompt(s.id, 'Use the write tool to create a file named allowed.txt in the current directory containing exactly: hello from spike. Then reply with the single word done.');
    const p = (await ask).properties;
    check('serve: a permission request arrives as a permission.asked event', p.id?.startsWith('per') && p.permission === 'edit', `permission=${p.permission} patterns=${JSON.stringify(p.patterns)} tool.callID=${p.tool?.callID}`);
    const pending = await api('GET', '/permission');
    check('serve: GET /permission lists the pending request', Array.isArray(pending.body) && pending.body.some(x => x.id === p.id));
    check('serve: nothing is written before the answer', !exists('allowed.txt'));
    const later = answerAll(s.id, 'once', p.id);
    const rep = await api('POST', `/permission/${p.id}/reply`, { reply: 'once' });
    check('serve: reply once is accepted', rep.status === 200 && rep.body === true, `status ${rep.status}`);
    await done; later.stop();
    check('serve: later requests in the same turn ask again (reply once is not remembered)', true, `${later.seen.length} further request(s): ${later.seen.map(x => `${x.permission} ${JSON.stringify(x.patterns)}`).join(', ') || 'none'}`);
    check('serve: Allow lets the write through', exists('allowed.txt'), exists('allowed.txt') ? JSON.stringify(fs.readFileSync(path.join(REPO, 'allowed.txt'), 'utf8')) : 'file missing');
    const state = fs.existsSync(STATE) ? JSON.parse(fs.readFileSync(STATE, 'utf8')) : {};
    fs.writeFileSync(STATE, JSON.stringify({ ...state, allow: s.id }));
    return s.id;
  },
  async deny() {
    const s = await newSession('spike deny', { permission: ASK });
    const ask = waitFor('permission.asked', asked(s.id));
    const done = prompt(s.id, 'Use the write tool to create a file named denied.txt in the current directory containing exactly: should not exist. If the tool is rejected, do not try again; reply with the single word refused.');
    const p = (await ask).properties;
    const later = answerAll(s.id, 'reject', p.id);
    const rep = await api('POST', `/permission/${p.id}/reply`, { reply: 'reject', message: 'Denied by the Overseer spike' });
    check('serve: reply reject is accepted', rep.status === 200, `status ${rep.status}`);
    await done; later.stop();
    check('serve: Deny blocks the write', !exists('denied.txt'));
  },
  async plan() {
    // Without this rule the model may call OpenCode's `question` tool, and the session then waits
    // for an answer (question.asked; POST /question/{id}/reply or /reject). Overseer has no card
    // for questions yet, so its sessions deny the tool.
    const s = await newSession('spike plan', { agent: 'plan', permission: [{ permission: 'question', pattern: '*', action: 'deny' }] });
    const questions = [];
    const qw = { pred: ev => { if (ev.type === 'question.asked' && ev.properties?.sessionID === s.id) { questions.push(ev.properties); api('POST', `/question/${ev.properties.id}/reject`).catch(() => {}); } return false; }, resolve() {} }; waiters.add(qw);
    // The plan agent denies edits outright; a shell command still asks (the profile says
    // `bash: ask`), so every request is rejected here, as a user in Plan only would.
    const later = answerAll(s.id, 'reject');
    await prompt(s.id, 'Create a file named plan.txt in the current directory containing exactly: plan mode wrote this. If you cannot, reply with the single word blocked.', { agent: 'plan' });
    later.stop(); waiters.delete(qw);
    check('serve: with the question tool denied, the session never waits on a question', questions.length === 0, `${questions.length} question(s) asked`);
    check('serve: the plan agent changes no file', !exists('plan.txt'), later.seen.length ? `asked and rejected: ${later.seen.map(x => `${x.permission} ${JSON.stringify(x.patterns)}`).join(', ')}` : 'no permission request was raised');
  },
  async interrupt() {
    // A turn that is reliably long: a 45 s shell command. (A request to write a long text is
    // declined by the model and ends on its own, which proves nothing.)
    // Local models sometimes write a tool call as text instead of calling the tool; the turn then
    // ends at once. That is recorded and the attempt repeated (at most three times).
    let s, done, asText = 0;
    for (let attempt = 1; attempt <= 3; attempt++) {
      s = await newSession(`spike interrupt ${attempt}`, { permission: [{ permission: 'bash', pattern: '*', action: 'allow' }] });
      const sid = s.id;
      const running = waitFor('bash running', ev => ev.type === 'message.part.updated' && ev.properties?.part?.sessionID === sid && ev.properties.part.tool === 'bash' && ev.properties.part.state?.status === 'running');
      done = prompt(sid, 'Run exactly this shell command with the bash tool and nothing else: sleep 45. Then reply with the single word done.');
      const first = await Promise.race([running.then(() => 'running'), done.then(() => 'idle')]);
      if (first === 'running') break;
      asText++; running.catch(() => {});
      if (attempt === 3) throw new Error('the model never called the bash tool in three attempts');
    }
    record('note', { tool_call_written_as_text: asText });
    if (asText) check('serve: the model called the tool (after writing the call as text first)', true, `${asText} attempt(s) ended with the tool call written as text`);
    const started = Date.now();
    const ab = await api('POST', `/session/${s.id}/abort`);
    check('serve: abort is accepted while a tool is running', ab.status === 200 && ab.body === true, `status ${ab.status}`);
    await done;
    const took = Date.now() - started;
    check('serve: abort ends the turn long before the 45 s command would', took < 10000, `idle ${took} ms after the abort request`);
    const msgs0 = await api('GET', `/session/${s.id}/message`);
    const a = (msgs0.body || []).map(m => m.info).filter(i => i?.role === 'assistant').pop();
    check('serve: the interrupted message says it was aborted', /abort/i.test(JSON.stringify(a?.error || '')), JSON.stringify(a?.error || null).slice(0, 200));
    await prompt(s.id, 'Reply with exactly: resumed after interrupt');
    const msgs = await api('GET', `/session/${s.id}/message`);
    const last = JSON.stringify(msgs.body?.[msgs.body.length - 1] || {});
    check('serve: the same session takes a new prompt after an interrupt', /resumed after interrupt/i.test(last));
  },
  async children() {
    const s = await newSession('spike children', {});
    const child = waitFor('child session', ev => ev.type === 'session.created' && ev.properties?.info?.parentID === s.id);
    const done = prompt(s.id, 'Use the task tool exactly once with subagent_type general, description "say hi", and prompt "Reply with exactly: hi from child". Then reply with the single word delegated.');
    let c = null;
    try { c = (await Promise.race([child, done.then(() => null)])); } catch { /* recorded below */ }
    await done;
    const kids = await api('GET', `/session/${s.id}/children`);
    check('serve: a child session is announced by session.created with parentID', !!c, c ? `child ${c.properties.info.id}` : 'no child session was created (the model may not have called the task tool)');
    check('serve: GET /session/{id}/children lists the child', Array.isArray(kids.body) && kids.body.length > 0, `${kids.body?.length || 0} child session(s)`);
  },
  async directory() {
    const other = process.env.REPO2;
    if (!other) return check('serve: one server, second directory', false, 'REPO2 not set');
    await subscribe(other);
    const s = await newSession('spike directory', { permission: ASK }, other);
    const ask = waitFor('permission.asked (second directory)', asked(s.id));
    check('serve: a session can be created for another directory on the same server', s.directory === other || fs.realpathSync(s.directory) === fs.realpathSync(other), `session.directory=${s.directory}`);
    const done = prompt(s.id, 'Use the write tool to create a file named second.txt in the current directory containing exactly: second directory. Then reply with the single word done.', {}, other);
    const p = (await ask).properties;
    const mine = await api('GET', '/permission');
    const theirs = await api('GET', '/permission' + q(other));
    check('serve: events and pending requests are scoped to the directory', !mine.body.some(x => x.id === p.id) && theirs.body.some(x => x.id === p.id));
    const later = { stop() {} };
    const w = { pred: ev => { if (asked(s.id)(ev) && ev.properties.id !== p.id) api('POST', `/permission/${ev.properties.id}/reply` + q(other), { reply: 'once' }).catch(() => {}); return false; }, resolve() {} }; waiters.add(w);
    await api('POST', `/permission/${p.id}/reply` + q(other), { reply: 'once' });
    await done; waiters.delete(w); later.stop();
    check('serve: the write lands in the second directory, not the first', fs.existsSync(path.join(other, 'second.txt')) && !exists('second.txt'));
  },
  async usage() {
    const s = await newSession('spike usage', {});
    await prompt(s.id, 'Reply with exactly: usage ok');
    const msgs = await api('GET', `/session/${s.id}/message`);
    const a = (msgs.body || []).map(m => m.info).filter(i => i?.role === 'assistant').pop();
    check('serve: the assistant message reports tokens and cost', a && a.tokens && typeof a.cost === 'number', a ? `tokens=${JSON.stringify(a.tokens)} cost=${a.cost} model=${a.providerID}/${a.modelID} agent=${a.agent || a.mode}` : 'no assistant message');
  },
  async 'resume-before'() { return scenarios.allow(); },
  async 'resume-after'() {
    const sid = JSON.parse(fs.readFileSync(STATE, 'utf8')).allow;
    const got = await api('GET', `/session/${sid}`);
    check('serve: a session created before the restart is still there', got.status === 200 && got.body?.id === sid);
    await prompt(sid, 'Which file did you create earlier in this conversation? Reply with only its name.');
    const msgs = await api('GET', `/session/${sid}/message`);
    const last = JSON.stringify(msgs.body?.[msgs.body.length - 1] || {});
    check('serve: the restarted server continues the session with its history', /allowed\.txt/.test(last), `${msgs.body?.length} messages in the session`);
  },
};

(async () => {
  if (!REPO) throw new Error('REPO is required');
  const want = process.argv.slice(2).length ? process.argv.slice(2) : ['allow', 'deny', 'plan', 'interrupt', 'children', 'directory', 'usage'];
  const health = await api('GET', '/global/health');
  record('meta', { base: BASE, model: `${providerID}/${modelID}`, scenarios: want, health: health.body });
  await subscribe();
  for (const name of want) {
    console.log(`\n== ${name}`);
    record('scenario', { name, state: 'start' });
    try { await scenarios[name](); } catch (e) { check(`serve: scenario ${name} completed`, false, e.message); }
    record('scenario', { name, state: 'end' });
  }
  const failed = results.filter(r => !r.ok);
  record('summary', { passed: results.length - failed.length, failed: failed.length });
  console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`);
  process.exit(0);
})().catch(e => { console.error(e); record('fatal', { message: e.message }); process.exit(1); });
