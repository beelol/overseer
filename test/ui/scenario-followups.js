// Packaged-UI scenario for the Gate K follow-ups from the owner's marks (AC-109 to AC-113), fixture
// harnesses only: the composer's heading and mark (AC-111) in three themes; its choices sit under the
// field at editor widths 360, 480, 640 and 900 px (AC-109); account names say the harness once on every
// surface (AC-110); a side-bar search is visible as the list's first row, started by shortcut from the
// editor and from the side bar and cleared by Escape or by mouse (AC-112); the grid never shows an empty
// screen, and the grid and dashboard mode are captured with nine working agents (AC-113).
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

(async () => {
  const s = new Session('followups');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  const login = (who, argv) => { fs.writeFileSync(next, who); cp.execFileSync(cli, argv, { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } }); };
  login('desk:pro', ['login']); login('deskclaude:max', ['auth', 'login']);
  const loops = [];
  try {
    const repo = makeRepo(path.join(s.root, 'followups-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'overseer.grid.maxTiles': 9 });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const theme = async name => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = name; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1800); };
    const state = () => s.ctl('state');
    const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];

    // AC-111: heading, mark and placeholder, in three themes.
    await cdp.command('Overseer: New Agent'); await delay(1500);
    const dash = await s.editorView(`document.body.dataset.mode === 'composer' && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`);
    const hero = await dash.eval(`({ title: document.querySelector('.view-composer .hero-title')?.textContent, mark: document.querySelector('.view-composer .hero-mark')?.getAttribute('aria-label'),
      svg: !!document.querySelector('.view-composer .hero-mark svg'), codicon: !!document.querySelector('.view-composer .hero-mark .codicon'), placeholder: document.getElementById('task').placeholder })`);
    check("the composer says \"What's next?\" beside the Overseer mark, with \"Send off a task\" in the field", hero.title === "What's next?" && hero.mark === 'Overseer' && hero.svg && !hero.codicon && hero.placeholder === 'Send off a task', hero);
    for (const t of ['Overseer Dark', 'Overseer Light', 'Default High Contrast']) { await theme(t); await s.screenshot('composer-' + t.toLowerCase().replace(/ /g, '-')); }
    await theme('Overseer Dark');

    // AC-109: the choices sit under the field at editor widths 360, 480, 640 and 900 px.
    await cdp.command('View: Close Primary Side Bar'); await delay(600);
    const widths = [];
    for (const target of [360, 480, 640, 900]) {
      // The editor area is the window minus the activity bar; size the window so the editor is about `target` wide.
      const chrome = await cdp.evalWorkbench(`innerWidth - document.querySelector('.part.editor').getBoundingClientRect().width`);
      await cdp.call('Emulation.setDeviceMetricsOverride', { width: Math.round(target + chrome), height: 900, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1200);
      const m = await dash.eval(`(() => {
        const box = document.querySelector('.view-composer .composer.big').getBoundingClientRect();
        const chips = [...document.querySelectorAll('.view-composer [data-chip]')].filter(c => !c.hidden && c.offsetParent);
        const inside = chips.filter(c => { const r = c.getBoundingClientRect(); return r.top < box.bottom && r.bottom > box.top; }).map(c => c.dataset.chip);
        const rows = new Set(chips.map(c => Math.round(c.getBoundingClientRect().top))).size;
        const audit = ${auditExpression({ root: '.view-composer' })};
        return { editor: innerWidth, composer: Math.round(box.width), inside, rows, overflow: audit.overflow.length, docOverflow: document.documentElement.scrollWidth > innerWidth + 1 };
      })()`);
      widths.push({ target, ...m });
      await s.screenshot(`composer-${target}`);
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    s.note('composer widths', widths);
    check('at editor widths 360, 480, 640 and 900 px no choice sits inside the text field, at most two rows of choices, nothing overflows',
      widths.every(w => w.inside.length === 0 && w.rows >= 1 && w.rows <= 2 && w.overflow === 0 && !w.docOverflow) && Math.abs(widths[0].editor - 360) <= 30 && Math.abs(widths[3].editor - 900) <= 30, widths);
    await cdp.command('View: Show Overseer'); await delay(800);

    // AC-110: account names say the harness once. Start a Codex agent on the machine's own login.
    const codex = state().profiles.find(p => p.harness === 'codex' && p.is_system);
    const run = s.ctl('task.create', { repo, harness: 'codex', profile_id: codex.id, title: 'Name check', prompt: 'say hi' });
    for (let i = 0; i < 40 && ACTIVE.includes(state().runs.find(r => r.id === run.run.id)?.status); i++) await delay(300);
    await s.selectAgent('Name check', { settle: 2500 });
    const chat = await s.editorView(`document.getElementById('title')?.textContent === 'Name check'`);
    const labels = {};
    labels.chatHeader = await chat.eval(`document.querySelector('.chat-meta')?.innerText || ''`);
    await cdp.command('Overseer: New Agent'); await delay(1200);
    labels.agentChip = await dash.eval(`document.querySelector('[data-chip="agent"]').innerText`);
    await dash.eval(`document.querySelector('[data-chip="agent"]').click()`); await delay(400);
    labels.agentMenu = await dash.eval(`[...document.querySelectorAll('.menu .menu-item')].map(b => b.innerText.replace(/\\s+/g, ' ').trim())`);
    await cdp.key('Escape');
    labels.accounts = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Accounts/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); return pane ? [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => r.innerText.replace(/\\s+/g, ' ').trim()) : []; })()`);
    const twice = t => ['codex', 'claude'].some(h => (String(t).toLowerCase().match(new RegExp(h, 'g')) || []).length > 1);
    const all = [labels.chatHeader, labels.agentChip, ...labels.agentMenu, ...labels.accounts];
    await s.screenshot('names');
    check('account names say the harness once (chat header, agent chip and menu, Accounts view); the machine\'s own login reads "Your login"',
      !all.some(twice) && /Your login/.test(labels.agentChip + labels.accounts.join(' ')) && !all.some(t => /existing login/.test(t)), labels);

    // AC-112: search you can see. Three agents to search among.
    for (const t of ['Alpha refactor', 'Beta docs', 'Gamma tests']) s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`${t} done`], prompt: '', title: t });
    await delay(2000);
    const agentsList = () => cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '', icon: (r.querySelector('.custom-view-tree-node-item-icon')?.className.match(/codicon-([a-z-]+)/) || [])[1] || '' })); })()`);
    // From the editor: click into the composer, then ⌥⌘F.
    { const at = await s.webviewPoint(dash, '#task'); await cdp.click(at.x, at.y); await delay(300); }
    await cdp.key('f', { meta: true, alt: true }); await delay(500);
    const openedFromEditor = await cdp.waitQuickTitle('Search agents').then(() => true, () => false);
    await cdp.call('Input.insertText', { text: 'beta' }, cdp.workbench);
    const t0 = Date.now(); let rows = [];
    for (let i = 0; i < 200; i++) { rows = await agentsList(); if (rows[0]?.label === '“beta”') break; await delay(5); }
    const ms = Date.now() - t0;
    await cdp.key('Enter'); await delay(400);
    rows = await agentsList();
    await s.screenshot('search-row');
    check('⌥⌘F from the editor starts a search; the query is the first row of the Agents list with its match count, within 200 ms',
      openedFromEditor && rows[0]?.label === '“beta”' && /^1 match$/.test(rows[0]?.description) && rows[0]?.icon === 'search' && rows.some(r => r.label === 'Beta docs') && !rows.some(r => r.label === 'Alpha refactor') && ms < 200, { openedFromEditor, first: rows[0], ms, labels: rows.map(r => r.label) });
    // Escape in the Agents view clears it.
    await cdp.command('Focus on Agents View'); await delay(400);
    await cdp.key('Escape'); await delay(800);
    rows = await agentsList();
    const clearedByKey = rows[0]?.label !== '“beta”' && rows.some(r => r.label === 'Alpha refactor');
    // From the side bar: ⌥⌘F, search, then clear by mouse with the row's ✕.
    await cdp.command('Focus on Agents View'); await delay(400);
    await cdp.key('f', { meta: true, alt: true }); await delay(500);
    const openedFromSide = await cdp.waitQuickTitle('Search agents').then(() => true, () => false);
    await cdp.call('Input.insertText', { text: 'gamma' }, cdp.workbench); await delay(600); await cdp.key('Enter'); await delay(500);
    const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === '“gamma”'); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 5000, 'search row');
    await cdp.move(pt.x, pt.y); await delay(500);
    const x = await cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === '“gamma”'); const a = [...r.querySelectorAll('.actions .action-label')].find(a => /^Clear Search/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + 8, y: b.top + 8 }; })()`);
    if (x) { await cdp.click(x.x, x.y); await delay(800); }
    rows = await agentsList();
    const clearedByMouse = !!x && rows[0]?.label !== '“gamma”' && rows.some(r => r.label === 'Alpha refactor');
    check('⌥⌘F also works from the side bar; Escape and the row\'s ✕ each clear the search and bring the list back', openedFromSide && clearedByKey && clearedByMouse, { openedFromSide, clearedByKey, clearedByMouse });

    // AC-113: no empty grid. Nothing working and nothing pinned: the grid command goes home with a note.
    for (let i = 0; i < 40 && state().runs.some(r => ACTIVE.includes(r.status)); i++) await delay(300);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);
    const home = await dash.eval(`({ mode: document.body.dataset.mode, note: document.querySelector('.view-composer .composer-note')?.innerText || '', emptyText: /No agents running/.test(document.body.innerText) })`);
    await s.screenshot('grid-with-nothing');
    check('with no agent working or pinned, the grid command goes to the home composer with a one-line note (no empty grid screen)', home.mode === 'composer' && /grid has nothing to show/.test(home.note) && !home.emptyText, home);
    // One working agent: the grid shows it; when it stops (its tile goes), the grid gives way to home.
    const one = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'i=0; while [ $i -lt 120 ]; do echo "working $i"; i=$((i+1)); sleep 0.5; done'], prompt: '', title: 'Lone worker' });
    loops.push(one.run.id);
    for (let i = 0; i < 30 && state().runs.find(r => r.id === one.run.id)?.status !== 'running'; i++) await delay(300);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    const grid = await cdp.webview(`document.body.dataset.mode === 'grid' && !!document.querySelector('.grid .tile')`, 15000);
    s.ctl('run.interrupt', { run_id: one.run.id });
    const back = await grid.waitFor(`document.body.dataset.mode === 'composer'`, 15000).then(() => true, () => false);
    const backNote = await grid.eval(`document.querySelector('.view-composer .composer-note')?.innerText || ''`);
    check('when the last tile goes, the grid gives way to the home composer', back && /grid is empty/.test(backNote), { back, backNote });

    // Nine working agents: the grid and dashboard mode, in both themes.
    for (let i = 1; i <= 9; i++) {
      const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', `i=0; while [ $i -lt 400 ]; do echo "step $i of task ${i}: checking module ${i}.$i"; i=$((i+1)); sleep 0.4; done`], prompt: '', title: ['Split payments', 'Refresh sessions', 'Migrate users', 'Fix flaky test', 'Upgrade deps', 'Docs pass', 'Cache warmup', 'Audit logs', 'Retry policy'][i - 1] });
      loops.push(t.run.id);
    }
    for (let i = 0; i < 40 && state().runs.filter(r => loops.includes(r.id) && r.status === 'running').length < 9; i++) await delay(300);
    await delay(3000);
    await cdp.command('View: Show Overseer'); await delay(500);
    const counts = [];
    for (const t of ['Overseer Dark', 'Overseer Light']) {
      await theme(t);
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(3500);
      counts.push(await cdp.webview(`document.body.dataset.mode === 'grid'`, 10000).then(v => v.eval(`document.querySelectorAll('.grid .tile').length`)).catch(() => 0));
      await s.screenshot('grid-nine-' + t.split(' ')[1].toLowerCase());
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);
      await cdp.command('Overseer: Open Dashboard'); await delay(3500);
      await s.screenshot('dashboard-nine-' + t.split(' ')[1].toLowerCase());
      await cdp.command('Overseer: Exit Dashboard'); await delay(2000);
    }
    await theme('Overseer Dark');
    check('the grid and dashboard mode are captured with nine working agents in both Overseer themes (nine tiles each time)', counts.length === 2 && counts.every(n => n === 9), { counts });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    for (const id of loops) { try { s.ctl('run.interrupt', { run_id: id }); } catch {} }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
