// Packaged-UI scenario for AC-80 (remembered place), fixture runs only. An agent with changes and a
// long conversation is shown with its review (review left, chat right); the review scope is set to
// Unstaged, Follow is turned on, and both the review and the chat are scrolled. After a window
// reload, and again after quitting VS Code and starting it again, Overseer reopens the same agent
// with the same arrangement, review scope and scroll positions, and in follow mode (which comes
// back paused until you resume it, as AC-49 requires).
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('place');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const env = { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' };
  try {
    const repo = makeRepo(path.join(s.root, 'place-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, env);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const lines = [10, 40, 70, 100, 130, 160, 190, 220, 250, 280];
    const script = `sed -i '' ${lines.map(n => `-e 's/^L${n}: original$/L${n}: agent edit/'`).join(' ')} a.txt b.txt; i=1; while [ $i -le 160 ]; do echo "progress line $i of the long conversation"; i=$((i+1)); done`;
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title: 'Place demo' });
    for (let i = 0; i < 40 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);
    await s.selectAgent('Place demo', { settle: 3000 });
    let review = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(t.run.id)}`, 30000);
    let chat = await cdp.webview(`document.getElementById('title')?.textContent === 'Place demo'`, 30000);
    // Scope Unstaged, Follow on, then scroll both.
    await review.eval(`(() => { const s = document.getElementById('scope'); s.value = 'unstaged'; s.dispatchEvent(new Event('change')); return true; })()`);
    await review.waitFor(`document.body.dataset.scope === 'unstaged' && document.querySelectorAll('#tree .file').length >= 2`, 15000);
    const icon = await s.webviewPoint(review, '#follow'); await cdp.click(icon.x, icon.y);
    await review.waitFor(`document.getElementById('follow').dataset.state === 'following'`, 10000);
    await delay(1500);
    await review.eval(`document.getElementById('diffs').scrollTop = 700`); await delay(300);
    await review.eval(`document.getElementById('diffs').dispatchEvent(new Event('scroll'))`);
    await chat.eval(`(() => { const sc = document.getElementById('scroll'); sc.scrollTop = Math.round(sc.scrollHeight * 0.4); sc.dispatchEvent(new Event('scroll')); return true; })()`);
    await delay(2000);
    const snapshot = async () => {
      const layout = await cdp.evalWorkbench(`(() => { const groups = [...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent); const total = groups.reduce((n, g) => n + g.getBoundingClientRect().width, 0);
        return groups.map(g => ({ share: Math.round(g.getBoundingClientRect().width / total * 50) / 50, active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(/[,:]/)[0] })); })()`);
      const r = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(t.run.id)} && document.querySelectorAll('#tree .file').length >= 2`, 40000);
      const c = await cdp.webview(`document.getElementById('title')?.textContent === 'Place demo' && document.querySelectorAll('#conv .msg').length > 20`, 40000);
      await delay(2500);
      return { layout, agent: await c.eval(`document.getElementById('title').textContent`), scope: await r.eval(`document.getElementById('scope').value`),
        follow: await r.eval(`document.getElementById('follow').dataset.state`), reviewTop: await r.eval(`Math.round(document.getElementById('diffs').scrollTop)`),
        chatTop: await c.eval(`Math.round(document.getElementById('scroll').scrollTop)`) };
    };
    const before = await snapshot();
    s.note('before', before);
    await s.screenshot('before');
    const compare = (label, after) => {
      check(`${label}: the same agent and arrangement (review left, chat right)`, after.agent === before.agent && JSON.stringify(after.layout) === JSON.stringify(before.layout), { before: before.layout, after: after.layout });
      check(`${label}: the same review scope`, after.scope === before.scope, { before: before.scope, after: after.scope });
      check(`${label}: still in follow mode (paused until resumed, as AC-49 requires)`, before.follow === 'following' && after.follow === 'paused', { before: before.follow, after: after.follow });
      check(`${label}: review and chat scroll positions restored`, Math.abs(after.reviewTop - before.reviewTop) <= 80 && Math.abs(after.chatTop - before.chatTop) <= 80, { review: [before.reviewTop, after.reviewTop], chat: [before.chatTop, after.chatTop] });
    };

    // Reload.
    await cdp.command('Developer: Reload Window'); await delay(7000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'after reload');
    const afterReload = await snapshot();
    await s.screenshot('after-reload');
    compare('after a reload', afterReload);

    // Quit and start VS Code again (the daemon keeps running).
    await s.quit(); await delay(1500);
    s.launch(repo, env);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'after restart');
    const afterRestart = await snapshot();
    await s.screenshot('after-restart');
    compare('after quitting and starting VS Code again', afterRestart);
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
