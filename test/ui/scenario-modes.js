// Packaged-UI scenario for AC-79 (grid and dashboard mode in the new layout), fixture runs only.
// From the chat-only arrangement (an agent without changes) and the review-and-chat arrangement
// (an agent with changes): the grid opens in the editor area and closing it returns to the same
// arrangement; dashboard mode hides the panel and secondary side bar, keeps the side bar on the
// Overseer agents list, and Exit returns to the same arrangement and parts.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('modes');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'modes-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const edits = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L9: original$/L9: agent edit/' a.txt"], prompt: '', title: 'With changes' });
    const quiet = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: ['nothing to change'], prompt: '', title: 'No changes' });
    for (let i = 0; i < 30 && s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)); i++) await delay(300);
    await cdp.command('View: Show Explorer'); await delay(500);
    const arrangement = () => cdp.evalWorkbench(`(() => {
      const vis = sel => { const e = document.querySelector(sel); return !!e && e.offsetWidth > 0 && e.offsetHeight > 0; };
      const groups = [...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent);
      const total = groups.reduce((n, g) => n + g.getBoundingClientRect().width, 0);
      return { groups: groups.map(g => ({ share: Math.round(g.getBoundingClientRect().width / total * 50) / 50, active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(/[,:]/)[0] })),
        sidebar: vis('.part.sidebar'), sidebarTitle: document.querySelector('.part.sidebar .title-label')?.textContent.trim() || '', panel: vis('.part.panel'), auxiliary: vis('.part.auxiliarybar') };
    })()`);
    const same = (a, b) => JSON.stringify(a.groups) === JSON.stringify(b.groups) && a.sidebar === b.sidebar && a.panel === b.panel && a.auxiliary === b.auxiliary;
    const select = async title => { await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type(title); await delay(300); await cdp.key('Enter'); await delay(2500); };
    const gridRoundTrip = async label => {
      const before = await arrangement();
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
      const dash = await cdp.webview(`document.body.dataset.mode === 'grid'`, 15000);
      const during = await arrangement();
      await s.screenshot(`grid-from-${label}`);
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
      const after = await arrangement();
      check(`${label}: the grid takes the editor area and closing it returns to the same arrangement`, during.groups.length === 1 && /^Overseer/.test(during.groups[0].active) && same(before, after), { before, during, after });
      return dash;
    };
    const dashboardRoundTrip = async label => {
      const before = await arrangement();
      await cdp.command('Overseer: Open Dashboard'); await delay(4000);
      const during = await arrangement();
      await s.screenshot(`dashboard-from-${label}`);
      await cdp.command('Overseer: Exit Dashboard'); await delay(3000);
      const after = await arrangement();
      check(`${label}: dashboard mode hides the panel and secondary side bar, keeps the side bar on Overseer, and Exit returns to the same arrangement`,
        !during.panel && !during.auxiliary && during.sidebar && /Overseer/i.test(during.sidebarTitle) && JSON.stringify(during.groups.map(g => g.active)) === JSON.stringify(before.groups.map(g => g.active)) && same(before, after), { before, during, after });
    };

    // Chat only.
    await select('No changes');
    await cdp.command('View: Toggle Terminal'); await delay(1200);
    await cdp.command('View: Show Explorer'); await delay(600);
    const chatOnly = await arrangement();
    check('starting point 1: the chat alone (one group), Explorer and terminal open', chatOnly.groups.length === 1 && chatOnly.sidebar && chatOnly.panel, chatOnly);
    await gridRoundTrip('chat only');
    await dashboardRoundTrip('chat only');

    // Review and chat.
    await select('With changes');
    await cdp.command('View: Show Explorer'); await delay(600);
    const split = await arrangement();
    check('starting point 2: review on the left, chat on the right', split.groups.length === 2 && /^Review/.test(split.groups[0].active) && /^Overseer/.test(split.groups[1].active), split);
    await gridRoundTrip('review and chat');
    await dashboardRoundTrip('review and chat');
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
