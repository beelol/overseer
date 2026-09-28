// Isolated packaged-UI check for a 100-job Swarm backlog. This is not a live harness
// qualification: no workers are launched, and the fixture-only plan API stays on the test daemon.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

function benefitEstimate(ids, parallelContext = 20) {
  const workers = ids.map(id => ({ id, elapsed_ms: 100, usage_milli: { points: 10 } }));
  const phase = elapsed_ms => ({ elapsed_ms, usage_milli: { points: 1 } });
  const costs = context => ({ planning: phase(10), context: phase(context),
    integration: phase(10), review: phase(10), retries: phase(0), workers });
  return { independent: true, max_workers: 8,
    allocation_milli: { points: 100000 }, finishing_reserve_milli: { points: 20000 },
    serial: costs(10), parallel: costs(parallelContext) };
}

(async () => {
  const s = new Session('swarm-status');
  const result = { checks: [] };
  const check = (name, ok, detail) => {
    result.checks.push({ name, ok: !!ok, detail });
    s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail);
  };
  try {
    const repo = makeRepo(path.join(s.root, 'backend'), { dirty: false });
    s.settings({ 'window.menuStyle': 'custom', 'overseer.experimental.swarm': true });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_SWARM_FIXTURE_API: '1', OVERSEER_CODEX_PATH: '/nonexistent/codex',
      OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const made = s.ctl('swarm.create', { category: 'Backend audit', objective: 'Audit Atlas routes', allowed_targets: [] });
    const jobs = Array.from({ length: 100 }, (_, n) => ({ id: `j${String(n).padStart(3, '0')}`,
      title: `Check route ${n}`, acceptance: `Record route ${n} evidence`, deps: [] }));
    s.ctl('swarm.plan', { id: made.id, generation: 1, revision: 0, jobs });
    s.ctl('agents.limit.set', { max_active: 4 });
    const planning = s.ctl('swarm.benefit.commit', { run_id: made.id, generation: 1,
      revision: 1, estimate: benefitEstimate(jobs.slice(0, 4).map(job => job.id)) });
    check('the recorded beneficial batch is scaled below the eight-worker ceiling',
      planning.decision === 'parallel' && planning.max_parallel_workers === 3, planning);
    const at = Date.now();
    const availability = s.ctl('swarm.availability.observe', { run_id: made.id,
      now_ms: at, purpose: 'worker', required_capabilities: ['code'],
      estimate_milli: { points: 100 }, snapshot: {
        version: 1, observed_ms: at, expires_ms: at + 60000,
        targets: [], pools: [] } });
    check('no approved target records a blocked eligibility reason',
      availability.state === 'blocked' && availability.reason === 'no_allowed_target', availability);
    await cdp.command('Overseer: Refresh');
    await s.openOverseerView();
    const rows = await cdp.waitFor(`(() => {
      const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      const rows = [...(pane?.querySelectorAll('.monaco-list-row') || [])].filter(r => r.offsetParent);
      if (!rows.some(r => r.querySelector('.label-name')?.textContent.trim() === 'Backend audit')) return null;
      return rows.map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '', level: Number(r.getAttribute('aria-level')) }));
    })()`, 20000, 'Swarm category row');
    check('Agents sidebar shows one Swarms section and a 100-job category summary',
      rows.some(r => r.label === 'Swarms' && r.level === 1) &&
      rows.some(r => r.label === 'Backend audit' && /0 working/.test(r.description) && /100 ready/.test(r.description)), rows);
    await s.screenshot('swarm-compact');
    if (!rows.some(r => r.label === 'Director')) await s.clickAgentRow('Backend audit', { twisty: true });
    const expanded = await s.agentRows();
    check('expanding the category shows its director and first job without opening transcripts',
      expanded.some(r => r.label === 'Director') && expanded.some(r => r.label === 'Check route 0'),
      expanded.slice(0, 8));
    await s.screenshot('swarm-expanded');
    await s.clickAgentRow('Capacity', { twisty: true });
    const capacity = await s.agentRows();
    check('capacity details explain the recorded blocked eligibility',
      capacity.some(r => /Last eligibility: blocked/i.test(r.label) &&
        /no allowed target/i.test(r.description)), capacity.slice(0, 12));
    check('capacity details show the scaled-down planning decision',
      capacity.some(r => /Planning: parallel/i.test(r.label) &&
        /3 of 8 worker ceiling.*beneficial/i.test(r.description)), capacity.slice(0, 12));
    await s.screenshot('swarm-blocked-capacity');
    const serial = s.ctl('swarm.benefit.commit', { run_id: made.id, generation: 1,
      revision: 1, estimate: benefitEstimate(jobs.slice(0, 4).map(job => job.id), 1000) });
    check('expensive coordination records a serial decision',
      serial.decision === 'serial' && serial.reason === 'no_time_benefit', serial);
    await cdp.command('Overseer: Refresh');
    const serialRows = await s.agentRows();
    check('capacity details explain the serial decision',
      serialRows.some(r => /Planning: serial/i.test(r.label) &&
        /no time benefit/i.test(r.description)), serialRows.slice(0, 12));
    await s.screenshot('swarm-serial-capacity');

    const chooseControl = async (title, category = 'Backend audit') => {
      const pt = await cdp.waitFor(`(() => {
        const row = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent &&
          r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(category)});
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
      await cdp.click(action.x, action.y);
    };
    const waitStatus = async (status, id = made.id) => {
      for (let n = 0; n < 40; n++) {
        if (s.ctl('swarm.get', { id }).status === status) return true;
        await delay(100);
      }
      return false;
    };
    const priorDeadlineMs = s.ctl('swarm.get', { id: made.id }).policy.effective.deadline_ms;
    await chooseControl('Extend Swarm Deadline…');
    await cdp.waitFor(`document.querySelector('.quick-input-widget')?.textContent.includes('30 minutes')`,
      10000, 'deadline extension choices');
    await cdp.key('Enter');
    let extended;
    for (let n = 0; n < 40; n++) {
      extended = s.ctl('swarm.get', { id: made.id });
      if (extended.policy.effective.deadline_ms === priorDeadlineMs + 30 * 60 * 1000) break;
      await delay(100);
    }
    check('deadline extension from the Swarm row records 30 more minutes',
      extended.policy.effective.deadline_ms === priorDeadlineMs + 30 * 60 * 1000 &&
      extended.policy.sources.deadline_ms === 'run_extension',
      { before_ms: priorDeadlineMs, after_ms: extended.policy.effective.deadline_ms });
    let controlStarted = Date.now();
    await chooseControl('Pause Swarm');
    const paused = await waitStatus('paused');
    const pauseMs = Date.now() - controlStarted;
    check('Pause from the Swarm row changes durable daemon state within 2 seconds',
      paused && pauseMs < 2000, { pause_ms: pauseMs });
    await chooseControl('Resume Swarm');
    check('Resume from the Swarm row restores durable daemon state', await waitStatus('running'));
    await chooseControl('Turn Swarm Off');
    const off = s.ctl('swarm.get', { id: made.id });
    check('Swarm off cancels the queued backlog', ['draining', 'stopped'].includes(off.status) &&
      off.job_counts.by_status.ready === undefined, { status: off.status, counts: off.job_counts });

    const toStop = s.ctl('swarm.create', { category: 'Stop audit', objective: 'Audit one route', allowed_targets: [] });
    s.ctl('swarm.plan', { id: toStop.id, generation: 1, revision: 0,
      jobs: [{ id: 'one', title: 'Check one route', acceptance: 'Evidence', deps: [] }] });
    await cdp.command('Overseer: Refresh');
    await chooseControl('Stop Swarm…', 'Stop audit');
    await cdp.waitFor(`document.body.innerText.includes('Stop Stop audit swarm?')`, 10000, 'Stop confirmation');
    check('Stop waits for the owner confirmation', s.ctl('swarm.get', { id: toStop.id }).status !== 'stopped');
    controlStarted = Date.now();
    await cdp.key('Enter');
    const stopped = await waitStatus('stopped', toStop.id);
    const stopMs = Date.now() - controlStarted;
    check('confirmed Stop cancels queued work within 2 seconds', stopped && stopMs < 2000,
      { stop_ms: stopMs });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
