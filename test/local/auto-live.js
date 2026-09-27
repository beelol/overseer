#!/usr/bin/env node
// Tiny account-authenticated Auto root check. Uses a fresh daemon home and
// repository; never changes the owner's checkout or approves a tool request.
// Codex can enforce read-only sandboxing. Claude's current Auto route
// advertises workspace-write only, so its prompt is read-only but its
// selected sandbox is reported honestly below.
// Run: node test/local/auto-live.js codex
//      node test/local/auto-live.js claude
'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const harness = process.argv[2];
if (!['codex', 'claude'].includes(harness)) {
  console.error('Choose codex or claude.');
  process.exit(2);
}
const profileId = harness === 'codex' ? 'system-codex' : 'system-claude';
const sandbox = harness === 'codex' ? 'read_only' : 'workspace_write';
const base = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-auto-live-'));
const home = path.join(base, 'home');
const repo = path.join(base, 'repo');
const env = { ...process.env, OVERSEER_HOME: home };
// crypto, rather than a prompt literal, gives the read-only task a fresh
// answer that the model can obtain only by reading the scratch repository.
const random = require('crypto').randomBytes(4).toString('hex');
const expected = 'willow-' + random;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function ctl(method, params = {}) {
  const call = cp.spawnSync(bin, ['ctl', method, JSON.stringify(params)], {
    env, encoding: 'utf8', timeout: 30000, maxBuffer: 8 * 1024 * 1024,
  });
  const line = (call.stdout || '').split('\n')[0];
  let message;
  try { message = JSON.parse(line); }
  catch { throw new Error(`${method}: no daemon response (${call.status})`); }
  if (message.error) throw new Error(`${method}: ${message.error.message}`);
  return message.result;
}

function initRepo() {
  fs.mkdirSync(home);
  fs.mkdirSync(repo);
  const git = (...args) => cp.execFileSync('git', args, { cwd: repo, stdio: 'ignore' });
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'Auto Live Check');
  git('config', 'user.email', 'auto@example.invalid');
  git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(repo, 'README.md'), `# Auto live check\n\nMarker: ${expected}\n`);
  git('add', '.');
  git('commit', '-q', '-m', 'base');
}

async function waitForRun(runId) {
  const until = Date.now() + 150000;
  while (Date.now() < until) {
    const run = ctl('state').runs.find(item => item.id === runId);
    if (!run) throw new Error('selected run disappeared');
    if (['completed', 'failed', 'interrupted'].includes(run.status)) return run;
    if (run.status === 'waiting_for_user') throw new Error('run requires user permission');
    await sleep(300);
  }
  ctl('run.interrupt', { run_id: runId });
  throw new Error('live run exceeded its two-and-a-half-minute observation bound');
}

async function main() {
  initRepo();
  const daemon = cp.spawn(bin, ['serve'], { env, stdio: 'ignore' });
  let runId;
  try {
    let ready = false;
    for (let i = 0; i < 100; i++) {
      try { ready = !!ctl('hello')?.version; } catch {}
      if (ready) break;
      await sleep(100);
    }
    if (!ready) throw new Error('isolated daemon did not start');
    const auth = ctl('profile.status', { id: profileId });
    if (!auth.logged_in) throw new Error(`${harness} subscription login unavailable`);
    ctl('auto.mode.set', { enabled: true });
    const workUnitId = `live-${harness}-${random}`;
    const request = {
      repo, work_unit_id: workUnitId, allowed_profiles: [profileId],
      min_tier: 'general', required_tools: [], sandbox,
      workspace_mode: 'worktree', approval_policy: 'never',
      execution_budget_ms: 120000, title: 'Read isolated marker',
      prompt: 'Read README.md in this repository and reply with only its Marker value.',
    };
    const previewRequest = { ...request };
    for (const field of ['prompt', 'title', 'workspace_mode', 'approval_policy']) {
      delete previewRequest[field];
    }
    const preview = ctl('auto.root.preview', previewRequest);
    if (!preview.decision?.selected) {
      const reasons = (preview.decision?.excluded || []).map(item => item.reason);
      throw new Error(`no eligible Auto route (${reasons.join(', ') || preview.decision?.reason || 'unknown'})`);
    }
    const started = ctl('auto.start', request);
    runId = started.run?.id;
    if (!runId) throw new Error(`Auto did not admit a root (${started.state})`);
    const run = await waitForRun(runId);
    const outputs = (ctl('events.list', { run_id: runId, limit: 1000 }).events || [])
      .filter(event => event.kind === 'output' && event.payload?.role === 'assistant')
      .map(event => event.payload.text || '');
    const state = ctl('state');
    const workspace = state.workspaces.find(item => item.id === run.workspace_id);
    const changedFiles = workspace ? cp.execFileSync('git', ['status', '--porcelain'], {
      cwd: workspace.path, encoding: 'utf8',
    }).trim().split('\n').filter(Boolean).length : null;
    const works = ctl('auto.usage.work.list', { limit: 10 }).work_units || [];
    const work = works.find(item => item.run_id === runId);
    const report = {
      implementation: cp.execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
        cwd: root, encoding: 'utf8',
      }).trim(),
      harness, profile_kind: 'system subscription', auth_method: auth.method,
      sandbox,
      selected: preview.decision.selected,
      quota: preview.selected_route?.quota || null,
      fit: preview.selected_route?.fit || null,
      status: run.status, assistant_outputs: outputs.length,
      marker_read: outputs.some(output => output.includes(expected)),
      work_recorded: !!work,
      usage_observations: work?.usage_observations ?? null,
      usage_recorded: !!work?.usage,
      quota_before_source: work?.quota_before?.source || null,
      quota_after_source: work?.quota_after?.source || null,
      subscription_window_draw: work?.subscription_window_draw || null,
      work_outcome: work?.status || null,
      run_count: state.runs.length, changed_files: changedFiles,
    };
    const passed = run.status === 'completed' && report.marker_read
      && report.run_count === 1 && report.changed_files === 0
      && report.work_recorded;
    process.stdout.write(JSON.stringify({ ...report, passed }) + '\n');
    if (!passed) process.exitCode = 1;
  } finally {
    if (runId) {
      try {
        const run = ctl('state').runs.find(item => item.id === runId);
        if (run && !['completed', 'failed', 'interrupted'].includes(run.status)) {
          ctl('run.interrupt', { run_id: runId });
        }
      } catch {}
    }
    try { ctl('daemon.shutdown'); } catch {}
    daemon.kill();
    await sleep(200);
    fs.rmSync(base, { recursive: true, force: true });
  }
}

main().catch(error => {
  console.error(error.message);
  process.exitCode = 1;
  fs.rmSync(base, { recursive: true, force: true });
});
