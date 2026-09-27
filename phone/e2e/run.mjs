#!/usr/bin/env node
// The phone's regression run, from one command (AC-132): the app on the iOS simulator and the
// Android emulator, against a real overseerd with fixture agents, driven by Maestro; then the
// speed budgets (AC-135). It fails when a scenario fails or a budget is missed.
//
//   node e2e/run.mjs                       both platforms, builds first
//   node e2e/run.mjs --platform ios        one platform
//   node e2e/run.mjs --skip-build          the release apps that are already built
//   node e2e/run.mjs --runs 5              fewer cold starts (20 unless given)
//   node e2e/run.mjs --seed-slow 400       holds every start for 400 ms: the run must fail
//   node e2e/run.mjs --only pair,send      some scenarios only
//   node e2e/run.mjs --out <dir>           where logs, screenshots and results go
//
// It uses Overseer's own simulator and virtual device, an overseerd with its own data folder,
// and fixture agents. It never touches the owner's daemon, checkouts, logins or other devices.

import { execFileSync, spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { BUNDLE, device, maestro, sleep } from './device.mjs';
import { scenarios } from './scenarios.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const phone = path.join(here, '..');
const root = path.join(phone, '..');
const PORTS = { ios: 47821, android: 47822 };
const APPS = {
  ios: path.join(phone, 'ios', 'build', 'Build', 'Products', 'Release-iphonesimulator', 'Overseer.app'),
  android: path.join(phone, 'android', 'app', 'build', 'outputs', 'apk', 'release', 'app-release.apk'),
};

function parse(argv) {
  const args = { platforms: ['ios', 'android'], build: true, runs: 20, slow: 0, only: null, out: path.join(root, 'docs', 'verification', 'evidence', 'phone', 'e2e'), measure: true, baseline: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => argv[(i += 1)];
    if (arg === '--platform') args.platforms = next().split(',');
    else if (arg === '--skip-build') args.build = false;
    else if (arg === '--skip-measure') args.measure = false;
    else if (arg === '--write-baseline') args.baseline = true;
    else if (arg === '--runs') args.runs = Number(next());
    else if (arg === '--seed-slow') args.slow = Number(next());
    else if (arg === '--only') args.only = next().split(',');
    else if (arg === '--out') args.out = path.resolve(next());
    else throw new Error(`unknown argument: ${arg}`);
  }
  return args;
}

class Log {
  constructor(file) {
    this.file = file;
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, '');
  }
  say(line) {
    const text = `${new Date().toISOString().slice(11, 19)} ${line}`;
    console.log(text);
    fs.appendFileSync(this.file, `${text}\n`);
  }
  raw(text) {
    fs.appendFileSync(this.file, text.endsWith('\n') ? text : `${text}\n`);
  }
}

function run(log, command, args, options = {}) {
  log.say(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, { cwd: phone, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, ...options });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  log.raw(output.split('\n').slice(-40).join('\n'));
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
  return output;
}

/** The lab: a real overseerd of its own, with fixture agents. */
async function startLab(log, platform, out) {
  const state = path.join(out, `lab-${platform}.json`);
  fs.rmSync(state, { force: true });
  // The daemon delivers notifications itself: to the iOS simulator through the simulator's own
  // tool, and nothing to Android, which shows its own while the app is open.
  const child = spawn('node', [path.join(here, 'lab.mjs'), 'start', '--port', String(PORTS[platform]), '--state', state], { cwd: phone, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', (chunk) => log.raw(`lab: ${String(chunk).slice(0, 400)}`));
  child.stderr.on('data', (chunk) => log.raw(`lab: ${chunk}`));
  const end = Date.now() + 60_000;
  while (!fs.existsSync(state)) {
    if (child.exitCode !== null) throw new Error('the lab daemon did not start');
    if (Date.now() > end) throw new Error('the lab daemon did not start within a minute');
    await sleep(200);
  }
  const lab = (...args) => execFileSync('node', [path.join(here, 'lab.mjs'), ...args], { cwd: phone, encoding: 'utf8' }).trim();
  return {
    state,
    info: () => JSON.parse(fs.readFileSync(state, 'utf8')),
    call: (method, params = {}) => JSON.parse(lab('call', state, method, JSON.stringify(params))),
    code: () => lab('code', state),
    agent: (mode, title, prompt = 'go') => lab('agent', state, mode, title, prompt),
    mode: (mode) => lab('mode', state, mode),
    stop: async () => {
      child.kill('SIGTERM');
      await new Promise((resolve) => (child.exitCode !== null ? resolve() : child.on('exit', resolve)));
    },
  };
}

async function flow(log, dev, name, env, out) {
  const file = path.join(here, 'flows', `${name}.yaml`);
  const args = ['--udid', dev.id, 'test', '--debug-output', path.join(out, 'maestro', dev.platform, name), '--flatten-debug-output'];
  for (const [key, value] of Object.entries({ APP: BUNDLE, ...env })) args.push('-e', `${key}=${value}`);
  args.push(file);
  log.say(`flow ${name}`);
  const child = maestro(args, { cwd: phone });
  let output = '';
  child.stdout.on('data', (chunk) => (output += chunk));
  child.stderr.on('data', (chunk) => (output += chunk));
  const status = await new Promise((resolve) => child.on('exit', resolve));
  log.raw(output.replace(/\u001b\[[0-9;]*[A-Za-z]/g, '').split('\n').filter((line) => line.trim()).slice(-60).join('\n'));
  if (status !== 0) throw new Error(`the flow ${name} failed`);
}

async function until(what, ok, ms = 20_000) {
  const end = Date.now() + ms;
  for (;;) {
    try {
      const value = await ok();
      if (value) return value;
    } catch {
      /* not yet */
    }
    if (Date.now() > end) throw new Error(`${what} did not happen in ${ms} ms`);
    await sleep(200);
  }
}

async function platformRun(args, platform, summary) {
  const out = path.join(args.out, platform);
  fs.rmSync(out, { recursive: true, force: true });
  const log = new Log(path.join(args.out, `${platform}.log`));
  log.say(`The phone's scenarios on ${platform}. Commit ${execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()}.`);
  const dev = device(platform);
  if (!fs.existsSync(APPS[platform])) throw new Error(`${APPS[platform]} is missing: build the release app first (without --skip-build)`);
  // A phone that has never been paired: the app is installed anew, with nothing stored.
  dev.stop();
  dev.uninstall();
  dev.install(APPS[platform]);
  dev.appearance('dark');
  const lab = await startLab(log, platform, args.out);
  const results = [];
  const context = { log, dev, lab, out, platform, until, sleep, flow: (name, env = {}) => flow(log, dev, name, env, args.out), shot: (name) => dev.screenshot(path.join(out, `${name}.png`)) };
  fs.mkdirSync(out, { recursive: true });
  try {
    for (const scenario of scenarios) {
      if (args.only && !args.only.includes(scenario.name) && !scenario.always) continue;
      if (scenario.platforms && !scenario.platforms.includes(platform)) {
        log.say(`skip ${scenario.name}: ${scenario.skipped}`);
        results.push({ name: scenario.name, criteria: scenario.criteria, ok: null, note: scenario.skipped });
        continue;
      }
      const started = Date.now();
      try {
        log.say(`scenario ${scenario.name} (${scenario.criteria.join(', ')}): ${scenario.says}`);
        await scenario.run(context);
        results.push({ name: scenario.name, criteria: scenario.criteria, ok: true, seconds: Math.round((Date.now() - started) / 100) / 10 });
        log.say(`ok   ${scenario.name}`);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        try {
          context.shot(`failed-${scenario.name}`);
        } catch {
          /* the screenshot is a help, not a need */
        }
        results.push({ name: scenario.name, criteria: scenario.criteria, ok: false, note: message });
        log.say(`FAIL ${scenario.name}: ${message}`);
        if (scenario.needed) break;
      }
    }
    if (args.measure && results.every((r) => r.ok !== false)) {
      if (args.slow > 0) {
        dev.stop();
        await sleep(300);
        dev.write('test.slow', args.slow);
        log.say(`seeded: every start is held for ${args.slow} ms; the budgets must notice`);
      }
      const measure = spawnSync('node', [path.join(here, 'measure.mjs'), '--platform', platform, '--runs', String(args.runs), '--out', args.out, ...(args.baseline ? ['--write-baseline'] : ['--check'])], { cwd: phone, encoding: 'utf8' });
      log.raw(`${measure.stdout}${measure.stderr}`);
      results.push({ name: 'budgets', criteria: ['AC-135', 'AC-136'], ok: measure.status === 0, note: measure.status === 0 ? '' : 'a budget was missed (see the measurements above)' });
      log.say(`${measure.status === 0 ? 'ok  ' : 'FAIL'} budgets`);
      if (args.slow > 0) {
        dev.stop();
        await sleep(300);
        dev.write('test.slow', 0);
      }
    }
  } finally {
    dev.stop();
    await lab.stop();
  }
  summary[platform] = results;
  return results.every((r) => r.ok !== false);
}

async function main() {
  const args = parse(process.argv.slice(2));
  fs.mkdirSync(args.out, { recursive: true });
  const build = new Log(path.join(args.out, 'build.log'));
  run(build, 'cargo', ['build', '-p', 'overseerd'], { cwd: root });
  if (args.build) {
    for (const platform of args.platforms) run(build, 'node', [path.join(phone, 'scripts', `run-${platform}.mjs`), '--release'], { env: { ...process.env, LANG: 'en_US.UTF-8', LC_ALL: 'en_US.UTF-8' } });
  }
  const summary = {};
  let ok = true;
  for (const platform of args.platforms) {
    try {
      ok = (await platformRun(args, platform, summary)) && ok;
    } catch (error) {
      ok = false;
      summary[platform] = [{ name: 'setup', criteria: [], ok: false, note: error instanceof Error ? error.message : String(error) }];
      console.error(`${platform}: ${summary[platform][0].note}`);
    }
  }
  fs.writeFileSync(path.join(args.out, 'result.json'), `${JSON.stringify({ at: new Date().toISOString(), seededSlow: args.slow, ok, platforms: summary }, null, 2)}\n`);
  console.log('');
  for (const [platform, results] of Object.entries(summary)) {
    for (const r of results) console.log(`${platform.padEnd(8)} ${r.ok === null ? 'skip' : r.ok ? 'ok  ' : 'FAIL'} ${r.name}${r.note ? `: ${r.note}` : ''}`);
  }
  console.log(ok ? '\nEvery scenario passed and every budget held.' : '\nThe run failed.');
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(2);
});
