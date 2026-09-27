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

function workerPids(home, runId) {
  const code = `import json, sqlite3, pathlib, sys
db = sqlite3.connect(sys.argv[1])
rows = db.execute("SELECT r.run_dir FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id WHERE l.run_id=?", (sys.argv[2],)).fetchall()
print(json.dumps([json.loads((pathlib.Path(row[0]) / 'shim.json').read_text())['child_pid'] for row in rows]))`;
  const out = cp.execFileSync('python3', ['-c', code, path.join(home, 'overseer.sqlite'), runId], { encoding: 'utf8' });
  return JSON.parse(out);
}

function liveWorkerPids(home, runId) {
  const pids = workerPids(home, runId);
  return { count: pids.length, live: pids.filter(pid => { try { process.kill(pid, 0); return true; } catch { return false; } }).length };
}

function blockReadyJobs(home, runId, ids) {
  const code = `import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
ids = sys.argv[3:]
rows = db.execute("SELECT id FROM swarm_jobs WHERE run_id=? AND status='ready' AND id IN (" + ",".join("?" for _ in ids) + ")", [sys.argv[2], *ids]).fetchall()
assert len(rows) == len(ids), (rows, ids)
db.executemany("UPDATE swarm_jobs SET status='blocked', stop_reason='fixture_blocker' WHERE run_id=? AND id=?", [(sys.argv[2], id) for id in ids])
db.commit()`;
  cp.execFileSync('python3', ['-c', code, path.join(home, 'overseer.sqlite'), runId, ...ids]);
}

function fixtureDirectorToken(home, directorRunId) {
  const code = `import json, pathlib, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
run_dir = db.execute("SELECT run_dir FROM runs WHERE id=?", (sys.argv[2],)).fetchone()[0]
print(json.loads((pathlib.Path(run_dir) / 'launch.json').read_text())['env']['OVERSEER_SWARM_DIRECTOR_TOKEN'])`;
  return cp.execFileSync('python3', ['-c', code, path.join(home, 'overseer.sqlite'), directorRunId],
    { encoding: 'utf8' }).trim();
}

(async () => {
  const s = new Session('swarm-scale');
  const result = { checks: [] };
  const check = (name, ok, detail) => {
    result.checks.push({ name, ok: !!ok, detail });
    s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail);
  };
  let runId;
  let directorRunId;
  try {
    const repo = makeRepo(path.join(s.root, 'backend'), { dirty: false });
    s.settings({ 'window.menuStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_SWARM_FIXTURE_API: '1', OVERSEER_CODEX_PATH: '/nonexistent/codex',
      OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const chooseControl = async title => {
      const pt = await cdp.waitFor(`(() => {
        const row = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent &&
          r.querySelector('.label-name')?.textContent.trim() === 'Large backend audit');
        if (!row) return null;
        const b = row.getBoundingClientRect(); return { x: b.left + 130, y: b.top + b.height / 2 };
      })()`, 10000, 'Swarm row');
      await cdp.click(pt.x, pt.y, { button: 'right' });
      await delay(400);
      s.note('Swarm context menu', await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-menu .action-item .action-label')]
        .map(a => a.getAttribute('aria-label') || a.textContent.trim()).filter(Boolean)`));
      const action = await cdp.waitFor(`(() => {
        const label = [...document.querySelectorAll('.monaco-menu .action-item .action-label')]
          .find(a => (a.getAttribute('aria-label') || a.textContent.trim()) === ${JSON.stringify(title)});
        if (!label) return null;
        const b = label.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
      })()`, 10000, title);
      const started = Date.now();
      await cdp.click(action.x, action.y);
      return started;
    };
    const waitStatus = async states => {
      for (let n = 0; n < 40; n++) {
        const state = s.ctl('swarm.get', { id: runId });
        if (states.includes(state.status)) return state;
        await delay(50);
      }
      return s.ctl('swarm.get', { id: runId });
    };

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
        program: n === 0 ? process.execPath : '/bin/sleep',
        args: n === 0 ? ['-e', 'process.on("SIGINT", () => {}); setTimeout(() => {}, 120000);'] : ['180'],
        prompt: 'Inspect', title: `Fixture worker ${n}` });
      if (launched.status !== 'launched') throw new Error(`worker ${n} not launched: ${JSON.stringify(launched)}`);
    }
    blockReadyJobs(s.home, runId, ['j032', 'j033', 'j034', 'j035']);
    const director = s.ctl('swarm.director.launch', { run_id: runId, generation: 1, repo,
      program: '/bin/sleep', args: ['180'], prompt: 'Direct the backend audit', title: 'Fixture director' });
    if (director.status !== 'launched') throw new Error(`director not launched: ${JSON.stringify(director)}`);
    directorRunId = director.overseer_run_id;
    let run;
    for (let n = 0; n < 100; n++) {
      run = s.ctl('swarm.get', { id: runId });
      if (run.active_worker_processes === 32 && run.director?.process_status === 'running') break;
      await delay(100);
    }
    const processes = liveWorkerPids(s.home, runId);
    check('32 distinct supervised fixture workers are alive',
      run.active_worker_processes === 32 && processes.count === 32 && processes.live === 32,
      { active_worker_processes: run.active_worker_processes, ...processes });
    check('remaining jobs include four blockers while 32 attempts hold reservations',
      run.job_counts.total === 100 && run.job_counts.by_status.reserved === 32 &&
      run.job_counts.by_status.ready === 64 && run.job_counts.by_status.blocked === 4, run.job_counts);
    check('supervised category director is running alongside 32 workers',
      run.director?.process_status === 'running' && run.director?.overseer_run_id === directorRunId,
      run.director);
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
    check('packaged Agents sidebar shows 32 working, 64 ready and four blocked',
      rows.some(row => row.label === 'Large backend audit' && /32 working/.test(row.description) &&
        /64 ready/.test(row.description) && /4 blocked/.test(row.description)), rows.slice(0, 4));
    check('workers are grouped under Swarm without duplicate ordinary agent rows',
      !rows.some(row => row.label === 'backend' || row.label === 'Fixture worker 0'), rows.slice(0, 4));
    await s.screenshot('32-workers-compact');
    await s.clickAgentRow('Large backend audit', { twisty: true });
    const expanded = await s.agentRows();
    check('expanded category shows director and bounded job page',
      expanded.some(row => row.label === 'Director' && row.description === 'running') &&
      expanded.some(row => row.label === 'Capacity') &&
      expanded.some(row => row.label === 'Inspect route 0') &&
      !expanded.some(row => row.label === 'Inspect route 99'), expanded.slice(0, 8));
    await s.screenshot('32-workers-expanded');
    await cdp.command('Overseer: Filter Swarm Jobs…');
    await cdp.waitQuickTitle('Show Swarm jobs');
    await cdp.type('Blocked');
    await cdp.key('Enter');
    const blockedRows = await cdp.waitFor(`(() => {
      const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent);
      const labels = rows.map(r => r.querySelector('.label-name')?.textContent.trim()).filter(Boolean);
      return labels.includes('Inspect route 32') ? labels : null;
    })()`, 10000, 'blocked Swarm jobs');
    check('packaged state filter shows four blocked jobs and no ready or reserved job rows',
      ['Inspect route 32', 'Inspect route 33', 'Inspect route 34', 'Inspect route 35'].every(label => blockedRows.includes(label)) &&
      !blockedRows.includes('Inspect route 0') && !blockedRows.includes('Inspect route 36'), blockedRows);
    await s.screenshot('blocked-jobs-filtered');
    await cdp.command('Overseer: Filter Swarm Jobs…');
    await cdp.waitQuickTitle('Show Swarm jobs');
    await cdp.type('All jobs');
    await cdp.key('Enter');
    await cdp.waitFor(`[...document.querySelectorAll('.monaco-list-row')].some(r =>
      r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Inspect route 0')`,
      10000, 'all Swarm jobs restored');
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
    check('selecting one live Swarm worker opens its run view without an ordinary-row error',
      await editor.eval(`document.body.textContent.includes('Fixture worker 0')`));
    let controlStarted = await chooseControl('Pause Swarm');
    const paused = await waitStatus(['paused']);
    check('Pause from the sidebar acknowledges within 2 seconds with 32 active workers',
      paused.status === 'paused' && Date.now() - controlStarted < 2000 &&
      paused.active_worker_processes === 32,
      { status: paused.status, active_worker_processes: paused.active_worker_processes,
        pause_ms: Date.now() - controlStarted });
    await chooseControl('Resume Swarm');
    const resumed = await waitStatus(['running']);
    check('Resume preserves the same 32 supervised workers',
      resumed.status === 'running' && resumed.active_worker_processes === 32,
      { status: resumed.status, active_worker_processes: resumed.active_worker_processes });
    await chooseControl('Turn Swarm Off');
    const draining = await waitStatus(['draining']);
    check('Swarm off drains 32 active workers and cancels the queued backlog',
      draining.status === 'draining' && draining.active_worker_processes === 32 &&
      draining.job_counts.by_status.cancelled === 64 && draining.job_counts.by_status.blocked === 4 &&
      liveWorkerPids(s.home, runId).live === 32,
      { status: draining.status, active_worker_processes: draining.active_worker_processes,
        job_counts: draining.job_counts });
    const afterOff = s.ctl('swarm.admit', { run_id: runId, generation: 1, revision: 1,
      owner_token: fixtureDirectorToken(s.home, directorRunId),
      job_id: 'j036', target_id: 'fixture-local', request_id: 'after-swarm-off',
      now_ms: now(), snapshot, required_capabilities: ['code'],
      estimate_milli: { points: 100 }, purpose: 'worker' });
    check('Swarm off rejects new worker admission',
      afterOff.status === 'blocked' && afterOff.reason === 'run_not_admitting', afterOff);
    await chooseControl('Stop Swarm…');
    await cdp.waitFor(`document.body.innerText.includes('Stop Large backend audit swarm?')`, 10000,
      'Stop confirmation');
    check('Stop waits for owner confirmation while workers remain active',
      s.ctl('swarm.get', { id: runId }).active_worker_processes === 32);
    controlStarted = Date.now();
    await cdp.key('Enter');
    const stopping = await waitStatus(['stopping', 'stopped']);
    check('confirmed Stop acknowledges within 2 seconds with 32 active workers',
      ['stopping', 'stopped'].includes(stopping.status) && Date.now() - controlStarted < 2000,
      { status: stopping.status, unconfirmed_exit_count: stopping.unconfirmed_exit_count,
        stop_ms: Date.now() - controlStarted });
    check('the SIGINT-resistant fixture worker remains unconfirmed with its estimate reserved',
      stopping.status === 'stopping' && stopping.unconfirmed_exit_count > 0 &&
      stopping.capacity?.windows?.[0]?.outstanding_estimate_milli >= 100,
      { unconfirmed_exit_count: stopping.unconfirmed_exit_count, capacity: stopping.capacity });
    await cdp.command('Overseer: Refresh');
    const unconfirmedRows = await cdp.waitFor(`(() => {
      const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent);
      const found = rows.find(r => /\\d+ exits? unconfirmed/.test(r.querySelector('.label-name')?.textContent.trim() || ''));
      return found && rows.map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(),
        description: r.querySelector('.label-description')?.textContent.trim() || '' }));
    })()`, 5000, 'unconfirmed exits row');
    check('sidebar shows unconfirmed exits separately while Stop is pending',
      unconfirmedRows.some(row => /\d+ exits? unconfirmed/.test(row.label)),
      unconfirmedRows.slice(0, 9));
    await s.screenshot('stopping-unconfirmed');
    let exited;
    for (let n = 0; n < 150; n++) {
      exited = s.ctl('swarm.get', { id: runId });
      if (exited.status === 'stopped' && exited.unconfirmed_exit_count === 0 &&
          liveWorkerPids(s.home, runId).live === 0) break;
      await delay(100);
    }
    check('Stop escalation confirms the final worker exit and clears uncertainty',
      exited.status === 'stopped' && exited.unconfirmed_exit_count === 0 &&
      liveWorkerPids(s.home, runId).live === 0,
      { status: exited.status, unconfirmed_exit_count: exited.unconfirmed_exit_count });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    if (runId) { try { s.ctl('swarm.stop', { run_id: runId }); } catch (error) { s.note('cleanup Stop failed', error.message); } }
    if (directorRunId) { try { s.ctl('run.interrupt', { run_id: directorRunId }); } catch (error) { s.note('cleanup director interrupt failed', error.message); } }
    let forcedCleanup = false;
    if (runId && !process.env.KEEP_OPEN) {
      try {
        for (let n = 0; n < 130 && liveWorkerPids(s.home, runId).live > 0; n++) await delay(100);
        const survivors = workerPids(s.home, runId).filter(pid => { try { process.kill(pid, 0); return true; } catch { return false; } });
        if (survivors.length) {
          forcedCleanup = true;
          s.note('Fixture workers needed forced cleanup', survivors);
          for (const pid of survivors) { try { process.kill(pid, 'SIGTERM'); } catch {} }
          await delay(300);
          for (const pid of survivors) { try { process.kill(pid, 0); process.kill(pid, 'SIGKILL'); } catch {} }
        }
      } catch (error) { forcedCleanup = true; s.note('Fixture cleanup inspection failed', error.message); }
    }
    if (directorRunId && !process.env.KEEP_OPEN) {
      let director;
      for (let n = 0; n < 80; n++) {
        director = s.ctl('state').runs.find(row => row.id === directorRunId);
        if (director?.ended_ms) break;
        await delay(100);
      }
      check('fixture director also confirms exit during teardown',
        !!director?.ended_ms && !['queued', 'starting', 'running', 'waiting_for_user'].includes(director.status),
        { status: director?.status, ended_ms: director?.ended_ms });
    }
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    if (runId && !process.env.KEEP_OPEN) {
      let processes;
      for (let n = 0; n < 50; n++) {
        processes = liveWorkerPids(s.home, runId);
        if (processes.live === 0) break;
        await delay(100);
      }
      check('all fixture worker processes exit after Stop and teardown',
        processes.count === 32 && processes.live === 0 && !forcedCleanup,
        { ...processes, forced_cleanup: forcedCleanup });
    }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
