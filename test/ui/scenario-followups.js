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
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.home.sendTo': 'agent', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'overseer.grid.maxTiles': 9 });
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
      logo: /overseer-logo\\.png/.test(getComputedStyle(document.querySelector('.view-composer .hero-mark')).backgroundImage), codicon: !!document.querySelector('.view-composer .hero-mark .codicon'), placeholder: document.getElementById('task').placeholder })`);
    check("the composer says \"What's next?\" beside the Overseer mark, with \"Send off a task\" in the field", hero.title === "What's next?" && hero.mark === 'Overseer' && hero.logo && !hero.codicon && hero.placeholder === 'Send off a task', hero);
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
        // AC-154: the row spans the field's width; shortened labels keep their full text in the tooltip.
        const first = chips[0].getBoundingClientRect(), last = chips[chips.length - 1].getBoundingClientRect();
        const edges = { left: Math.round(first.left - box.left), right: Math.round(box.right - last.right) };
        const clipped = chips.filter(c => { const l = c.querySelector('.chip-label'); return l && l.scrollWidth > l.clientWidth + 1; }).map(c => ({ chip: c.dataset.chip, tooltip: c.title }));
        const audit = ${auditExpression({ root: '.view-composer' })};
        return { editor: innerWidth, composer: Math.round(box.width), inside, rows, edges, clipped, overflow: audit.overflow.length, docOverflow: document.documentElement.scrollWidth > innerWidth + 1 };
      })()`);
      widths.push({ target, ...m });
      await s.screenshot(`composer-${target}`);
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    s.note('composer widths', widths);
    check('at editor widths 360, 480, 640 and 900 px no choice sits inside the text field, at most two rows of choices, nothing overflows',
      widths.every(w => w.inside.length === 0 && w.rows >= 1 && w.rows <= 2 && w.overflow === 0 && !w.docOverflow) && Math.abs(widths[0].editor - 360) <= 30 && Math.abs(widths[3].editor - 900) <= 30, widths);
    check('the composer choices fill one row edge to edge with the field; a shortened label keeps its full text in the tooltip (AC-154)',
      widths.every(w => w.rows === 1 && Math.abs(w.edges.left) <= 2 && Math.abs(w.edges.right) <= 2 && w.clipped.every(c => c.tooltip && c.tooltip.length > 3)), widths.map(w => ({ target: w.target, rows: w.rows, edges: w.edges, clipped: w.clipped })));
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
    check('account names say the harness once (chat header, agent chip and menu, Accounts view); the Mac\'s own login reads "Mac\'s default login" (AC-235)',
      !all.some(twice) && /Mac's default login/.test(labels.chatHeader + labels.accounts.join(' ')) && !all.some(t => /existing login|Your login/.test(t)), labels);

    // AC-112: search you can see. Three agents to search among.
    for (const t of ['Alpha refactor', 'Beta docs', 'Gamma tests']) s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`${t} done`], prompt: '', title: t });
    await delay(2000);
    const agentsList = () => cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '', icon: (r.querySelector('.custom-view-tree-node-item-icon')?.className.match(/codicon-([a-z-]+)/) || [])[1] || '' })); })()`);
    const agentLabels = async () => (await agentsList()).filter(r => r.label && !/^“/.test(r.label)).map(r => r.label);
    // The field: where it sits, what it looks like.
    const field = async () => (await s.searchFrame()).eval(`(() => { const i = document.getElementById('q'), box = document.querySelector('.search-field').getBoundingClientRect();
      return { value: i.value, focused: document.activeElement === i, count: document.getElementById('count').textContent, clearShown: !document.getElementById('clear').hidden,
        input: i.tagName === 'INPUT' && i.type === 'text', placeholder: i.placeholder, border: getComputedStyle(document.querySelector('.search-field')).borderTopWidth, height: Math.round(box.height) }; })()`);
    const above = await cdp.evalWorkbench(`(() => { const heads = [...document.querySelectorAll('.part.sidebar .pane-header')].filter(h => h.offsetParent).map(h => h.textContent.trim()); return heads; })()`);
    // From the editor: click into the composer, then ⌥⌘F puts the cursor in the field.
    { const at = await s.webviewPoint(dash, '#task'); await cdp.click(at.x, at.y); await delay(300); }
    await cdp.key('f', { meta: true, alt: true });
    const openedFromEditor = await s.searchFocused().then(() => true, () => false);
    await cdp.call('Input.insertText', { text: 'beta' }, cdp.workbench);
    const t0 = Date.now(); let shown = [];
    for (let i = 0; i < 200; i++) { shown = await agentLabels(); if (shown.includes('Beta docs') && !shown.includes('Alpha refactor')) break; await delay(5); }
    const ms = Date.now() - t0;
    await delay(300);
    const typed = await field();
    await s.screenshot('search-field');
    check('the side bar has a search field above the Agents list; ⌥⌘F from the editor puts the cursor in it; typing filters the list within 200 ms and the field says how many match',
      openedFromEditor && typed.input && typed.placeholder === 'Search agents' && parseFloat(typed.border) >= 1 && typed.height <= 32 && typed.value === 'beta' && typed.count === '1 match' && typed.clearShown &&
      shown.includes('Beta docs') && !shown.includes('Alpha refactor') && ms < 200 && /Search/i.test(above[0] || '') && /Agents/.test(above[1] || ''), { openedFromEditor, typed, ms, shown, panes: above });
    // Escape in the field clears it.
    await cdp.key('Escape'); await delay(800);
    const afterEsc = await field(); shown = await agentLabels();
    const clearedByKey = afterEsc.value === '' && shown.includes('Alpha refactor') && shown.includes('Beta docs');
    // From the side bar: ⌥⌘F, type, then clear with the field's ✕.
    await cdp.command('Focus on Agents View'); await delay(400);
    await cdp.key('f', { meta: true, alt: true });
    const openedFromSide = await s.searchFocused().then(() => true, () => false);
    await cdp.call('Input.insertText', { text: 'gamma' }, cdp.workbench); await delay(700);
    const narrowed = (await agentLabels()).includes('Gamma tests') && !(await agentLabels()).includes('Alpha refactor');
    const sf = await s.searchFrame();
    { const at = await s.webviewPoint(sf, '#clear'); await cdp.click(at.x, at.y); await delay(800); }
    const afterX = await field(); shown = await agentLabels();
    const clearedByMouse = afterX.value === '' && shown.includes('Alpha refactor');
    check('⌥⌘F also works from the side bar; Escape and the field\'s ✕ each clear the search and bring the list back', openedFromSide && narrowed && clearedByKey && clearedByMouse, { openedFromSide, narrowed, clearedByKey, clearedByMouse });

    // The space under the field holds the status filters (VS Code gives a webview pane a minimum height).
    // Filters live behind the field's filter icon (a VS Code menu).
    const LABEL = { all: 'All', working: 'Working', needs: 'Needs you', done: 'Done', failed: 'Failed', archived: 'Archived' };
    const clickFilter = async value => { const f = await s.searchFrame(); const at = await s.webviewPoint(f, '#filter'); await cdp.click(at.x, at.y);
      await cdp.waitQuickTitle('Show agents'); await cdp.type(LABEL[value]); await delay(300); await cdp.key('Enter'); await delay(900); return f; };
    // AC-155: one line; nothing under the field.
    const oneLine = await (await s.searchFrame()).eval(`({ children: [...document.body.children].filter(e => e.tagName !== 'SCRIPT' && e.offsetParent !== null).length, fieldHeight: Math.round(document.querySelector('.search-field').getBoundingClientRect().height), filterIcon: !!document.querySelector('#filter .codicon-filter') })`);
    const pane = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.sidebar .pane')].map(p => ({ head: p.querySelector('.pane-header')?.textContent.trim().slice(0, 12), h: Math.round(p.getBoundingClientRect().height) }))[0]`);
    s.ctl('task.archive', { task_id: state().tasks.find(t => t.title === 'Alpha refactor').id, archived: true }); await delay(800);
    const running = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 60'], prompt: '', title: 'Still working' }); loops.push(running.run.id); await delay(1500);
    let ff = await clickFilter('working'); const onlyWorking = await agentLabels(); const workingCount = await ff.eval(`window.__overseerSearch.count()`);
    await s.screenshot('filter-working');
    ff = await clickFilter('done'); const onlyDone = await agentLabels();
    ff = await clickFilter('archived'); const onlyArchived = await agentLabels();
    ff = await clickFilter('all'); const everything = await agentLabels();
    check('the status filters under the field narrow the list (Working, Done, Archived) and All brings it back; the field says how many are shown',
      onlyWorking.includes('Still working') && !onlyWorking.includes('Beta docs') && workingCount === '1 agent' && onlyDone.includes('Beta docs') && !onlyDone.includes('Still working') &&
      onlyArchived.includes('Alpha refactor') && !onlyArchived.includes('Beta docs') && everything.includes('Beta docs') && everything.includes('Still working') && !everything.includes('Alpha refactor'),
      { onlyWorking, workingCount, onlyDone, onlyArchived, everything });
    const iconOn = await (await s.searchFrame()).eval(`document.querySelector('#filter').classList.contains('on')`);
    check('the search is one line with a filter icon (its menu picks the filter; the icon shows when one is on) and nothing under the field (AC-155); the pane keeps VS Code\'s minimum height for extension panes',
      oneLine.children === 1 && oneLine.fieldHeight <= 30 && oneLine.filterIcon && workingCount === '1 agent' && iconOn === false, { oneLine, pane, iconOnAfterAll: iconOn });
    await clickFilter('working'); await s.screenshot('filter-on');
    await clickFilter('all');
    await theme('Overseer Light'); await s.screenshot('search-field-light'); await theme('Overseer Dark');
    s.ctl('run.interrupt', { run_id: running.run.id }); await delay(1500);

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
      await cdp.command('Overseer: Enter Focus Mode'); await delay(3500);
      await s.screenshot('dashboard-nine-' + t.split(' ')[1].toLowerCase());
      await cdp.command('Overseer: Exit Focus Mode'); await delay(2000);
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
