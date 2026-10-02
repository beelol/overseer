// Packaged-UI scenario for AC-239 (stuck, failed and limited agents come back to Overseer), with
// the Claude fixture as Overseer's model and as the agent (no paid turn):
//   the owner asks on home for an agent; it hits its account's usage limit (the fixture's
//   `ratelimit` mode: "[rate_limit]", "API Error", "(429)"). Home shows Overseer's card with the
//   plain reason and the request's stage line says it the same way; Overseer proposes continuing
//   on the second account. The agent's chat (its error, its status line's tooltip, its details),
//   the side bar's tooltip and home show no raw error class or HTTP code. The owner's Yes on home
//   continues the work on the second fixture profile in the same worktree. Screenshots in the
//   three Overseer themes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

// What must never reach the owner from a limited agent (AC-239's Verify).
const RAW = /\[rate_limit\]|\brate_limit\b|\b429\b|turn reported failure|API Error/i;

(async () => {
  const s = new Session('trouble');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const settingsFile = path.join(s.profile, 'User/settings.json');
  const userSettings = () => { try { return JSON.parse(fs.readFileSync(settingsFile, 'utf8')); } catch { return {}; } };
  try {
    const repo = makeRepo(path.join(s.root, 'site'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 250) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(250); } return run(id)?.status; };
    const setTheme = async theme => {
      const bg = `getComputedStyle(document.querySelector('.part.activitybar') || document.body).backgroundColor`;
      const before = await cdp.evalWorkbench(bg);
      s.settings({ ...userSettings(), 'workbench.colorTheme': theme });
      await cdp.waitFor(`${bg} !== ${JSON.stringify(before)}`, 20000, 'theme ' + theme).catch(() => {});
      await delay(1500);
    };

    // A second account, and an agent in the site so Overseer knows where new work goes.
    const work = s.ctl('profile.create', { name: 'Work', harness: 'claude' }).id;
    s.ctl('agent.cadence', { cadence: 'off', by: 'owner' });
    const seed = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Seed', prompt: 'look around' }).run.id;
    await waitStatus(seed, /completed|failed/);

    // ---------- The owner asks on home; the new agent hits its usage limit.
    await cdp.command('Overseer: Talk to Overseer'); await delay(2000);
    const view = await s.editorView(`!!document.getElementById('home') && !!document.querySelector('#task')`);
    const p = await s.webviewPoint(view, '#task');
    await cdp.click(p.x, p.y); await delay(250);
    await cdp.type('start an agent to write the API'); await delay(200); await cdp.key('Enter');
    await view.waitFor(`[...document.querySelectorAll('#home-conv .proposal')].some(c => /write the API/.test(c.textContent) && !c.querySelector('.proposal-actions').hidden)`, 60000);
    fs.writeFileSync(modeFile, 'ratelimit');
    const yesOn = async re => {
      const at = await view.eval(`(() => { const c = [...document.querySelectorAll('#home-conv .proposal')].reverse().find(c => ${re}.test(c.textContent) && !c.querySelector('.proposal-actions').hidden); if (!c) return null; c.scrollIntoView({ block: 'center' }); c.querySelector('[data-proposal=yes]').dataset.target = '1'; return true; })()`);
      if (!at) throw new Error('no open proposal matching ' + re);
      await delay(400);
      const pt = await s.webviewPoint(view, '[data-proposal=yes][data-target="1"]');
      await cdp.click(pt.x, pt.y);
      await view.eval(`document.querySelectorAll('[data-target]').forEach(e => delete e.dataset.target)`);
    };
    await yesOn('/write the API/');
    let limited;
    for (let i = 0; i < 120 && !(limited = s.ctl('state').runs.find(r => r.title === 'write the API')); i++) await delay(250);
    if (!limited) throw new Error('no agent started');
    const status = await waitStatus(limited.id, /failed|completed/);
    const raw = run(limited.id).exit_reason || '';
    check('the agent hit its usage limit, recorded with its raw reason in the daemon', status === 'failed' && RAW.test(raw), { status, exit_reason: raw, plain_reason: run(limited.id).plain_reason });

    // Overseer's card and its proposal on home: the plain reason, and continuing on Work.
    await view.waitFor(`[...document.querySelectorAll('#home-conv .card-trouble')].some(c => /write the API/.test(c.textContent))`, 30000);
    await view.waitFor(`[...document.querySelectorAll('#home-conv .proposal')].some(c => /Continue write the API on .*Work/.test(c.textContent) && !c.querySelector('.proposal-actions').hidden)`, 60000);
    await view.waitFor(`[...document.querySelectorAll('#home-conv .req-stage')].some(b => !b.hidden && /usage limit/.test(b.textContent))`, 20000).catch(() => {});
    await delay(800);
    const home = await view.eval(`(() => ({
      cards: [...document.querySelectorAll('#home-conv .card-trouble .card-text')].map(e => e.textContent.trim()),
      stages: [...document.querySelectorAll('#home-conv .req-stage')].filter(b => !b.hidden).map(b => b.textContent.trim()),
      proposals: [...document.querySelectorAll('#home-conv .proposal')].map(c => c.querySelector('.proposal-list').textContent.trim()),
      replies: [...document.querySelectorAll('#home-conv .home-msg.from-overseer')].map(e => e.textContent.trim()),
      text: document.body.innerText,
      attrs: [...document.querySelectorAll('[title], [aria-label]')].map(e => (e.getAttribute('title') || '') + ' ' + (e.getAttribute('aria-label') || '')).join('\\n'),
    }))()`);
    const turns = s.ctl('overseer.messages', { after: 0, limit: 500 }).messages.filter(m => m.card && m.card.kind === 'trouble' && m.card.agent === limited.id);
    check('home shows one Overseer card with the plain reason', turns.length === 1 && home.cards.includes("write the API reached its account's usage limit."), { cards: home.cards, troubleMessages: turns.length });
    check("the request's stage line says it in the same plain words", home.stages.some(t => t === "write the API reached its account's usage limit"), home.stages);
    check('Overseer proposes continuing on the other account (Work)', home.proposals.some(l => /Continue write the API on .*Work/.test(l)), home.proposals);
    check('home shows no error class or HTTP code (text, titles and labels)', !RAW.test(home.text) && !RAW.test(home.attrs), { raw: (home.text.match(RAW) || home.attrs.match(RAW) || [])[0] });
    for (const [theme, tag] of [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']]) { if (tag !== 'overseer') await setTheme(theme); await s.screenshot(`home-limited-${tag}`); }

    // ---------- The side bar's tooltip for the agent.
    await s.openOverseerView();
    const rowAt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'write the API').pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000, 'the agent in the side bar');
    await cdp.move(rowAt.x, rowAt.y - 30); await delay(300);
    await cdp.move(rowAt.x, rowAt.y);
    const hoverText = `[...document.querySelectorAll('.workbench-hover, .monaco-hover, .hover-contents')].filter(e => e.offsetParent || e.getClientRects().length).map(e => e.innerText).join('\\n')`;
    let tip = '';
    for (let i = 0; i < 40 && !/write the API/.test(tip); i++) { await delay(250); tip = await cdp.evalWorkbench(hoverText); }
    await s.screenshot('sidebar-tooltip');
    check("the side bar's tooltip gives the plain reason, no error class or HTTP code", /write the API/.test(tip) && /usage limit/i.test(tip) && !RAW.test(tip), { tip });
    await cdp.move(5, 5); await delay(300);

    // ---------- The agent's chat: its error, its status line's tooltip and its details.
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type('write the API'); await delay(400); await cdp.key('Enter'); await delay(2000);
    const chat = await cdp.webview(`document.getElementById('title')?.textContent === 'write the API' && !!document.querySelector('#conv .turn')`, 30000);
    await chat.eval(`document.getElementById('more').click()`); await delay(400);
    await chat.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => /^Details$/.test(b.textContent.trim()))?.click()`); await delay(600);
    const c = await chat.eval(`(() => ({
      errors: [...document.querySelectorAll('#conv .error-block')].map(e => e.innerText.trim()),
      meta: document.getElementById('meta')?.getAttribute('title') || '',
      details: [...document.querySelectorAll('dl dd')].map(d => d.textContent.trim()),
      sys: [...document.querySelectorAll('#conv .sys')].map(e => e.textContent.trim()),
      text: document.body.innerText,
      attrs: [...document.querySelectorAll('[title], [aria-label]')].map(e => (e.getAttribute('title') || '') + ' ' + (e.getAttribute('aria-label') || '')).join('\\n'),
    }))()`);
    await s.screenshot('chat-limited');
    check("the agent's chat says it in plain words: its error, its status line's tooltip and its details", c.errors.some(e => /usage limit/i.test(e)) && /usage limit/i.test(c.meta) && c.details.some(d => /usage limit/i.test(d)), { errors: c.errors, meta: c.meta, details: c.details.slice(0, 2) });
    check("the agent's chat shows no error class or HTTP code (text, titles and labels)", !RAW.test(c.text) && !RAW.test(c.attrs), { raw: (c.text.match(RAW) || c.attrs.match(RAW) || [])[0] });
    check("the agent's chat has no line naming the daemon's record (\"trouble\")", !c.sys.some(t => /^trouble$/i.test(t)), c.sys);
    // AC-231: what Overseer added to the start's prompt is shown, as one line that opens to it.
    check("the agent's chat shows what Overseer added to its task", c.sys.some(t => t.startsWith('Overseer added what it knows')), c.sys);
    await setTheme('Overseer');

    // ---------- Yes on home: the work goes on on Work, in the same worktree.
    fs.writeFileSync(modeFile, 'echo');
    await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
    await yesOn('/Continue write the API on .*Work/');
    let next;
    for (let i = 0; i < 120 && !(next = s.ctl('state').runs.find(r => r.profile_id === work && r.task_id === limited.task_id)); i++) await delay(250);
    const nextStatus = next ? await waitStatus(next.id, /completed|failed/) : null;
    const before = run(limited.id);
    check('Yes continues the work on the second account, in the same task and worktree', !!next && before.status === 'handed_off' && next.workspace_id === limited.workspace_id && nextStatus === 'completed', { before: before.status, next: next && { profile: next.profile_id, status: nextStatus, same_worktree: next.workspace_id === limited.workspace_id } });
    await delay(1000);
    const after = await view.eval(`document.body.innerText`);
    await s.screenshot('continued-on-work');
    check('home after the continue still shows no error class or HTTP code', !RAW.test(after), { raw: (after.match(RAW) || [])[0] });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { s.ctl('daemon.stop_all'); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
