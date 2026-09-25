// Packaged-UI scenario: in an untrusted (Restricted Mode) workspace Overseer cannot launch
// or control agents. Uses VS Code's real workspace-trust implementation.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('trust');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'repo'));
    s.settings({ 'security.workspace.trust.enabled': true, 'security.workspace.trust.startupPrompt': 'never', 'security.workspace.trust.emptyWindow': false, 'security.workspace.trust.banner': 'always' });
    s.install(latestVsix());
    // An empty window with security.workspace.trust.emptyWindow=false is untrusted (Restricted Mode).
    s.launch('--new-window', { OVERSEER_TEST_TRUST: '1' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    await cdp.command('Workspaces: Manage Workspace Trust');
    await delay(1500);
    const restricted = await cdp.evalWorkbench(`/You are in Restricted Mode|You have not trusted/.test(document.body.innerText) || !/You trust this (folder|window)/.test(document.body.innerText)`);
    await s.screenshot('trust-editor');
    await cdp.key('w', { meta: true });
    check('workspace opened in Restricted Mode', restricted);
    await cdp.key('p', { meta: true, shift: true });
    await delay(500);
    await cdp.type('Overseer: New Task');
    await delay(800);
    const qi = await cdp.quickInputState();
    await s.screenshot('palette');
    check('New Task is not offered/enabled in Restricted Mode', !qi.rows.some(r => /Overseer: New Task/.test(r)), qi.rows);
    await cdp.key('Escape');
    const showOverseer = await cdp.quickInputState();
    check('no task flow started', !showOverseer || !/New task/.test(showOverseer.title), showOverseer);
    const icon = await cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item a, .activitybar .action-label')].find(a => /^Overseer/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(icon.x, icon.y);
    await delay(1500);
    const welcome = await cdp.evalWorkbench(`document.body.innerText.includes('Launching agents requires a trusted workspace')`);
    check('view explains that launching requires trust', welcome);
    const tasks = s.ctl('state').tasks;
    check('no task or harness launched', tasks.length === 0, tasks.length);
    await s.screenshot('restricted');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    await s.quit(); s.stopDaemon();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
