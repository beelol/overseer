#!/usr/bin/env node
// AC-215: guided owner tests. Every owner-check file (docs/owner-checks/*.json) is valid and names
// existing criteria; Voice Mode's matches its RFC's steps; a fixture check run step by step through
// scripts/dev test builds its dev daemon with a scratch repository and a fixture agent, records each
// answer, writes one evidence record and leaves nothing behind. A temporary HOME and dev root; no
// VS Code here (the dev VS Code is scenario-dev-instance.js's). Run: node test/dev/guided.js
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');

const repo = path.resolve(__dirname, '../..');
require('../../scripts/git-fallback').ensureGit('guided tests'); // AC-159
const DEV = path.join(repo, 'scripts/dev');
const { checkProblems } = require(DEV);
const tmp = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-gdt-'));
const home = path.join(tmp, 'h'), root = path.join(tmp, 'r'), evidence = path.join(tmp, 'evidence');
fs.mkdirSync(home);
const results = [];
async function check(name, fn) {
  try { await fn(); results.push(true); console.log(`ok   ${name}`); } catch (e) { results.push(false); console.log(`FAIL ${name}\n     ${(e.stack || e.message).split('\n').slice(0, 6).join('\n     ')}`); }
}
const realHome = os.homedir();
const env = { ...process.env, HOME: home, OVERSEER_DEV_ROOT: root, CARGO_HOME: process.env.CARGO_HOME || path.join(realHome, '.cargo'), RUSTUP_HOME: process.env.RUSTUP_HOME || path.join(realHome, '.rustup') };
for (const k of Object.keys(env)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_DEV_ROOT') delete env[k];
const FIXTURE = path.join(__dirname, 'owner-check-fixture.json');
function dev(args, { ok = true } = {}) {
  const r = cp.spawnSync(process.execPath, [DEV, ...args], { env, encoding: 'utf8', input: '', maxBuffer: 64 * 1024 * 1024 });
  if (ok && r.status !== 0) throw new Error(`scripts/dev ${args.join(' ')} exited ${r.status}:\n${r.stdout}\n${r.stderr}`);
  return r;
}
const fx = (...a) => dev(['test', 'fixture', '--file', FIXTURE, ...a]);

(async () => {
  const rfc = fs.readFileSync(path.join(repo, 'docs/overseer-rfc.md'), 'utf8');
  const known = new Set([...rfc.matchAll(/\*\*(AC-\d{2,3}) — /g)].map(m => m[1]));
  const dir = path.join(repo, 'docs/owner-checks');
  const files = fs.readdirSync(dir).filter(f => f.endsWith('.json'));

  await check('every owner-check file is valid and names existing criteria', () => {
    assert.ok(files.length >= 1, 'at least one owner check');
    for (const f of files) {
      const c = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
      assert.deepStrictEqual(checkProblems(c), [], f);
      assert.strictEqual(`${c.name}.json`, f, 'file named after the check');
      for (const ac of c.criteria) assert.ok(known.has(ac), `${f}: ${ac} is in the RFC`);
      for (const s of c.steps) for (const ac of s.criteria) assert.ok(c.criteria.includes(ac), `${f} step ${s.id}: ${ac} is one of the check's criteria`);
      assert.ok(fs.existsSync(path.join(repo, c.source.split('#')[0])), `${f}: its source ${c.source} exists`);
    }
    assert.deepStrictEqual(checkProblems({ name: 'Bad Name', steps: [] }).length > 3, true, 'a bad file is caught');
  });

  await check("voice-mode.json has the Voice Mode RFC's eight steps, with the same criteria", () => {
    const c = JSON.parse(fs.readFileSync(path.join(dir, 'voice-mode.json'), 'utf8'));
    const text = fs.readFileSync(path.join(repo, 'docs/rfcs/voice-mode.md'), 'utf8');
    const sec = text.slice(text.indexOf("## The owner's checks"), text.indexOf('\n## ', text.indexOf("## The owner's checks") + 5));
    const steps = [...sec.matchAll(/^(\d+)\. \*\*([^*]+?)\*\*/gm)].map(m => ({ id: m[1], criteria: [...m[2].matchAll(/AC-\d+/g)].map(x => x[0]) }));
    assert.strictEqual(steps.length, 8);
    assert.deepStrictEqual(c.steps.map(s => [s.id, s.criteria]), steps.map(s => [s.id, s.criteria]));
  });

  await check('the fixture check: --start builds its dev daemon, a scratch repository and a fixture agent, and prints step 1', () => {
    const r = fx('--start', '--no-build');
    assert.ok(/Step 1 of 3: Look \(AC-215\)/.test(r.stdout), r.stdout);
    assert.ok(/Scratch repository: .*check-fixture\/scratch/.test(r.stdout), r.stdout);
    const st = JSON.parse(dev(['ctl', '--name', 'check-fixture', 'state']).stdout);
    assert.deepStrictEqual(st.tasks.map(t => t.title), ['Fixture agent']);
    assert.ok(JSON.stringify(st).includes('check-fixture/scratch'), 'the agent works in the scratch repository');
    assert.strictEqual(dev(['test', 'fixture', '--file', FIXTURE, '--start', '--no-build'], { ok: false }).status, 1, 'a second start is refused');
  });

  await check('--record, --skip and --status walk the steps one at a time', () => {
    assert.ok(/Recorded step 1 \(Look\)[\s\S]*Step 2 of 3: Skip me/.test(fx('--record', 'the agent is there').stdout));
    assert.ok(/Skipped step 2[\s\S]*Step 3 of 3: Last/.test(fx('--skip', 'not today').stdout));
    assert.ok(/Step 3 of 3: Last/.test(fx('--status').stdout));
    assert.ok(/All 3 steps .* are recorded/.test(fx('--record', 'hello').stdout));
    const again = dev(['test', 'fixture', '--file', FIXTURE, '--record', 'x'], { ok: false });
    assert.strictEqual(again.status, 1); assert.ok(/every step is recorded/.test(again.stderr));
  });

  await check('--finish writes one evidence record with every answer and criterion, and cleans up', () => {
    const r = fx('--finish', '--evidence', evidence);
    assert.ok(/2 of 3 steps recorded, 1 skipped/.test(r.stdout), r.stdout);
    const rec = JSON.parse(fs.readFileSync(path.join(evidence, 'record.json'), 'utf8'));
    assert.deepStrictEqual(rec.steps.map(s => [s.id, s.answer?.status, s.answer?.text]), [['1', 'recorded', 'the agent is there'], ['2', 'skipped', 'not today'], ['3', 'recorded', 'hello']]);
    assert.strictEqual(rec.instance, 'dev-check-fixture'); assert.ok(rec.commit);
    const md = fs.readFileSync(path.join(evidence, 'record.md'), 'utf8');
    for (const s of ['# Owner check: A fixture owner check', '## 1. Look (AC-215)', '- Answer (', 'the agent is there', '- Skipped (', 'not today']) assert.ok(md.includes(s), s);
    assert.ok(!fs.existsSync(path.join(root, 'check-fixture')), 'the dev daemon folder is gone');
    assert.strictEqual(cp.spawnSync('pgrep', ['-f', root], { encoding: 'utf8' }).stdout.trim(), '', 'nothing left running');
  });

  await check('without a terminal and without a step flag it explains how an agent runs it', () => {
    const r = dev(['test', 'voice-mode'], { ok: false });
    assert.strictEqual(r.status, 2); assert.ok(/from an agent use --start, --record, --skip, --status and --finish/.test(r.stderr), r.stderr);
    assert.ok(/voice-mode .*Gate R .*8 steps/.test(dev(['test', '--list']).stdout));
  });

  await check('AGENTS.md tells agents to use it whenever the owner asks for an owner check', () => {
    const text = fs.readFileSync(path.join(repo, 'AGENTS.md'), 'utf8');
    assert.ok(/owner asks for an owner check[^\n]*scripts\/dev test <check>/.test(text));
  });

  dev(['clean', '--all'], { ok: false });
  fs.rmSync(tmp, { recursive: true, force: true });
  const passed = results.filter(Boolean).length;
  console.log(`${passed} of ${results.length} passed`);
  process.exit(passed === results.length ? 0 : 1);
})();
