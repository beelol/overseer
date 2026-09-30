// Packaged-UI scenario for AC-235: you can always see which account an agent uses. Two Claude
// fixture agents (SYNTHETIC accounts, no real login is read): one on the Mac's default login
// (bilal@testbox.com, Max), one on a named account "Personal" (ana.silva@personal.example, Pro).
// The side bar's rows, each agent's header, the grid's tiles and the composer's agent choice name
// the provider, the plan and the email with its local part shortened; the default login is
// "Mac's default login"; no surface says "Your login". Screenshots of each.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const MAC = 'Claude Max · bil…@testbox.com';
const OWN = 'Claude Pro · ana…@personal.example';

(async () => {
  const s = new Session('account-shown');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  // The Mac's default Claude login, as the fixture reports it for its folder.
  const sys = path.join(s.root, 'desktop-home');
  fs.mkdirSync(path.join(sys, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(sys, '.claude/fixture-account.json'), JSON.stringify({ email: 'bilal@testbox.com', plan: 'max' }));
  try {
    const repo = makeRepo(path.join(s.root, 'shop'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.home.sendTo': 'agent' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      OVERSEER_TEST_SYSTEM_HOME: sys, CLAUDE_FIXTURE_MODE: 'permission', OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');

    // A named account with its own login.
    const personal = s.ctl('account.create', { provider: 'anthropic', name: 'Personal' }).account;
    fs.writeFileSync(path.join(personal.home, 'claude', 'fixture-account.json'), JSON.stringify({ email: 'ana.silva@personal.example', plan: 'pro' }));
    const mac = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', prompt: 'write perm.txt', title: 'Tidy the login page' }).run.id;
    const own = s.ctl('task.create', { repo, harness: 'claude', profile_id: personal.id, prompt: 'write perm.txt', title: 'Write the release notes' }).run.id;
    for (let i = 0; i < 60 && s.ctl('state').runs.filter(r => [mac, own].includes(r.id) && r.status === 'waiting_for_user').length < 2; i++) await delay(300);

    // VS Code reads each account's login (as it does when it starts and on Refresh).
    await cdp.command('Overseer: Refresh Account Status');
    let labels = {};
    for (let i = 0; i < 40; i++) {
      const st = s.ctl('state');
      labels = Object.fromEntries(st.profiles.map(p => [p.id, p.account && p.account.label]));
      if (/@testbox\.com/.test(labels['system-claude'] || '') && /@personal\.example/.test(labels[personal.id] || '')) break;
      await delay(250);
    }
    check('the daemon names both accounts: provider and plan, shortened email, whose login it is',
      labels['system-claude'] === `${MAC} · Mac's default login` && labels[personal.id] === `${OWN} · Personal`, labels);
    const full = JSON.stringify(s.ctl('state'));
    check('the full email address never leaves the daemon', !full.includes('bilal@testbox.com') && !full.includes('ana.silva@personal.example'));

    // The side bar: each agent's row names its account.
    await s.openOverseerView();
    let rows = [];
    for (let i = 0; i < 30; i++) {
      rows = await s.agentRows();
      const find = t => rows.find(r => r.label === t && r.level >= 2);
      if ((find('Tidy the login page')?.description || '').includes('Max · bil…@testbox.com') && (find('Write the release notes')?.description || '').includes('Pro · ana…@personal.example')) break;
      await delay(300);
    }
    const row = t => rows.filter(r => r.label === t).pop() || {};
    check('the side bar: the default login\'s agent row shows its plan and email domain', row('Tidy the login page').description.includes('Max · bil…@testbox.com') && /Mac's default login/.test(row('Tidy the login page').aria || ''), row('Tidy the login page'));
    check('the side bar: the named account\'s agent row shows its plan and email domain', row('Write the release notes').description.includes('Pro · ana…@personal.example') && /Personal/.test(row('Write the release notes').aria || ''), row('Write the release notes'));
    // A wider side bar, as an owner drags it, so the whole account shows in the screenshot (a
    // narrow one cuts it; the row's hover and accessible name always hold it all).
    const edge = await cdp.evalWorkbench(`(() => { const r = document.querySelector('.part.sidebar').getBoundingClientRect(); return { x: r.right, y: r.top + r.height / 2 }; })()`);
    await cdp.drag({ x: edge.x + 1, y: edge.y }, { x: edge.x + 240, y: edge.y }); await delay(800);
    await s.screenshot('side-bar');

    // Each agent's header.
    const header = async (title, shot) => {
      await s.selectAgent(title, { settle: 1500 });
      const chat = await s.editorView(`document.getElementById('title')?.textContent === ${JSON.stringify(title)}`);
      await chat.waitFor(`!!document.querySelector('.chat-meta .meta-account')`, 10000);
      const text = await chat.eval(`document.querySelector('.chat-meta .meta-account').innerText.trim()`);
      const details = await chat.eval(`(() => { const d = [...document.querySelectorAll('dl dt')].find(x => x.textContent === 'Account'); return d ? d.nextElementSibling.textContent : ''; })()`);
      await s.screenshot(shot);
      return { text, details };
    };
    const h1 = await header('Tidy the login page', 'header-default-login');
    check('the header of an agent on the default login: provider, plan, email domain, "Mac\'s default login"', h1.text === `${MAC} · Mac's default login`, h1);
    const h2 = await header('Write the release notes', 'header-named-account');
    check('the header of an agent on a named account: provider, plan, email domain and its name', h2.text === `${OWN} · Personal`, h2);

    // The grid: every tile names its account.
    await cdp.command('Overseer: Toggle Agent Grid');
    const dash = await s.editorView();
    await dash.waitFor(`document.querySelectorAll('.grid .tile').length >= 2`, 20000);
    const tiles = await dash.waitFor(`(() => { const t = [...document.querySelectorAll('.grid .tile')].map(x => ({ title: x.querySelector('.tile-title')?.textContent, account: x.querySelector('.tile-account')?.textContent || '', who: x.querySelector('.tile-who')?.title || '' })); return t.some(x => x.account.includes('@')) && t; })()`, 15000);
    check('the grid: each tile names its account', tiles.some(t => t.title === 'Tidy the login page' && t.account === MAC) && tiles.some(t => t.title === 'Write the release notes' && t.account === OWN), tiles);
    await s.screenshot('grid');
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(800);

    // The composer: the agent choice names the account it will run on.
    await cdp.command('Overseer: New Agent'); await delay(800);
    const comp = await s.editorView(`document.body.dataset.mode === 'composer' && !!document.querySelector('.view-composer:not([hidden]) #task')`);
    await comp.waitFor(`!document.querySelector('[data-chip="repo"]').textContent.includes('Loading') && /@/.test(document.querySelector('[data-chip="agent"]').textContent)`, 20000);
    const chip = () => comp.eval(`(() => { const c = document.querySelector('[data-chip="agent"]'); const r = document.getElementById('composer-account'); return { text: c.innerText.trim(), title: c.title, runsOn: r && !r.hidden ? r.textContent : '', whole: r ? r.scrollWidth <= r.clientWidth : false }; })()`);
    const pick = async label => {
      await comp.eval(`document.querySelector('[data-chip="agent"]').click()`); await delay(400);
      const items = await comp.eval(`[...document.querySelectorAll('.menu .menu-item')].map(b => b.innerText.replace(/\\s+/g, ' ').trim())`);
      // The item under Claude Code's heading (Codex and OpenCode have a "Mac's default login" too).
      await comp.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => { let h = b.previousElementSibling; while (h && !h.classList.contains('menu-head')) h = h.previousElementSibling; return b.querySelector('.menu-label')?.textContent === ${JSON.stringify(label)} && /Claude/.test(h?.textContent || ''); }).click()`); await delay(500);
      return items;
    };
    const menu = await pick("Mac's default login");
    const c1 = await chip();
    check('the composer on the default login: the agent choice shows the plan and email domain', c1.text === 'Claude Code · Max · bil…@testbox.com' && c1.title.includes(`${MAC} · Mac's default login`) && c1.runsOn === `Runs on ${MAC} · Mac's default login` && c1.whole, c1);
    check('the composer\'s account menu names each account with its plan and email', menu.some(m => m.includes("Mac's default login") && m.includes(MAC)) && menu.some(m => m.includes('Personal') && m.includes(OWN)), menu);
    await s.screenshot('composer-default-login');
    await pick('Personal');
    const c2 = await chip();
    check('the composer on a named account: the agent choice shows the plan and email domain', c2.text === 'Claude Code · Pro · ana…@personal.example' && c2.title.includes(`${OWN} · Personal`) && c2.runsOn === `Runs on ${OWN} · Personal` && c2.whole, c2);
    await s.screenshot('composer-named-account');

    // The Accounts view says the same.
    const accounts = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Accounts/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); return pane ? [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => r.innerText.replace(/\\s+/g, ' ').trim()) : []; })()`);
    check('the Accounts view: the default login and the named account with plan and email', accounts.some(a => /Mac's default login/.test(a) && /Max · bil…@testbox\.com/.test(a)) && accounts.some(a => /Personal/.test(a) && /Pro · ana…@personal\.example/.test(a)), accounts);

    // Nothing says "Your login", in the workbench or in any Overseer view.
    const texts = [await cdp.evalWorkbench(`document.body.innerText`), ...(await Promise.all([comp].map(f => f.eval(`document.body.innerText`))))];
    check('no surface says "Your login"', !texts.some(t => /Your login|existing login/.test(t)), texts.map(t => (t.match(/.{0,40}(Your login|existing login).{0,40}/) || [''])[0]));
    for (const id of [mac, own]) { try { s.ctl('run.interrupt', { run_id: id }); } catch {} }
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
