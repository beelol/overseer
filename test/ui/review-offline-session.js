// A TOOL, not a scenario (so scripts/test-all does not pick it up): reopens a RECORDED offline session (the OVERSEER_HOME, profile and extensions folder that
// scenario-offline-session.js left behind) in the current build, without running any agent: the
// chats are drawn from the daemon's stored events. For checking an interface change on real
// data without spending a paid turn again.
//
//   node test/ui/review-offline-session.js /tmp/ovs-ui-XXXXXX
const fs = require('fs');
const path = require('path');
const { Session, latestVsix, delay } = require('./harness');

(async () => {
  const recorded = process.argv[2];
  if (!recorded || !fs.existsSync(path.join(recorded, 'overseer-home'))) { console.error('usage: review-offline-session.js <recorded session folder>'); process.exit(2); }
  const s = new Session('offline-session-review');
  Object.assign(s, { home: path.join(recorded, 'overseer-home'), profile: path.join(recorded, 'profile'), extensions: path.join(recorded, 'extensions') });
  const result = { checks: [], recorded: path.basename(recorded) };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    s.install(latestVsix());
    s.launch(path.join(recorded, 'offline-repo'));
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const runs = s.ctl('state').runs;
    const first = runs.find(r => r.harness === 'codex' && r.status === 'handed_off');
    await s.selectRun(first.id, { settle: 1500 });
    if (!(await s.agentRows()).some(r => /^Earlier: Codex/.test(r.label || ''))) await s.clickAgentRow('Offline session', { twisty: true });
    await s.clickAgentRow('Earlier: Codex');
    const dash = await s.editorView(`[...document.querySelectorAll('#conv .cont-note')].some(n => /Transitioning/.test(n.textContent))`);
    const chat = await dash.eval(`({ red: [...document.querySelectorAll('#conv .error-block')].map(e => e.textContent.slice(0, 60)), quiet: [...document.querySelectorAll('#conv [data-continuity="network"]')].map(n => ({ text: n.textContent, tip: n.title })), notes: [...document.querySelectorAll('#conv .cont-note')].map(n => n.textContent) })`);
    check('the lost connection is one quiet line with the number of attempts, not a stack of red alerts', chat.red.length === 0 && chat.quiet.length === 1 && /The connection was lost; the agent keeps trying to reconnect\. · \d+ attempts/.test(chat.quiet[0].text), chat);
    await s.screenshot('predecessor-quiet');
    const accounts = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Local models').map(r => r.querySelector('.label-description')?.textContent.trim())`);
    check('under Accounts, local models need no account', accounts.length === 1 && accounts[0] === 'no account needed', accounts);
    await s.screenshot('accounts-local');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    await s.quit(); s.stopDaemon();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED');
    process.exit(failed ? 1 : 0);
  }
})();
