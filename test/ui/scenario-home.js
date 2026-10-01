// Packaged-UI scenario for AC-182 (Claude Code fixture, no paid tokens): one conversation, from
// home. With no agent selected the editor area shows the conversation with Overseer above the
// composer; this profile has chosen "Start an agent directly" (AC-236, overseer.home.sendTo). Keyboard only: a task typed at home starts an agent (no Overseer turn) and its card
// appears; the target chip's menu and `@overseer …` send to Overseer and start no agent; an agent
// named with `@` reaches Overseer as its id; the two corrections and Start fresh, reached with Tab
// and pressed with Enter; the docked chat shows the same conversation; Start fresh keeps a hold;
// what Overseer did is a card with a row per agent; the text budget (AC-54) re-measured at 1280
// and 900 px; screenshots in the three Overseer themes.
const { auditExpression } = require('./audit');
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('home');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'home-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false, 'overseer.home.sendTo': 'agent' });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const overseerTurns = () => { const sess = s.ctl('overseer.session'); return sess.run_id ? s.ctl('run.turns', { run_id: sess.run_id }).length : 0; };
    const runs = () => s.ctl('state').runs.filter(r => !r.parent_run_id);
    // Keyboard only: Tab (or Shift+Tab) until the element the predicate names has focus; Enter acts on it.
    const tabTo = async (view, pred, { back = false, max = 80 } = {}) => {
      for (let i = 0; i < max; i++) {
        if (await view.eval(`(() => { const a = document.activeElement; return !!a && (${pred}); })()`)) return true;
        await cdp.key('Tab', { shift: back }); await delay(60);
      }
      throw new Error('focus never reached ' + pred);
    };
    const focused = sel => `a.matches(${JSON.stringify(sel)})`;
    // The task field, reached from the keyboard: the composer takes the focus when home opens
    // (Overseer: New Agent, from the command palette); Tab finds it otherwise.
    const inTask = () => home.eval(`document.activeElement?.id === 'task' && document.hasFocus()`);
    const focusTask = async () => {
      if (await inTask()) return;
      await cdp.command('Overseer: New Agent'); await delay(800);
      if (await inTask()) return;
      await tabTo(home, focused('#task'));
    };

    // Home: the conversation above the composer, the composer's target New agent.
    await cdp.command('Overseer: Open Overseer View'); await delay(2500);
    const home = await s.editorView(`!!document.querySelector('#task') && !!document.querySelector('#target')`);
    const target = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('home shows the conversation with Overseer above the composer, whose target is the remembered "Start directly" (AC-236)', target === 'agent', { target });
    // A task typed at home starts an agent with no Overseer turn, and its card appears.
    await focusTask();
    await cdp.type('tidy the docs'); await delay(200); await cdp.key('Enter');
    let started;
    for (let i = 0; i < 60 && !(started = runs().find(r => r.title.startsWith('tidy the docs'))); i++) await delay(250);
    check('Enter at home starts an agent exactly as before, with no Overseer turn in the event log', !!started && overseerTurns() === 0, { started: started?.id, overseerTurns: overseerTurns() });
    await home.waitFor(`[...document.querySelectorAll('#home-conv .card')].some(c => c.dataset.kind === 'started' && /tidy the docs/.test(c.textContent))`, 20000);
    await s.screenshot('home-started');
    check('the start appears in the conversation as a card', true);
    for (let i = 0; i < 60 && runs().find(r => r.id === started.id).status !== 'completed'; i++) await delay(250);
    // The start opened the agent's chat (AC-59); home is one command away (Overseer: New Agent).
    const goHome = async () => {
      for (let i = 0; i < 3; i++) {
        await cdp.command('Overseer: New Agent');
        if (await home.waitFor(`document.body.dataset.mode === 'composer' && !!document.querySelector('#task')`, 8000).then(() => true, () => false)) { await delay(500); return; }
      }
      throw new Error('home did not come back');
    };
    await goHome();

    // The target chip's menu, by keyboard: Tab to the chip, Enter opens it, the arrow picks, Enter chooses.
    await focusTask();
    await tabTo(home, focused('#target'));
    await cdp.key('Enter'); await delay(300);
    const menuOpen = await home.eval(`!!document.querySelector('.menu[role=menu]') && document.activeElement?.getAttribute('role') === 'menuitemradio'`);
    const items = await home.eval(`[...document.querySelectorAll('.menu[role=menu] .menu-label')].map(e => e.textContent)`);
    await cdp.key('ArrowDown'); await delay(100); await cdp.key('Enter'); await delay(300);
    const viaMenu = await home.eval(`document.querySelector('#target')?.dataset.target`);
    await s.screenshot('home-target-overseer');
    await cdp.key('Enter'); await delay(300); await cdp.key('ArrowUp'); await delay(100); await cdp.key('Enter'); await delay(300);
    const backToAgent = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('the target chip\'s menu, by keyboard, offers Overseer and Start an agent directly and switches between them', menuOpen && JSON.stringify(items) === JSON.stringify(['Overseer', 'Start an agent directly']) && viaMenu === 'overseer' && backToAgent === 'agent', { menuOpen, items, viaMenu, backToAgent });

    // `@overseer` sends the text to Overseer and starts no agent.
    fs.writeFileSync(modeFile, 'overseer');
    const nRuns = runs().length;
    await focusTask();
    await cdp.type('@overseer What is everyone doing?'); await delay(300);
    const chip = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('typing @overseer first switches the target chip to Overseer', chip === 'overseer', { chip });
    await cdp.key('Enter');
    await home.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-owner')].some(m => /What is everyone doing/.test(m.textContent))`, 20000);
    await home.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-overseer')].some(m => /Here is what everyone is doing/.test(m.textContent))`, 40000);
    const answer = await home.eval(`[...document.querySelectorAll('#home-conv .home-msg.from-overseer')].pop()?.textContent`);
    check('the same text sent to Overseer starts no agent and is answered in the conversation', runs().length === nRuns && /tidy the docs/.test(answer || ''), { runs: runs().length, answer: (answer || '').slice(0, 200) });
    await s.screenshot('home-overseer');

    // An agent named with `@` reaches Overseer as its id.
    await focusTask();
    await cdp.type('@overseer Tell @ti'); await delay(400);
    const menu = await home.eval(`[...document.querySelectorAll('#mentions .agent-mention')].map(e => e.textContent.trim()).filter(Boolean)`);
    check('`@` offers the agents by name, narrowed as you type', menu.some(t => /tidy the docs/.test(t)) && !menu.some(t => /^overseer/.test(t)), menu);
    await cdp.key('Enter'); await delay(200);
    const inserted = await home.eval(`document.querySelector('#task')?.value`);
    check('Enter inserts the named agent', /@tidy the docs /.test(inserted || ''), { inserted });
    await cdp.type(`to add a changelog`); await delay(200); await cdp.key('Enter');
    await home.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-owner')].some(m => /add a changelog/.test(m.textContent))`, 20000);
    const sess = s.ctl('overseer.session');
    const sent = sess.messages.filter(m => m.source === 'owner').map(m => m.text).pop();
    check('a named agent reaches Overseer as that agent\'s id', (sent || '').includes(`@tidy the docs (${started.id})`), { sent });
    await home.waitFor(`document.querySelectorAll('#home-conv .proposal:not(.answered)').length >= 1`, 40000).catch(() => {});
    for (const p of s.ctl('overseer.session').proposals) s.ctl('overseer.answer', { id: p.id, yes: false, surface: 'ctl', by: 'owner' });

    // Corrections: Start as an agent from an owner message; Ask Overseer instead from a started card.
    const lastOwner = s.ctl('overseer.session').messages.filter(m => m.source === 'owner').pop();
    await home.eval(`document.querySelector('#home-conv [data-id="${lastOwner.id}"]')?.scrollIntoView({ block: 'center' })`); await delay(300);
    fs.writeFileSync(modeFile, 'echo');
    const before = runs().length;
    await focusTask();
    await tabTo(home, `a.dataset.action === 'start-as-agent' && a.closest('[data-id]')?.dataset.id === ${JSON.stringify(lastOwner.id)}`, { back: true });
    await cdp.key('Enter');
    let corrected;
    for (let i = 0; i < 60 && !(corrected = runs().find(r => /add a changelog/.test(r.title))); i++) await delay(250);
    check('*Start as an agent* starts one from a message sent to Overseer', !!corrected && runs().length === before + 1, { corrected: corrected?.id });
    for (let i = 0; i < 60 && corrected && runs().find(r => r.id === corrected.id).status !== 'completed'; i++) await delay(250);
    await goHome();
    // An agent started by mistake: Ask Overseer instead stops it, removes its untouched worktree, and puts the words back for Overseer.
    await focusTask();
    fs.writeFileSync(modeFile, 'slow');
    await cdp.type('what is the plan for the docs?'); await delay(200); await cdp.key('Enter');
    let mistaken;
    for (let i = 0; i < 60 && !(mistaken = runs().find(r => r.title.startsWith('what is the plan'))); i++) await delay(250);
    await goHome();
    await home.waitFor(`[...document.querySelectorAll('#home-conv .card')].some(c => c.dataset.kind === 'started' && /what is the plan/.test(c.textContent))`, 20000);
    await home.eval(`[...document.querySelectorAll('#home-conv .card[data-kind="started"]')].pop()?.scrollIntoView({ block: 'center' })`); await delay(200);
    await focusTask();
    await tabTo(home, `a.dataset.action === 'ask-overseer-instead' && /what is the plan/.test(a.closest('.card')?.textContent || '')`, { back: true });
    await cdp.key('Enter');
    let gone = false;
    for (let i = 0; i < 80; i++) { const r = runs().find(r => r.id === mistaken.id); const ws = s.ctl('state').workspaces.find(w => w.id === r.workspace_id); if (r.status !== 'running' && ws && ws.removed_ms) { gone = true; break; } await delay(250); }
    const back = await home.eval(`document.querySelector('#task')?.value`);
    const chip2 = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('*Ask Overseer instead* stops the agent just started, removes its untouched worktree, and puts the words back for Overseer', gone && /what is the plan/.test(back || '') && chip2 === 'overseer', { gone, back, chip2 });
    await cdp.key('Escape');
    fs.writeFileSync(modeFile, 'overseer');

    // Talk to Overseer is this same conversation (AC-227: one view, nothing docked below).
    await cdp.command('Overseer: Talk to Overseer'); await delay(2500);
    const talk = await s.editorView(`document.querySelector('#target')?.dataset.target === 'overseer' && document.body.dataset.mode === 'composer'`, 30000);
    const talkText = await talk.eval(`document.querySelector('#home-conv').innerText`);
    const panelViews = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.panel .pane-header, .part.panel .composite-bar .action-label')].map(e => e.getAttribute('aria-label') || e.textContent.trim()).filter(t => /Overseer/.test(t))`);
    const same = ['What is everyone doing?', 'add a changelog'].every(t => talkText.includes(t));
    check('Talk to Overseer is home with the same conversation, and nothing is docked below', same && panelViews.length === 0, { talk: talkText.slice(0, 300), panelViews });
    await s.screenshot('talk-is-home');

    // Start fresh archives the conversation; a hold stays.
    const held = runs().find(r => r.id === started.id);
    s.ctl('agent.hold', { run_id: held.id, reason: 'kept through a fresh start', by: 'owner' });
    await goHome();
    // What Overseer did is a card with a row per agent (AC-185): a message to an agent, sent at Steer.
    fs.writeFileSync(modeFile, 'echo');
    s.ctl('overseer.level', { level: 'steer' });
    s.ctl('overseer.propose', { actions: [{ action: 'message', agent: corrected.id, text: 'Please add the release date too.', why: 'named' }], source: 'ctl' });
    await home.waitFor(`[...document.querySelectorAll('#home-conv .done-card .card-row')].some(r => /add a changelog/.test(r.textContent) && /delivered|answered|picked up/.test(r.textContent))`, 40000);
    const cardRow = await home.eval(`(() => { const r = [...document.querySelectorAll('#home-conv .done-card .card-row')].pop(); return { text: r.innerText, title: r.title, state: r.dataset.state }; })()`);
    check('what Overseer did is a card with one row per agent: why, the delivery, the state and its time, the whole text in the tooltip', /named/.test(cardRow.text) && /add/.test(cardRow.text) && /\d\d:\d\d/.test(cardRow.text) && cardRow.title.includes('Please add the release date too.'), cardRow);
    await home.eval(`document.querySelector('#home-conv .done-card:last-of-type')?.scrollIntoView({ block: 'center' })`); await delay(300);
    await s.screenshot('home-card-rows');
    s.ctl('overseer.level', { level: 'ask_first' });
    fs.writeFileSync(modeFile, 'overseer');
    // Start fresh, by keyboard.
    await focusTask();
    await tabTo(home, focused('#home-fresh'), { back: true, max: 200 });
    // The level and Start fresh sit at the bottom right, under the message box (the owner, 2026-09-30).
    const foot = await home.eval(`(() => { const f = document.getElementById('home-fresh'), l = document.getElementById('home-level'), box = document.querySelector('.composer-foot'); const r = f.getBoundingClientRect(), b = box && box.getBoundingClientRect(); return { inFoot: !!box && box.contains(f) && box.contains(l), level: l.textContent, right: b ? Math.round(b.right - r.right) : null, belowBox: b ? r.top >= b.top : false }; })()`);
    check('Ask first and Start fresh sit at the bottom right, under the message box', foot.inFoot && foot.level === 'Ask first' && foot.right !== null && foot.right < 24 && foot.belowBox, foot);
    await s.screenshot('home-level-and-fresh');
    await cdp.key('Enter');
    await home.waitFor(`document.querySelectorAll('#home-conv .home-msg, #home-conv .card').length === 0`, 20000);
    const holds = s.ctl('agent.holds').holds;
    check('*Start fresh* begins a new conversation and leaves a hold in place', holds.some(h => h.run_id === held.id), { holds });
    await s.screenshot('home-fresh');

    // The three themes, and the text budget (AC-54) re-measured in each at 1280 and 900 px: no
    // overflow, no unbroken run over 80 characters, every icon-only control named, and home's text
    // at least 40% under the baseline's new-agent view (1,037 characters).
    await goHome(); await delay(800);
    // The theme through the user's settings (VS Code applies it at once; the picker's filter can
    // fall through to the Marketplace); light or dark is read back from the workbench.
    const setTheme = async theme => {
      const bg = `getComputedStyle(document.querySelector('.part.activitybar') || document.body).backgroundColor`;
      const before = await cdp.evalWorkbench(bg);
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false, 'overseer.home.sendTo': 'agent' });
      const want = /Light/.test(theme) ? 'vs' : 'vs-dark';
      await cdp.waitFor(`(() => { const w = document.querySelector('.monaco-workbench'); return !!w && w.classList.contains(${JSON.stringify(want)}) && ${bg} !== ${JSON.stringify(before)}; })()`, 20000, 'theme ' + theme);
      await delay(1500);
    };
    const setWidth = async w => { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 900, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1500); };
    result.textBudget = {};
    for (const theme of ['Overseer', 'Overseer Dark', 'Overseer Light']) {
      if (theme !== 'Overseer') await setTheme(theme);
      for (const w of [1280, 900]) {
        await setWidth(w);
        const a = await home.eval(auditExpression({ root: 'body' }));
        result.textBudget[`${theme}@${w}`] = { chars: a.chars, longRuns: a.longRuns, overflow: a.overflow.length, unnamed: a.unnamed };
        await s.screenshot(`home-${theme.toLowerCase().replace(/\s+/g, '-')}-${w}`);
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    const budget = Object.values(result.textBudget);
    check('the text budget (AC-54) at 1280 and 900 px in the three themes: no overflow, no long runs, every icon-only control named, and at least 40% under the baseline new-agent view', budget.length === 6 && budget.every(b => b.overflow === 0 && b.longRuns.length === 0 && b.unnamed.length === 0 && b.chars <= 622), result.textBudget);
    s.ctl('agent.release', { run_id: held.id, by: 'owner' });
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
