#!/usr/bin/env node
// A real overseerd for the phone's scenarios: its own data folder, fixture agents instead of
// paid ones, phone access on, and pairing that confirms itself the way the owner would on the
// Mac. It never touches the owner's running daemon, checkouts or logins.
//
//   node e2e/lab.mjs start [--port 47811] [--state <file>]    runs until it is stopped
//   node e2e/lab.mjs call <state file> <method> [json]        a request as the Mac makes it
//   node e2e/lab.mjs code <state file>                        a new pairing code
//   node e2e/lab.mjs agent <state file> <mode> <title>        starts a fixture agent
//   node e2e/lab.mjs stop <state file>
//
// The state file (JSON) says where the lab is: home, socket, port, repo, the pairing code.

import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(here, '..', '..');
const BIN = process.env.OVERSEER_BIN || path.join(ROOT, 'target', 'debug', 'overseerd');
const FIXTURE = path.join(ROOT, 'fixtures', 'fake-harness', 'claude-fixture.js');
const DEFAULT_STATE = path.join(os.tmpdir(), 'overseer-phone-lab.json');

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function call(socketPath, method, params = {}) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath);
    let buffer = '';
    socket.setEncoding('utf8');
    socket.on('connect', () => socket.write(`${JSON.stringify({ id: 1, method, params })}\n`));
    socket.on('data', (chunk) => {
      buffer += chunk;
      const at = buffer.indexOf('\n');
      if (at < 0) return;
      socket.destroy();
      const reply = JSON.parse(buffer.slice(0, at));
      if (reply.error) reject(new Error(`${method}: ${JSON.stringify(reply.error)}`));
      else resolve(reply.result);
    });
    socket.on('error', (error) => reject(new Error(`${method}: ${error.message}`)));
  });
}

async function until(what, ok, ms = 20_000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const value = await ok();
      if (value) return value;
    } catch {
      /* not yet */
    }
    await sleep(60);
  }
  throw new Error(`${what} did not happen in ${ms} ms`);
}

function makeRepo(home) {
  const dir = path.join(home, 'shop');
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  const git = (...args) => execFileSync('git', args, { cwd: dir, stdio: 'ignore' });
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'Lab');
  git('config', 'user.email', 'lab@example.invalid');
  git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'README.md'), '# shop\n\nA repository for the phone scenarios.\n');
  fs.writeFileSync(path.join(dir, 'a.txt'), 'a\n');
  fs.writeFileSync(path.join(dir, 'src', 'cart.js'), 'export function total(items) {\n  return items.reduce((sum, item) => sum + item.price, 0);\n}\n');
  git('add', '.');
  git('commit', '-q', '-m', 'base');
  return dir;
}

function readState(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function envFor(home) {
  return {
    ...process.env,
    OVERSEER_HOME: home,
    OVERSEER_GATEWAY_MDNS: 'off',
    OVERSEER_CLAUDE_PATH: FIXTURE,
    OVERSEER_CODEX_PATH: '/nonexistent/harness-disabled-in-tests',
    OVERSEER_OPENCODE_PATH: '/nonexistent/harness-disabled-in-tests',
    OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS',
    CLAUDE_FIXTURE_MODE_FILE: path.join(home, 'fixture-mode'),
    FIXTURE_SLOW_MS: process.env.FIXTURE_SLOW_MS || '400',
  };
}

async function agent(state, mode, title, prompt = 'go') {
  fs.writeFileSync(path.join(state.home, 'fixture-mode'), mode);
  const created = await call(state.socket, 'task.create', { repo: state.repo, harness: 'claude', prompt, title });
  // The fixture reads its mode when it starts; the next agent may need another.
  await until(`agent "${title}" starting`, async () => {
    const found = (await call(state.socket, 'state')).runs.find((r) => r.id === created.run.id);
    return found && found.status !== 'queued' && found.status !== 'starting';
  });
  return created.run.id;
}

async function start(args) {
  const port = Number(args.port || 47811);
  const stateFile = args.state || DEFAULT_STATE;
  // A short path: a Unix socket's path is limited to about 100 characters.
  const home = fs.mkdtempSync(path.join('/tmp', 'ovs-e2e'));
  const env = { ...envFor(home), ...(args.push ? { OVERSEER_TEST_PUSH_DIR: path.join(home, 'push') } : {}) };
  fs.writeFileSync(path.join(home, 'fixture-mode'), 'showcase');
  if (!fs.existsSync(BIN)) throw new Error(`${BIN} is missing. Build it: cargo build -p overseerd`);
  const log = fs.openSync(path.join(home, 'lab.log'), 'a');
  const child = spawn(BIN, ['serve'], { env, stdio: ['ignore', log, log] });
  const socket = execFileSync(BIN, ['socket-path'], { env, encoding: 'utf8' }).trim();
  await until('the daemon', () => call(socket, 'hello'));
  const status = await call(socket, 'gateway.enable', { port });
  if (status.enabled !== true) throw new Error('phone access did not turn on');
  const repo = makeRepo(home);
  const state = { home, socket, port, repo, pid: child.pid, code: null, runs: {} };

  if (args.seed !== 'none') {
    state.runs.showcase = await agent(state, 'showcase', 'Tidy the cart totals', 'tidy the cart');
    state.runs.nested = await agent(state, 'nested', 'Split the checkout in two', 'split the checkout');
    state.runs.permission = await agent(state, 'showcase-permission', 'Add a discount rule', 'add a discount');
  }
  state.code = (await call(socket, 'gateway.pair_start')).code;
  fs.writeFileSync(stateFile, JSON.stringify(state, null, 2));
  console.log(JSON.stringify({ state: stateFile, ...state }));

  // The owner's part of pairing, done for them: whoever presents the code is confirmed.
  let stopping = false;
  const stop = () => {
    if (stopping) return;
    stopping = true;
    child.kill('SIGTERM');
    try {
      execFileSync('pkill', ['-9', '-f', home], { stdio: 'ignore' });
    } catch {
      /* nothing left */
    }
    if (!args.keep) fs.rmSync(home, { recursive: true, force: true });
    process.exit(0);
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  child.on('exit', () => {
    if (!stopping && !args.survive) process.exit(1);
  });
  for (;;) {
    await sleep(150);
    try {
      const now = await call(socket, 'gateway.status');
      for (const waiting of now.pairing?.waiting ?? []) {
        await call(socket, 'gateway.pair_confirm', { request: waiting.request, accept: true });
        console.log(JSON.stringify({ paired: waiting.name ?? waiting.request }));
      }
    } catch {
      /* the daemon is restarting or gone */
    }
  }
}

function parse(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg.startsWith('--')) {
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('--')) out[arg.slice(2)] = true;
      else {
        out[arg.slice(2)] = next;
        i += 1;
      }
    } else out._.push(arg);
  }
  return out;
}

const [command, ...rest] = process.argv.slice(2);
const args = parse(rest);
try {
  if (command === 'start') await start(args);
  else if (command === 'call') {
    const state = readState(args._[0]);
    console.log(JSON.stringify(await call(state.socket, args._[1], args._[2] ? JSON.parse(args._[2]) : {})));
  } else if (command === 'code') {
    const file = args._[0];
    const state = readState(file);
    state.code = (await call(state.socket, 'gateway.pair_start')).code;
    fs.writeFileSync(file, JSON.stringify(state, null, 2));
    console.log(state.code);
  } else if (command === 'agent') {
    const state = readState(args._[0]);
    console.log(await agent(state, args._[1], args._[2] ?? args._[1], args._[3]));
  } else if (command === 'stop') {
    const state = readState(args._[0]);
    try {
      process.kill(state.pid, 'SIGTERM');
    } catch {
      /* gone already */
    }
    try {
      execFileSync('pkill', ['-9', '-f', state.home], { stdio: 'ignore' });
    } catch {
      /* nothing left */
    }
    fs.rmSync(state.home, { recursive: true, force: true });
  } else {
    console.error('usage: lab.mjs start|call|code|agent|stop (see the top of the file)');
    process.exit(2);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
