#!/usr/bin/env node
// One tiny live turn each on Claude Code and Codex, started from the phone and answered on it
// (AC-125), within the owner's paid-turn rules: Claude on Haiku; Codex on gpt-5.6-luna at low
// effort; one attempt each and no retry. The Mac's own CLIs with the owner's own logins answer;
// the daemon has its own data folder. Nothing else is started: the lab seeds no agents.
//
//   node e2e/live.mjs [--platform ios|android] [--only claude,codex]
//
// Writes docs/verification/evidence/phone/e2e/live/<platform>.json and its screenshots.

import { execFileSync, spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { BUNDLE, device, maestro, sleep } from './device.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const phone = path.join(here, '..');
const args = process.argv.slice(2);
const platform = args.includes('--platform') ? args[args.indexOf('--platform') + 1] : 'ios';
const out = path.join(phone, '..', 'docs', 'verification', 'evidence', 'phone', 'e2e', 'live');
const PORT = platform === 'ios' ? 47841 : 47842;
const APPS = {
  ios: path.join(phone, 'ios', 'build', 'Build', 'Products', 'Release-iphonesimulator', 'Overseer.app'),
  android: path.join(phone, 'android', 'app', 'build', 'outputs', 'apk', 'release', 'app-release.apk'),
};
const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : null;
const TURNS = [
  { harness: 'claude', model: 'haiku', effort: '', profile: 'system-claude' },
  { harness: 'codex', model: 'gpt-5.6-luna', effort: 'low', profile: 'system-codex' },
].filter((turn) => !only || only.includes(turn.harness));
const TASK = 'Reply with the single word ok and nothing else.';
const FINAL = ['completed', 'failed', 'interrupted', 'cancelled', 'error'];

const lines = [];
const say = (line) => {
  const stamped = `${new Date().toISOString().slice(11, 19)} ${line}`;
  lines.push(stamped);
  console.log(stamped);
};

const state = path.join(out, `lab-${platform}.json`);
const lab = (...rest) => execFileSync('node', [path.join(here, 'lab.mjs'), ...rest], { encoding: 'utf8' }).trim();
const call = (method, params = {}) => JSON.parse(lab('call', state, method, JSON.stringify(params)));

async function flow(dev, name, env) {
  const child = maestro(['--udid', dev.id, 'test', '--debug-output', path.join(out, 'maestro', platform, name), '--flatten-debug-output', ...Object.entries({ APP: BUNDLE, ...env }).flatMap(([k, v]) => ['-e', `${k}=${v}`]), path.join(here, 'flows', `${name}.yaml`)], { cwd: phone });
  let text = '';
  child.stdout.on('data', (d) => (text += d));
  child.stderr.on('data', (d) => (text += d));
  const status = await new Promise((resolve) => child.on('exit', resolve));
  for (const file of fs.readdirSync(path.join(out, 'maestro', platform, name), { recursive: true })) {
    if (String(file).endsWith('.png') && !path.basename(String(file)).startsWith('step-') && !path.basename(String(file)).startsWith('screenshot-')) fs.copyFileSync(path.join(out, 'maestro', platform, name, String(file)), path.join(out, `${platform}-${path.basename(String(file))}`));
  }
  if (status !== 0) throw new Error(`the flow ${name} failed: ${text.split('\n').filter((l) => /FAILED|Assertion/.test(l)).slice(0, 2).join(' ')}`);
}

async function main() {
  fs.mkdirSync(out, { recursive: true });
  const dev = device(platform);
  const result = { platform, at: new Date().toISOString(), task: TASK, turns: [] };
  const child = spawn('node', [path.join(here, 'lab.mjs'), 'start', '--port', String(PORT), '--state', state, '--live', '--seed', 'none'], { stdio: 'ignore', detached: true });
  child.unref();
  try {
    for (let i = 0; i < 90 && !(fs.existsSync(state) && (() => { try { call('gateway.devices'); return true; } catch { return false; } })()); i += 1) await sleep(1000);
    say(`lab up on ${PORT}, the Mac's own Claude Code and Codex`);
    // The phone offers the repositories agents have worked in: a free generic task makes the lab's known.
    const repo = JSON.parse(fs.readFileSync(state, 'utf8')).repo;
    call('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Know the repository', args: ['-c', 'true'] });
    for (const turn of TURNS) {
      const status = call('profile.status', { id: turn.profile });
      turn.signedIn = Boolean(status.logged_in);
      say(`${turn.profile}: ${turn.signedIn ? 'signed in' : 'not signed in'} (${status.method ?? 'no method'}, ${status.version ?? 'no version'})`);
    }
    // The phone, new and paired to this lab.
    dev.stop();
    dev.uninstall();
    dev.install(APPS[platform]);
    const paired = spawnSync('node', [path.join(here, 'run.mjs'), '--platform', platform, '--dev', state, '--only', 'pair', '--skip-measure', '--out', path.join(out, 'pairing')], { cwd: phone, encoding: 'utf8', env: { ...process.env, LANG: 'en_US.UTF-8', LC_ALL: 'en_US.UTF-8' } });
    if (paired.status !== 0) throw new Error('the phone did not pair');
    say('the phone paired');
    for (const turn of TURNS) {
      const record = { harness: turn.harness, model: turn.model, effort: turn.effort || 'default', signedIn: turn.signedIn };
      result.turns.push(record);
      if (!turn.signedIn) {
        record.outcome = 'not run: the Mac is not signed in to this harness';
        say(`${turn.harness}: ${record.outcome}`);
        continue;
      }
      const before = new Set(call('state').runs.map((r) => r.id));
      try {
        // One attempt: the form is filled and started once.
        await flow(dev, 'new-live', { REPO: 'shop', HARNESS: turn.harness, MODEL: turn.model, EFFORT: turn.effort, TASK });
        let made = null;
        for (let i = 0; i < 240 && !made; i += 1) {
          made = call('state').runs.find((r) => !before.has(r.id) && !r.parent_run_id && r.harness === turn.harness && FINAL.includes(r.status)) ?? null;
          if (!made) await sleep(1000);
        }
        if (!made) throw new Error('the agent did not finish within four minutes');
        const events = call('events.list', { after: 0, limit: 5000 }).events.filter((e) => e.run_id === made.id);
        const answer = events.filter((e) => e.kind === 'output' && e.payload?.role === 'assistant').map((e) => e.payload.text).join('\n').trim();
        const all = call('events.list', { after: 0, limit: 5000 }).events;
        const created = all.find((e) => e.kind === 'task_created' && e.run_id === made.id);
        const asked = all.filter((e) => e.kind === 'remote_command' && e.payload?.method === 'task.create' && created && e.seq < created.seq).at(-1);
        const usage = events.filter((e) => e.kind === 'usage').map((e) => e.payload).at(-1) ?? null;
        const errors = events.filter((e) => e.kind === 'error').map((e) => JSON.stringify(e.payload).slice(0, 300));
        Object.assign(record, { run: made.id, status: made.status, model_used: made.model ?? null, answer: answer.slice(0, 200), from_phone: asked?.source ?? null, usage, errors });
        record.outcome = made.status === 'completed' && /\bok\b/i.test(answer) && String(asked?.source).startsWith('phone:') ? 'answered' : 'not answered as asked';
        say(`${turn.harness} on ${turn.model}${turn.effort ? `, effort ${turn.effort}` : ''}: ${made.status}, answer ${JSON.stringify(answer.slice(0, 60))}, started by ${asked?.source}`);
        if (record.outcome === 'answered') await flow(dev, 'live-answer', { HARNESS: turn.harness });
      } catch (error) {
        record.outcome = `failed: ${error instanceof Error ? error.message : String(error)}`;
        say(`${turn.harness}: ${record.outcome}`);
      }
    }
  } finally {
    try {
      lab('stop', state);
    } catch {
      /* stopped already */
    }
    dev.stop();
    fs.rmSync(state, { force: true });
    const file = path.join(out, `${platform}.json`);
    const earlier = only && fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : null;
    if (earlier) result.earlier = earlier;
    fs.writeFileSync(file, `${JSON.stringify(result, null, 2)}\n`);
    fs.appendFileSync(path.join(out, `${platform}.log`), `${lines.join('\n')}\n`);
    // Maestro's own debug files are large; the screenshots are kept beside the record.
    fs.rmSync(path.join(out, 'maestro'), { recursive: true, force: true });
    fs.rmSync(path.join(out, 'pairing'), { recursive: true, force: true });
  }
  const ok = result.turns.every((t) => t.outcome === 'answered');
  say(ok ? 'both live turns were started from the phone and answered' : 'a live turn was not answered: see the record');
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(2);
});
