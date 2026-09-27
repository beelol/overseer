// Packaged Auto root entry point: a user chooses Auto in the composer and the daemon selects
// the first model/effort before a fixture turn. The isolated profile and synthetic app-server
// spend no model tokens. This is only one slice of AUTO-AC-25, not its browser-child Verify clause.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('auto-root');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const app = path.join(repoRoot, 'fixtures/fake-harness/codex-app-fixture.js');
  const sys = path.join(s.root, 'desktop-home');
  const next = path.join(s.root, 'next-login');
  const trace = path.join(s.root, 'app-server-trace.txt');
  fs.mkdirSync(sys, { recursive: true });
  fs.writeFileSync(next, 'auto-fixture:pro');
  cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  try {
    const repo = makeRepo(path.join(s.root, 'auto-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: app, OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_MODE: 'managed-models', FIXTURE_TRACE_FILE: trace,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_MODE,FIXTURE_TRACE_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    check('Auto gate is off before an explicit UI choice', s.ctl('auto.mode.get').enabled === false);
    await cdp.command('Overseer: Open Overseer View');
    let dash = await s.editorView();
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    const agentPoint = await s.webviewPoint(dash, '[data-chip="agent"]');
    await cdp.click(agentPoint.x, agentPoint.y); await delay(300);
    if (!(await dash.eval(`!!document.querySelector('.menu')`))) await cdp.key('Enter');
    await dash.waitFor(`!!document.querySelector('.menu')`, 5000);
    await dash.eval(`[...document.querySelectorAll('.menu .menu-item')].find(e => e.textContent.startsWith('Auto routing · any eligible agent')).click()`);
    const point = await s.webviewPoint(dash, '#task');
    await cdp.click(point.x, point.y); await cdp.type('seed context');
    await dash.waitFor(`!document.getElementById('start').disabled`, 5000);
    await cdp.key('Enter');
    let root;
    for (let i = 0; i < 80 && !root; i++) {
      await delay(250);
      root = s.ctl('state').runs.find(r => !r.parent_run_id && r.title === 'seed context');
    }
    if (!root) throw new Error('Auto root was not created: ' + await dash.eval(`document.querySelector('.composer-note')?.textContent`));
    for (let i = 0; i < 80 && ['queued', 'starting', 'running'].includes(root.status); i++) {
      await delay(250); root = s.ctl('state').runs.find(r => r.id === root.id);
    }
    const events = s.ctl('events.list', { after: 0, limit: 2000 }).events;
    const decision = events.find(e => e.kind === 'auto_decision');
    const selected = decision?.payload?.decision?.selected;
    const output = s.ctl('events.list', { run_id: root.id, limit: 2000 }).events
      .filter(e => e.kind === 'output').map(e => e.payload?.text || '').join('\n');
    check('the UI enabled Auto and the daemon chose one model and effort before the fixture turn',
      s.ctl('auto.mode.get').enabled && root.status === 'completed' && root.model && root.effort && selected && /parent ready/.test(output),
      { run: root.id, status: root.status, model: root.model, effort: root.effort, selected, output });
    const shown = await dash.waitFor(`document.body.dataset.mode === 'chat' && document.getElementById('title')?.textContent === 'seed context'`, 20000).then(() => true, () => false);
    check('the selected Auto run opens in the same chat surface as a manual run', shown);
    await s.screenshot('auto-root-chat');
    // Leaving a webview textarea focused can swallow the command-palette shortcut.
    const statusPoint = await cdp.evalWorkbench(`(() => { const r = document.querySelector('.part.statusbar').getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    await cdp.click(statusPoint.x, statusPoint.y);
    await cdp.command('Overseer: New Agent');
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    const autoDefault = await dash.eval(`document.querySelector('[data-chip="agent"]').getAttribute('aria-label')`);
    const nextPoint = await s.webviewPoint(dash, '#task');
    await cdp.click(nextPoint.x, nextPoint.y); await cdp.type('fixture: delegate browser then diagnose');
    await dash.waitFor(`!document.getElementById('start').disabled`, 5000);
    await cdp.key('Enter');
    let parent;
    for (let i = 0; i < 100 && !parent; i++) {
      await delay(250);
      parent = s.ctl('state').runs.find(r => !r.parent_run_id && r.title === 'fixture: delegate browser then diagnose');
    }
    if (!parent) throw new Error('Auto parent with delegated browser work was not created');
    let children = [];
    for (let i = 0; i < 120; i++) {
      await delay(250);
      const runs = s.ctl('state').runs;
      parent = runs.find(r => r.id === parent.id);
      children = runs.filter(r => r.parent_run_id === parent.id);
      if (parent.status === 'completed' && children.length === 2 && children.every(r => r.status === 'completed')) break;
    }
    const parentOutput = s.ctl('events.list', { run_id: parent.id, limit: 2000 }).events
      .filter(e => e.kind === 'output').map(e => e.payload?.text || '').join('\n');
    const childModels = children.map(r => `${r.model}/${r.effort}`);
    check('Auto parent receives a browser-labelled child result and assigns a stronger diagnosis unit while healthy',
      /Auto routing/.test(autoDefault) && parent.status === 'completed' && children.length === 2 && children.every(r => r.status === 'completed') &&
      childModels.includes('gpt-6-sol/medium') && childModels.includes('gpt-6-astra/high') &&
      /browser result: parent context found/.test(parentOutput) && /continued with browser result/.test(parentOutput),
      { parent: parent.id, status: parent.status, children: children.map(r => ({ id: r.id, model: r.model, effort: r.effort, status: r.status })), output: parentOutput });
    await s.screenshot('auto-delegation');
    await cdp.command('Developer: Reload Window'); await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    dash = await s.editorView();
    const after = s.ctl('state').runs.filter(r => !r.parent_run_id && r.title === 'seed context');
    await cdp.command('Overseer: New Agent');
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    const remembered = await dash.eval(`document.querySelector('[data-chip="agent"]').getAttribute('aria-label')`);
    check('window reload keeps the original Auto run and remembers Auto as the next choice', after.length === 1 && after[0].id === root.id && /Auto routing/.test(remembered),
      { runs: after.map(r => r.id), remembered });
    await s.screenshot('auto-remembered');
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
