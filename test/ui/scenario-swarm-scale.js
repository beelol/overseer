// Packaged-UI scale check: 32 supervised local fixture workers in a 100-job category.
// This does not use provider accounts or qualify live harness communication.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

function now() { return Date.now(); }

function benefitEstimate(ids) {
  const workers = ids.map(id => ({ id, elapsed_ms: 100, usage_milli: { points: 10 } }));
  const phase = elapsed_ms => ({ elapsed_ms, usage_milli: { points: 1 } });
  const costs = context => ({ planning: phase(10), context: phase(context),
    integration: phase(10), review: phase(10), retries: phase(0), workers });
  return { independent: true, max_workers: ids.length,
    allocation_milli: { points: 100000 }, finishing_reserve_milli: { points: 20000 },
    serial: costs(10), parallel: costs(20) };
}

function liveWorkerPids(home, runId) {
  const code = `import json, sqlite3, pathlib, sys
db = sqlite3.connect(sys.argv[1])
rows = db.execute("SELECT r.run_dir FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id WHERE l.run_id=?", (sys.argv[2],)).fetchall()
print(json.dumps([json.loads((pathlib.Path(row[0]) / 'shim.json').read_text())['child_pid'] for row in rows]))`;
  const out = cp.execFileSync('python3', ['-c', code, path.join(home, 'overseer.sqlite'), runId], { encoding: 'utf8' });
  const pids = JSON.parse(out);
  return { count: pids.length, live: pids.filter(pid => { try { process.kill(pid, 0); return true; } catch { return false; } }).length };
}

(async () => {
  const s = new Session('swarm-scale');
  const result = { checks: [] };
  const check = (name, ok, detail) => {
    result.checks.push({ name, ok: !!ok, detail });
    s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail);
  };
  let runId;
  try {
    const repo = makeRepo(path.join(s.root, 'backend'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_SWARM_FIXTURE_API: '1', OVERSEER_CODEX_PATH: '/nonexistent/codex',
      OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');

    s.ctl('agents.limit.set', { max_active: 33 });
    const made = s.ctl('swarm.create', { category: 'Large backend audit', objective: 'Inspect 100 independent routes',
      allowed_targets: ['fixture-local'], policy: { max_workers: 32, deadline_ms: 240000 } });
    runId = made.id;
    const jobs = Array.from({ length: 100 }, (_, n) => ({ id: `j${String(n).padStart(3, '0')}`,
      title: `Inspect route ${n}`, acceptance: `Record route ${n} evidence`, deps: [] }));
    s.ctl('swarm.plan', { id: runId, generation: 1, revision: 0, jobs });
    const firstWave = jobs.slice(0, 32).map(job => job.id);
    const benefit = s.ctl('swarm.benefit.commit', { run_id: runId, generation: 1, revision: 1,
      estimate: benefitEstimate(firstWave) });
    check('fixture benefit decision permits the planned 32-worker wave', benefit.decision === 'parallel', benefit);

    const at = now();
    const snapshot = { version: 1, observed_ms: at - 1000, expires_ms: at + 240000,
      targets: [{ id: 'fixture-local', account_id: 'fixture', pool_ids: ['fixture-pool'],
        capabilities: ['code'], health: 'up', auth: 'ok' }],
      pools: [{ id: 'fixture-pool', windows: [{ id: 'run', unit: 'points', remaining_milli: 1000000,
        protected_milli: 0, reserved_milli: 0, confidence: 'exact', expires_ms: at + 240000 }] }] };
    for (let n = 0; n < 32; n++) {
      const jobId = firstWave[n];
      const admitted = s.ctl('swarm.admit', { run_id: runId, generation: 1, revision: 1,
        job_id: jobId, target_id: 'fixture-local', request_id: `ui-scale-${n}`,
        now_ms: at + Math.floor(n / 4) * 5000, snapshot,
        required_capabilities: ['code'], estimate_milli: { points: 100 }, purpose: 'worker' });
      if (admitted.status !== 'admitted') throw new Error(`worker ${n} not admitted: ${JSON.stringify(admitted)}`);
      const launched = s.ctl('swarm.worker.launch', { run_id: runId, job_id: jobId,
        attempt_id: admitted.attempt_id, token: admitted.token, repo,
        program: '/bin/sleep', args: ['180'], prompt: 'Inspect', title: `Fixture worker ${n}` });
      if (launched.status !== 'launched') throw new Error(`worker ${n} not launched: ${JSON.stringify(launched)}`);
    }
    let run;
    for (let n = 0; n < 100; n++) {
      run = s.ctl('swarm.get', { id: runId });
      if (run.active_worker_processes === 32) break;
      await delay(100);
    }
    const processes = liveWorkerPids(s.home, runId);
    check('32 distinct supervised fixture workers are alive',
      run.active_worker_processes === 32 && processes.count === 32 && processes.live === 32,
      { active_worker_processes: run.active_worker_processes, ...processes });
    check('remaining 68 jobs stay ready while 32 attempts hold reservations',
      run.job_counts.total === 100 && run.job_counts.by_status.reserved === 32 &&
      run.job_counts.by_status.ready === 68, run.job_counts);
    check('daemon exposes only fixture commitments, with provider usage unknown',
      run.capacity?.provider_usage_state === 'unknown' &&
      run.capacity?.selected_targets?.[0]?.id === 'fixture-local' &&
      run.capacity?.selected_targets?.[0]?.attempts === 32 &&
      run.capacity?.windows?.[0]?.allocation_milli === 100000 &&
      run.capacity?.windows?.[0]?.finishing_reserve_milli === 20000 &&
      run.capacity?.windows?.[0]?.outstanding_estimate_milli === 3200,
      run.capacity);

    await cdp.command('Overseer: Refresh');
    await s.openOverseerView();
    const rows = await cdp.waitFor(`(() => {
      const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      const rows = [...(pane?.querySelectorAll('.monaco-list-row') || [])].filter(r => r.offsetParent);
      const match = rows.find(r => r.querySelector('.label-name')?.textContent.trim() === 'Large backend audit');
      if (!match) return null;
      return rows.map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(),
        description: r.querySelector('.label-description')?.textContent.trim() || '',
        level: Number(r.getAttribute('aria-level')) }));
    })()`, 20000, 'large Swarm row');
    check('packaged Agents sidebar shows 32 working and 68 ready',
      rows.some(row => row.label === 'Large backend audit' && /32 working/.test(row.description) &&
        /68 ready/.test(row.description)), rows.slice(0, 4));
    check('workers are grouped under Swarm without duplicate ordinary agent rows',
      !rows.some(row => row.label === 'backend' || row.label === 'Fixture worker 0'), rows.slice(0, 4));
    await s.screenshot('32-workers-compact');
    await s.clickAgentRow('Large backend audit', { twisty: true });
    const expanded = await s.agentRows();
    check('expanded category shows director and bounded job page',
      expanded.some(row => row.label === 'Director') && expanded.some(row => row.label === 'Capacity') &&
      expanded.some(row => row.label === 'Inspect route 0') &&
      !expanded.some(row => row.label === 'Inspect route 99'), expanded.slice(0, 8));
    await s.screenshot('32-workers-expanded');
    await s.clickAgentRow('Capacity', { twisty: true });
    const capacityRows = await s.agentRows();
    check('expanded capacity names target, reserve and planning decision without claiming live usage',
      capacityRows.some(row => row.label === 'Target: fixture-local') &&
      capacityRows.some(row => row.label === 'Allocation: fixture-pool / run' &&
        /finishing reserve 20 points/.test(row.description)) &&
      capacityRows.some(row => row.label === 'Planning: parallel') &&
      capacityRows.some(row => row.label === 'Current limit unknown') &&
      capacityRows.some(row => row.label === 'Provider usage unknown'),
      capacityRows.filter(row => row.level === 4).slice(0, 8));
    await s.screenshot('capacity-expanded');
    await s.clickAgentRow('Inspect route 0', { twisty: true });
    const withWorker = await s.agentRows();
    check('an active job opens its supervised worker without mounting all transcripts',
      withWorker.some(row => row.label === 'Worker' && row.description === 'working') &&
      withWorker.filter(row => row.label === 'Worker').length === 1,
      withWorker.filter(row => row.label === 'Worker'));
    await s.screenshot('one-worker-open');
    await s.clickAgentRow('Worker');
    const editor = await s.editorView();
    check('selecting one Swarm worker opens its run view',
      await editor.eval(`document.body.textContent.includes('Fixture worker 0')`));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    if (runId) { try { s.ctl('swarm.stop', { run_id: runId }); } catch (error) { s.note('cleanup Stop failed', error.message); } }
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    if (runId && !process.env.KEEP_OPEN) {
      let processes;
      for (let n = 0; n < 50; n++) {
        processes = liveWorkerPids(s.home, runId);
        if (processes.live === 0) break;
        await delay(100);
      }
      check('all fixture worker processes exit after Stop and teardown',
        processes.count === 32 && processes.live === 0, processes);
    }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
