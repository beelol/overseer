#!/usr/bin/env node
// Records what a real overseerd says, as fixtures for the phone's view models.
//
//   node scripts/record.mjs                 every scenario
//   node scripts/record.mjs showcase nested  only these
//
// Each scenario gets its own daemon with its own OVERSEER_HOME in a temporary directory, its own
// Git repositories, and fixture harnesses only (fixtures/fake-harness). Nothing here can reach the
// owner's daemon, a real harness or a paid account. For every scenario the file holds:
//   initial      `state` before the first agent
//   events       every event of the stream, in order
//   checkpoints  `state` after every 10 events, so also after every 50 (and at moments the scenario marks)
//   final        `state` at the end
// Paths of the temporary directory, of this repository and of the home folder are replaced by
// fixed ones, so a fixture does not say where it was recorded.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../../..');
const out = path.resolve(here, '../test/fixtures');
const bin = path.join(repoRoot, 'target/debug/overseerd');
const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
const NOWHERE = '/nonexistent/harness-disabled-in-recordings';
const EVERY = 10;
const sleep = ms => new Promise(r => setTimeout(r, ms));

if (!fs.existsSync(bin)) {
  console.log('building overseerd (cargo may wait on a lock)…');
  execFileSync('cargo', ['build', '-p', 'overseerd'], { cwd: repoRoot, stdio: 'inherit' });
}

/** One connection to the daemon: newline-delimited JSON over its Unix socket. */
class Connection {
  constructor(socketPath) {
    this.socketPath = socketPath; this.nextId = 1; this.pending = new Map(); this.buffer = ''; this.onEvent = () => {}; this.onReplayed = () => {};
  }
  open() {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath);
      socket.setEncoding('utf8');
      socket.once('connect', () => { this.socket = socket; resolve(this); });
      socket.once('error', reject);
      socket.on('data', chunk => {
        this.buffer += chunk;
        for (let i = this.buffer.indexOf('\n'); i >= 0; i = this.buffer.indexOf('\n')) {
          const line = this.buffer.slice(0, i); this.buffer = this.buffer.slice(i + 1);
          if (!line) continue;
          const msg = JSON.parse(line);
          if (msg.method === 'event') this.onEvent(msg.params);
          else if (msg.method === 'replayed') this.onReplayed(msg.params);
          else if (msg.method === 'resync') throw new Error('the event stream lagged while recording; record again');
          else if (msg.id !== undefined && this.pending.has(msg.id)) {
            const { resolve: ok, reject: fail } = this.pending.get(msg.id); this.pending.delete(msg.id);
            if (msg.error) fail(Object.assign(new Error(msg.error.message || msg.error.code), { code: msg.error.code })); else ok(msg.result);
          }
        }
      });
    });
  }
  request(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => { this.pending.set(id, { resolve, reject }); this.socket.write(JSON.stringify({ id, method, params }) + '\n'); });
  }
  close() { this.socket?.destroy(); }
}

function git(dir, ...args) { return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim(); }

function makeRepo(dir) {
  fs.mkdirSync(dir, { recursive: true });
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.name', 'Fixture');
  git(dir, 'config', 'user.email', 'fixture@example.invalid');
  git(dir, 'config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'README.md'), '# Fixture repository\n\nUsed by recorded scenarios.\n\n## Sign-in\n\nSessions are validated on every request.\n');
  fs.writeFileSync(path.join(dir, 'a.txt'), 'a\n');
  fs.writeFileSync(path.join(dir, 'b.txt'), 'b\n');
  fs.writeFileSync(path.join(dir, '.gitignore'), 'ignored/\n*.log\n');
  git(dir, 'add', '.');
  git(dir, 'commit', '-q', '-m', 'base');
  return fs.realpathSync(dir);
}

/** A daemon of its own, the stream of its events, and the helpers a scenario uses. */
class Session {
  constructor(name, env = {}) { this.name = name; this.extraEnv = env; this.events = []; this.checkpoints = []; this.marks = {}; this.waiters = []; this.pendingCheckpoints = []; }

  async start() {
    const base = process.env.OVERSEER_RECORD_TMP || os.tmpdir();
    fs.mkdirSync(base, { recursive: true });
    this.root = fs.realpathSync(fs.mkdtempSync(path.join(base, 'ovs-rec-')));
    this.home = path.join(this.root, 'home');
    this.modeFile = path.join(this.root, 'mode');
    fs.mkdirSync(this.home, { recursive: true });
    fs.mkdirSync(path.join(this.root, 'sys'), { recursive: true });
    const env = { ...process.env };
    for (const k of Object.keys(env)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_RECORD_TMP') delete env[k];
    Object.assign(env, {
      OVERSEER_HOME: this.home, OVERSEER_GATEWAY_MDNS: 'off', OVERSEER_NOTIFY_COMMAND: '/usr/bin/true', OVERSEER_TEST_SYSTEM_HOME: path.join(this.root, 'sys'),
      OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: NOWHERE, OVERSEER_OPENCODE_PATH: NOWHERE,
      CLAUDE_FIXTURE_MODE_FILE: this.modeFile, FIXTURE_SLOW_MS: '5000',
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS,FIXTURE_MODE,REPLAY_FILE,REPLAY_DELAY_MS',
      ...this.extraEnv,
    });
    this.env = env;
    this.child = spawn(bin, ['serve'], { env, stdio: 'ignore' });
    this.socketPath = execFileSync(bin, ['socket-path'], { env, encoding: 'utf8' }).trim();
    for (let i = 0; ; i++) {
      try { this.rpc = await new Connection(this.socketPath).open(); await this.rpc.request('hello', {}); break; } catch (e) { if (i > 100) throw new Error('the daemon did not start: ' + e.message); await sleep(50); }
    }
    this.hello = await this.rpc.request('hello', {});
    if (path.resolve(this.hello.data_dir) !== path.resolve(this.home)) throw new Error(`refusing to record: the daemon answers for ${this.hello.data_dir}, not ${this.home}`);
    this.initial = await this.rpc.request('state');
    this.stream = await new Connection(this.socketPath).open();
    this.stream.onEvent = e => this.onEvent(e);
    const replayed = new Promise(r => { this.stream.onReplayed = r; });
    await this.stream.request('events.subscribe', { after: 0 });
    await replayed;
    return this;
  }

  onEvent(e) {
    this.events.push(e);
    this.lastEventAt = Date.now();
    if (this.events.length % EVERY === 0) this.checkpoint();
    for (const w of [...this.waiters]) if (w.pred(e)) { this.waiters.splice(this.waiters.indexOf(w), 1); w.resolve(e); }
  }

  /** The daemon's state now. Its own cursor says which events it includes. */
  checkpoint(mark) {
    const p = this.rpc.request('state').then(state => { this.checkpoints.push({ cursor: state.cursor, ...(mark ? { mark } : {}), state }); return state; });
    this.pendingCheckpoints.push(p);
    return p;
  }

  setMode(mode) { fs.writeFileSync(this.modeFile, mode); }

  /** Waits for an event (also one that already arrived after `from`). */
  event(pred, { from = 0, timeout = 30000, what = 'an event' } = {}) {
    const seen = this.events.slice(from).find(pred);
    if (seen) return Promise.resolve(seen);
    return new Promise((resolve, reject) => {
      const w = { pred, resolve };
      this.waiters.push(w);
      setTimeout(() => { if (this.waiters.includes(w)) reject(new Error(`${this.name}: timed out waiting for ${what}`)); }, timeout);
    });
  }

  status(runId, statuses, from = 0) { return this.event(e => e.run_id === runId && e.kind === 'status' && statuses.includes(e.payload.status), { from, what: `${runId} to be ${statuses.join(' or ')}` }); }
  ended(runId, from = 0) { return this.status(runId, ['completed', 'failed', 'interrupted', 'disconnected', 'unknown'], from); }

  async claude(repo, mode, prompt, extra = {}) {
    this.setMode(mode);
    const from = this.events.length;
    const made = await this.rpc.request('task.create', { repo, harness: 'claude', workspace_mode: 'worktree', prompt, ...extra });
    if (made.launch_error) throw new Error(`${this.name}: ${made.launch_error}`);
    // The fixture reads its mode when it starts: the next agent may change the file after that.
    await this.status(made.run.id, ['running'], from);
    await sleep(150);
    return { ...made, from };
  }

  /** Until nothing has arrived for a while. */
  async quiet(ms = 500) { while (Date.now() - (this.lastEventAt || 0) < ms) await sleep(50); }

  async finish() {
    await this.quiet();
    this.final = await this.rpc.request('state');
    await Promise.all(this.pendingCheckpoints);
    // Every event the final state includes must be in the recording.
    for (let i = 0; i < 100 && (this.events.at(-1)?.seq || 0) < this.final.cursor; i++) await sleep(20);
    if ((this.events.at(-1)?.seq || 0) < this.final.cursor) throw new Error(`${this.name}: the stream ended at ${this.events.at(-1)?.seq}, the state at ${this.final.cursor}`);
    this.events = this.events.filter(e => e.seq <= this.final.cursor);
    this.checkpoints = this.checkpoints.filter(c => c.cursor <= this.final.cursor).sort((a, b) => a.cursor - b.cursor);
  }

  async stop() {
    this.stream?.close();
    try {
      for (let i = 0; i < 40; i++) {
        const active = await this.rpc.request('run.active');
        if (!active.length) break;
        for (const run of active) if (!run.parent_run_id) await this.rpc.request('run.interrupt', { run_id: run.id }).catch(() => {});
        await sleep(250);
      }
      await this.rpc.request('daemon.shutdown').catch(() => {});
    } catch { /* the daemon is already gone */ }
    this.rpc?.close();
    await sleep(100);
    try { this.child.kill('SIGKILL'); } catch { /* gone */ }
    try { execFileSync('pkill', ['-9', '-f', this.home], { stdio: 'ignore' }); } catch { /* nothing left */ }
    fs.rmSync(this.root, { recursive: true, force: true });
  }

  /** The recording with every local path replaced by a fixed one. */
  scrubbed() {
    const swaps = [[this.root, '/fixture'], [this.root.replace(/^\/private/, ''), '/fixture'], [repoRoot, '/overseer'], [os.homedir(), '/Users/fixture']]
      .filter(([from]) => from && from !== '/').sort((a, b) => b[0].length - a[0].length);
    const scrub = value => {
      let text = JSON.stringify(value);
      for (const [from, to] of swaps) text = text.split(JSON.stringify(from).slice(1, -1)).join(to);
      return JSON.parse(text);
    };
    return scrub({
      scenario: this.name,
      about: 'Recorded from a real overseerd with fixture harnesses by phone/model/scripts/record.mjs. Paths are replaced by fixed ones.',
      daemon: { version: this.final.daemon.version, parser_version: this.final.daemon.parser_version, protocol: this.hello.protocol },
      marks: this.marks,
      initial: this.initial,
      events: this.events,
      checkpoints: this.checkpoints,
      final: this.final,
    });
  }

  save() {
    const r = this.scrubbed();
    const lines = list => (list.length ? '[\n' + list.map(x => '    ' + JSON.stringify(x)).join(',\n') + '\n  ]' : '[]');
    const text = '{\n' + [
      `  "scenario": ${JSON.stringify(r.scenario)}`, `  "about": ${JSON.stringify(r.about)}`, `  "daemon": ${JSON.stringify(r.daemon)}`, `  "marks": ${JSON.stringify(r.marks)}`,
      `  "initial": ${JSON.stringify(r.initial)}`, `  "events": ${lines(r.events)}`, `  "checkpoints": ${lines(r.checkpoints)}`, `  "final": ${JSON.stringify(r.final)}`,
    ].join(',\n') + '\n}\n';
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, `${this.name}.json`), text);
    console.log(`${this.name}: ${r.events.length} events, ${r.checkpoints.length} checkpoints, ${r.final.runs.length} runs, ${(text.length / 1024).toFixed(0)} kB`);
  }
}

const permission = (s, runId, from) => s.event(e => e.run_id === runId && e.kind === 'permission', { from, what: 'a permission request' });

const scenarios = {
  async showcase(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'showcase', 'Make expired sessions refresh once', { title: 'Refresh sessions once', model: 'fixture-large' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async 'showcase-permission'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'showcase-permission', 'Make expired sessions refresh once, then write the changelog', { title: 'Refresh sessions and changelog' });
    s.marks.root = a.run.id;
    const asked = await permission(s, a.run.id, a.from);
    await s.status(a.run.id, ['waiting_for_user'], a.from);
    await s.quiet(300);
    await s.checkpoint('permission pending');
    s.marks.pending_through = s.events.at(-1).seq;
    await s.rpc.request('run.permission', { run_id: a.run.id, request_id: asked.payload.request_id, allow: true });
    await s.ended(a.run.id, a.from);
  },
  async nested(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'nested', 'Delegate the work to a sub-agent', { title: 'Delegate to a sub-agent' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async 'permission-allow'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'permission', 'Write perm.txt', { title: 'Write a file, allowed' });
    s.marks.root = a.run.id;
    const asked = await permission(s, a.run.id, a.from);
    await s.status(a.run.id, ['waiting_for_user'], a.from);
    await s.quiet(300);
    await s.checkpoint('permission pending');
    s.marks.pending_through = s.events.at(-1).seq;
    await s.rpc.request('run.permission', { run_id: a.run.id, request_id: asked.payload.request_id, allow: true });
    await s.ended(a.run.id, a.from);
  },
  async 'permission-deny'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'permission', 'Write perm.txt', { title: 'Write a file, denied' });
    s.marks.root = a.run.id;
    const asked = await permission(s, a.run.id, a.from);
    await s.status(a.run.id, ['waiting_for_user'], a.from);
    await s.quiet(300);
    await s.checkpoint('permission pending');
    s.marks.pending_through = s.events.at(-1).seq;
    await s.rpc.request('run.permission', { run_id: a.run.id, request_id: asked.payload.request_id, allow: false, message: 'Not in this branch' });
    await s.ended(a.run.id, a.from);
  },
  async 'echo-follow-up'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'echo', 'Say what you received', { title: 'Echo, then a follow-up' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
    await s.quiet(300);
    await s.checkpoint('first turn done');
    const from = s.events.length;
    await s.rpc.request('run.follow_up', { run_id: a.run.id, prompt: 'And once more, with **feeling**' });
    await s.ended(a.run.id, from);
  },
  async 'slow-interrupted'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'slow', 'Take your time with the migration', { title: 'A slow migration, stopped' });
    s.marks.root = a.run.id;
    await s.event(e => e.run_id === a.run.id && e.kind === 'output' && e.payload.role === 'assistant', { from: a.from, what: 'the first reply' });
    await s.quiet(300);
    await s.checkpoint('working');
    s.marks.working_through = s.events.at(-1).seq;
    await s.rpc.request('run.interrupt', { run_id: a.run.id });
    await s.ended(a.run.id, a.from);
  },
  async 'stop-live'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'stop-live', 'Write a long list', { title: 'A long list, stopped' });
    s.marks.root = a.run.id;
    await s.event(e => e.run_id === a.run.id && e.kind === 'output' && e.payload.role === 'assistant', { from: a.from, what: 'the first reply' });
    await s.quiet(300);
    await s.rpc.request('run.interrupt', { run_id: a.run.id });
    await s.ended(a.run.id, a.from);
  },
  async ratelimit(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'ratelimit', 'Summarize the repository', { title: 'Rate limited' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async auth(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'auth', 'Summarize the repository', { title: 'Signed out' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async quota(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'quota', 'Summarize the repository', { title: 'Out of quota' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async 'failed-reason'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'failed-reason', 'Run the migration', { title: 'A failed migration' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async unparsed(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const a = await s.claude(repo, 'unparsed', 'Say done', { title: 'A line Overseer cannot parse' });
    s.marks.root = a.run.id;
    await s.ended(a.run.id, a.from);
  },
  async 'generic-300'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const from = s.events.length;
    const made = await s.rpc.request('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Print 300 lines',
      args: ['-c', 'i=1; while [ $i -le 300 ]; do echo "line $i of 300"; i=$((i+1)); done; echo "a warning on stderr" 1>&2'] });
    if (made.launch_error) throw new Error(made.launch_error);
    s.marks.root = made.run.id;
    await s.ended(made.run.id, from);
  },
  async 'codex-subagent'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const from = s.events.length;
    const made = await s.rpc.request('task.create', { repo, harness: 'codex', workspace_mode: 'worktree', prompt: 'Ask one sub-agent to say hi, then create hello.txt', title: 'Codex with a sub-agent' });
    if (made.launch_error) throw new Error(made.launch_error);
    s.marks.root = made.run.id;
    await s.ended(made.run.id, from);
  },
  async 'codex-app-tree'(s) {
    const repo = makeRepo(path.join(s.root, 'shop'));
    const from = s.events.length;
    const made = await s.rpc.request('task.create', { repo, harness: 'codex-app', workspace_mode: 'worktree', prompt: 'Spawn a child that spawns a grandchild, then touch approved.txt', title: 'Codex app-server with a tree' });
    if (made.launch_error) throw new Error(made.launch_error);
    s.marks.root = made.run.id;
    const asked = await permission(s, made.run.id, from);
    await s.status(made.run.id, ['waiting_for_user'], from);
    await s.quiet(300);
    await s.checkpoint('permission pending');
    s.marks.pending_through = s.events.at(-1).seq;
    await s.rpc.request('run.permission', { run_id: made.run.id, request_id: String(asked.payload.request_id), allow: true });
    await s.ended(made.run.id, from);
  },
  /** Nine agents in two repositories: a nested child and grandchild, one archived, two that need the owner. */
  async 'nine-agents'(s) {
    const shop = makeRepo(path.join(s.root, 'shop'));
    const billing = makeRepo(path.join(s.root, 'billing-service'));
    const account = await s.rpc.request('profile.create', { name: 'Work account', harness: 'claude' });
    const done = [];
    const a = await s.claude(shop, 'showcase', 'Make expired sessions refresh once', { title: 'Refresh sessions once', model: 'fixture-large' });
    done.push(s.ended(a.run.id, a.from));
    const b = await s.claude(shop, 'nested', 'Delegate the work to a sub-agent', { title: 'Delegate to a sub-agent', profile_id: account.id });
    done.push(s.ended(b.run.id, b.from));
    const c = await s.claude(billing, 'auth', 'Summarize the invoices module', { title: 'Summarize invoices' });
    done.push(s.ended(c.run.id, c.from));
    const d = await s.claude(billing, 'ratelimit', 'Rename the tax helper', { title: 'Rename the tax helper', profile_id: account.id });
    done.push(s.ended(d.run.id, d.from));
    const e = await s.claude(billing, 'echo', 'Say what you received', { title: 'Echo the prompt', model: 'fixture-small' });
    done.push(s.ended(e.run.id, e.from));
    const from = s.events.length;
    const f = await s.rpc.request('task.create', { repo: shop, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'List the files', args: ['-c', 'ls; echo listed'] });
    done.push(s.ended(f.run.id, from));
    const g = await s.claude(shop, 'failed-reason', 'Run the migration', { title: 'Run the migration' });
    done.push(s.ended(g.run.id, g.from));
    await Promise.all(done);
    await s.rpc.request('task.archive', { task_id: e.task.id, archived: true });
    await s.checkpoint('seven finished, one archived');
    const h = await s.claude(billing, 'permission', 'Write perm.txt', { title: 'Write the permissions file' });
    await permission(s, h.run.id, h.from);
    await s.status(h.run.id, ['waiting_for_user'], h.from);
    const i = await s.claude(shop, 'slow', 'Take your time with the migration', { title: 'A slow migration', env: undefined });
    await s.event(ev => ev.run_id === i.run.id && ev.kind === 'output', { from: i.from, what: 'the first reply' });
    Object.assign(s.marks, { showcase: a.run.id, nested: b.run.id, auth: c.run.id, ratelimit: d.run.id, archived: e.run.id, generic: f.run.id, failed: g.run.id, waiting: h.run.id, running: i.run.id, account: account.id });
  },
};

const ENV = {
  'codex-subagent': { OVERSEER_CODEX_PATH: fx('replay.js'), REPLAY_FILE: path.join(repoRoot, 'fixtures/transcripts/codex-0.155-exec-subagent-live.jsonl'), REPLAY_DELAY_MS: '10' },
  'codex-app-tree': { OVERSEER_CODEX_PATH: fx('codex-app-fixture.js'), FIXTURE_MODE: 'tree' },
  'nine-agents': { FIXTURE_SLOW_MS: '60000' },
};

const wanted = process.argv.slice(2);
const names = wanted.length ? wanted : Object.keys(scenarios);
let failed = 0;
for (const name of names) {
  if (!scenarios[name]) { console.error(`unknown scenario ${name}; known: ${Object.keys(scenarios).join(', ')}`); process.exit(2); }
  const s = new Session(name, ENV[name]);
  try {
    await s.start();
    await scenarios[name](s);
    await s.finish();
    s.save();
  } catch (error) {
    failed++;
    console.error(`${name}: FAILED: ${error.stack || error.message}`);
  } finally {
    await s.stop();
  }
}
process.exit(failed ? 1 : 0);
