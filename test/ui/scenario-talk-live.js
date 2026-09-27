// LIVE paid scenario for AC-107 (one tiny Claude Haiku turn on the existing login): Talk to Overseer
// on the real Claude Code harness. One agent is working; the owner asks "What is everyone doing?";
// Overseer's answer names that agent. One attempt, no retries.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('talk-live');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  let worker;
  try {
    const repo = makeRepo(path.join(s.root, 'talk-live-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.chat.harness': 'claude', 'overseer.chat.model': 'haiku' });
    s.install(latestVsix());
    s.launch(repo, {});
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const status = s.ctl('profile.status', { id: 'system-claude' });
    s.note('claude login', { signedIn: status.signed_in ?? status.status, plan: status.plan });
    worker = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo "migrating the billing tables"; sleep 600'], prompt: '', title: 'Billing migration' });
    await delay(2000);

    await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
    const intro = await cdp.webview(`!!document.querySelector('.talk-intro')`, 20000);
    { const p = await s.webviewPoint(intro, '#prompt'); await cdp.click(p.x, p.y); }
    await cdp.type('What is everyone doing? One short sentence per agent.'); await cdp.key('Enter');
    const chat = await cdp.webview(`!!document.querySelector('#conv') && /What is everyone doing/.test(document.querySelector('#conv').textContent)`, 40000);
    const done = await chat.waitFor(`!!document.querySelector('#conv .turn-foot:not([hidden])')`, 120000).then(() => true, () => false);
    const text = await chat.eval(`document.querySelector('#conv').innerText`);
    // The run's status can lag the turn's footer by a moment.
    let run; for (let i = 0; i < 40; i++) { run = s.ctl('state').runs.find(r => r.title === 'Talk to Overseer'); if (run?.status === 'completed') break; await delay(250); }
    await s.screenshot('live-answer');
    check('the live Claude turn finished (haiku, existing login)', done && run && run.status === 'completed', { status: run?.status, model: run?.model });
    check('Overseer\'s answer names the working agent from the shared state', /Billing migration/i.test(text) && /running|working|migrat/i.test(text), text.slice(0, 800));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    if (worker) try { s.ctl('run.interrupt', { run_id: worker.run.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
