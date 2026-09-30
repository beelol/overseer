// Packaged-UI scenario for AC-150 (fixture runs only): the first click always lands. In each
// arrangement (composer alone, review beside the chat, the grid) keyboard focus is
// first put somewhere else (the side bar, or another editor group), then ONE click goes to a view's
// first control, and the scenario checks that the click did its job: the composer's agent menu opens,
// the chat's More menu opens, the review's Changes only toggle flips, a grid tile's pin toggles, and
// the Agents view's search button opens the search box, which takes the typing that follows.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('first-click');
  const result = { checks: [], clicks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'click-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const edit = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L2: original$/L2: clicked/' a.txt; echo done"], prompt: '', title: 'Click target' });
    s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo busy; sleep 600'], prompt: '', title: 'Busy agent' });
    await delay(2500);
    await s.openOverseerView(); await delay(800);

    // Where focus goes first: the side bar (its Accounts header) or another editor group.
    const focusSideBar = async () => {
      const p = await cdp.evalWorkbench(`(() => { const h = [...document.querySelectorAll('.part.sidebar .pane-header')].find(h => /Accounts/.test(h.textContent)); if (!h) return null; const r = h.getBoundingClientRect(); return { x: r.left + r.width - 60, y: r.top + r.height / 2 }; })()`);
      if (p) { await cdp.click(p.x, p.y); await delay(400); }
      return cdp.evalWorkbench(`!!document.activeElement?.closest('.part.sidebar')`);
    };
    const focusOther = async frame => { const p = await s.webviewPoint(frame, 'body'); await cdp.click(p.x, p.y); await delay(400); return true; };
    const once = async (label, frame, selector, before, after) => {
      if (before) await frame.eval(before);
      const p = await s.webviewPoint(frame, selector);
      await cdp.click(p.x, p.y);
      const ok = await frame.waitFor(after, 2500).then(() => true, () => false);
      result.clicks.push({ label, ok });
      return ok;
    };

    // Composer alone: focus in the side bar, one click on the agent chip opens its menu.
    await cdp.command('Overseer: New Agent'); await delay(1500);
    const dash = await s.editorView(`document.body.dataset.mode === 'composer'`);
    await dash.waitFor(`!!document.querySelector('[data-chip="agent"]')`, 10000);
    const sb1 = await focusSideBar();
    check('composer: after focus in the side bar, one click on the agent choice opens its menu', sb1 && await once('composer agent chip', dash, '[data-chip="agent"]', `document.querySelector('.menu')?.remove()`, `!!document.querySelector('.menu')`), result.clicks.at(-1));
    await cdp.key('Escape'); await delay(300);

    // Review beside the chat.
    await s.selectAgent('Click target', { settle: 3000 });
    const review = await cdp.webview(`!!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 20000);
    const chat = await s.editorView(`!!document.querySelector('.view-chat') && !document.querySelector('.view-chat').hidden`);
    await focusOther(review);
    check('chat: after focus in the review (another editor group), one click on More opens its menu', await once('chat more', chat, '#more', `document.querySelector('.menu')?.remove()`, `!!document.querySelector('.menu')`), result.clicks.at(-1));
    await cdp.key('Escape'); await delay(300);
    await focusSideBar();
    const pressed = await review.eval(`document.getElementById('changes-only').getAttribute('aria-pressed')`);
    check('review: after focus in the side bar, one click on Changes only flips it', await once('review toggle', review, '#changes-only', '', `document.getElementById('changes-only').getAttribute('aria-pressed') !== ${JSON.stringify(pressed)}`), result.clicks.at(-1));
    await focusOther(chat);
    const pressed2 = await review.eval(`document.getElementById('changes-only').getAttribute('aria-pressed')`);
    check('review: after focus in the chat (another editor group), one click flips it back', await once('review toggle back', review, '#changes-only', '', `document.getElementById('changes-only').getAttribute('aria-pressed') !== ${JSON.stringify(pressed2)}`), result.clicks.at(-1));

    // Searching agents (AC-264: no Search section): after focus in the chat, ONE click on the Agents
    // view's search button opens VS Code's input box with the cursor in it, and typing filters.
    await focusOther(chat);
    // The title bar's buttons show while the pointer is over the Agents header.
    const agentsPane = sel => cdp.evalWorkbench(`(() => { const p = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); const a = p?.querySelector(${JSON.stringify(sel)}); if (!a) return null; const r = a.getBoundingClientRect(); return r.width ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : null; })()`);
    const header = await agentsPane('.pane-header .title');
    if (header) { await cdp.move(header.x, header.y); await delay(400); }
    const searchButton = await agentsPane('.pane-header .action-label[aria-label^="Search Agents"]');
    let focused = false;
    if (searchButton) {
      await cdp.move(searchButton.x, searchButton.y); await delay(200);
      await cdp.click(searchButton.x, searchButton.y);
      focused = await s.searchFocused(2500).then(() => true, () => false);
    }
    result.clicks.push({ label: 'search button', ok: focused });
    await cdp.type('Busy'); await delay(600);
    const typed = await cdp.evalWorkbench(`document.querySelector('.quick-input-widget input')?.value || ''`);
    check('search: after focus in the chat, one click on the Agents view\'s search button puts the cursor in the search box and typing filters', focused && typed === 'Busy', { searchButton, focused, typed });
    await cdp.key('Escape'); await delay(400);

    // The grid: a tile's pin.
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
    const grid = await s.editorView(`document.body.dataset.mode === 'grid' && document.querySelectorAll('.grid .tile').length > 0`);
    await focusSideBar();
    const pin = await grid.eval(`document.querySelector('.grid .tile .tile-pin').getAttribute('aria-pressed')`);
    check('grid: after focus in the side bar, one click on a tile\'s pin toggles it', await once('grid pin', grid, '.grid .tile .tile-pin', '', `document.querySelector('.grid .tile .tile-pin')?.getAttribute('aria-pressed') !== ${JSON.stringify(pin)}`), result.clicks.at(-1));
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);

    await s.screenshot('after');
    s.ctl('run.interrupt', { run_id: s.ctl('state').runs.find(r => r.title === 'Busy agent').id });
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
