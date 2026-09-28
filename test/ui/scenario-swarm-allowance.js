// Packaged VS Code fixture for observed quota changes; no provider account is used.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix } = require('./harness');

(async () => {
  const s = new Session('swarm-allowance');
  const result = { checks: [] };
  const check = (name, ok, detail) => {
    result.checks.push({ name, ok: !!ok, detail });
    s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail);
  };
  try {
    const repo = makeRepo(path.join(s.root, 'backend'), { dirty: false });
    s.settings({ 'overseer.experimental.swarm': true });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_SWARM_FIXTURE_API: '1', OVERSEER_CODEX_PATH: '/nonexistent/codex',
      OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`,
      60000, 'Overseer status bar');
    const made = s.ctl('swarm.create', { category: 'Allowance audit',
      objective: 'Audit backend routes', allowed_targets: ['route-a'] });
    const at = Date.now();
    const snapshot = (time, remaining, confidence = 'exact') => ({
      version: 1, observed_ms: time, expires_ms: time + 60000,
      targets: [{ id: 'route-a', account_id: 'fixture-a', pool_ids: ['pool-a'],
        capabilities: ['code'], health: 'up', auth: 'ok' }],
      pools: [{ id: 'pool-a', windows: [{ id: 'week', unit: 'points',
        remaining_milli: remaining, protected_milli: 0, reserved_milli: 0,
        confidence, expires_ms: time + 60000 }] }],
    });
    const observe = (time, remaining, confidence) => s.ctl('swarm.availability.observe', {
      run_id: made.id, now_ms: time, purpose: 'worker', required_capabilities: ['code'],
      estimate_milli: { points: 100 }, snapshot: snapshot(time, remaining, confidence),
    });
    check('the newer fixture allowance blocks a worker',
      observe(at, 100000).state === 'eligible' &&
      observe(at + 1000, 500).reason === 'finishing_reserve');
    await cdp.command('Overseer: Refresh');
    await s.openOverseerView();
    await s.clickAgentRow('Allowance audit', { twisty: true });
    await s.clickAgentRow('Capacity', { twisty: true });
    const dropped = await s.agentRows();
    check('Capacity shows the last observed decrease in native units',
      dropped.some(row => /pool-a · week/.test(row.label) &&
        /0\.5 points.*down 99\.5 points/.test(row.description)) &&
      dropped.some(row => /Last eligibility: blocked/.test(row.label) &&
        /finishing reserve/.test(row.description)), dropped);
    await s.screenshot('allowance-drop');
    check('an unknown later balance remains unknown',
      observe(at + 2000, null, 'unknown').reason === 'unknown_quota');
    await cdp.command('Overseer: Refresh');
    await cdp.waitFor(`(() => {
      const row = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent &&
        r.querySelector('.label-name')?.textContent.trim() === 'pool-a · week');
      return row && /unknown/.test(row.querySelector('.label-description')?.textContent || '');
    })()`, 10000, 'updated allowance row');
    const unknown = await s.agentRows();
    check('Capacity shows unknown instead of zero or a fabricated change',
      unknown.some(row => /pool-a · week/.test(row.label) &&
        /unknown/.test(row.description) && !/0 points/.test(row.description)), unknown);
    await s.screenshot('allowance-unknown');
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
