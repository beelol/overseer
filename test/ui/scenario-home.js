// Packaged-UI scenario for AC-182 (Claude Code fixture, no paid tokens): one conversation, from
// home. With no agent selected the editor area shows the conversation with Overseer above the
// composer. Keyboard only: a task typed at home starts an agent (no Overseer turn) and its card
// appears; `@overseer …` sends to Overseer and starts no agent; an agent named with `@` reaches
// Overseer as its id; the two corrections; the docked chat shows the same conversation; Start
// fresh keeps a hold; screenshots in the three Overseer themes.
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
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const overseerTurns = () => { const sess = s.ctl('overseer.session'); return sess.run_id ? s.ctl('run.turns', { run_id: sess.run_id }).length : 0; };
    const runs = () => s.ctl('state').runs.filter(r => !r.parent_run_id);

    // Home: the conversation above the composer, the composer's target New agent.
    await cdp.command('Overseer: Open Overseer View'); await delay(2500);
    const home = await s.editorView(`!!document.querySelector('#task') && !!document.querySelector('#target')`);
    const target = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('home shows the conversation with Overseer above the composer, whose target is New agent', target === 'agent', { target });
    // A task typed at home starts an agent with no Overseer turn, and its card appears.
    { const p = await s.webviewPoint(home, '#task'); await cdp.click(p.x, p.y); await delay(150); }
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

    // `@overseer` sends the text to Overseer and starts no agent.
    fs.writeFileSync(modeFile, 'overseer');
    const nRuns = runs().length;
    { const p = await s.webviewPoint(home, '#task'); await cdp.click(p.x, p.y); await delay(150); }
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
    { const p = await s.webviewPoint(home, '#task'); await cdp.click(p.x, p.y); await delay(150); }
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
    { const p = await s.webviewPoint(home, `#home-conv [data-id="${lastOwner.id}"] [data-action="start-as-agent"]`); await cdp.click(p.x, p.y); }
    let corrected;
    for (let i = 0; i < 60 && !(corrected = runs().find(r => /add a changelog/.test(r.title))); i++) await delay(250);
    check('*Start as an agent* starts one from a message sent to Overseer', !!corrected && runs().length === before + 1, { corrected: corrected?.id });
    for (let i = 0; i < 60 && corrected && runs().find(r => r.id === corrected.id).status !== 'completed'; i++) await delay(250);
    await goHome();
    // An agent started by mistake: Ask Overseer instead stops it, removes its untouched worktree, and puts the words back for Overseer.
    { const p = await s.webviewPoint(home, '#task'); await cdp.click(p.x, p.y); await delay(150); }
    fs.writeFileSync(modeFile, 'slow');
    await cdp.type('what is the plan for the docs?'); await delay(200); await cdp.key('Enter');
    let mistaken;
    for (let i = 0; i < 60 && !(mistaken = runs().find(r => r.title.startsWith('what is the plan'))); i++) await delay(250);
    await goHome();
    await home.waitFor(`[...document.querySelectorAll('#home-conv .card')].some(c => c.dataset.kind === 'started' && /what is the plan/.test(c.textContent))`, 20000);
    await home.eval(`[...document.querySelectorAll('#home-conv .card[data-kind="started"]')].pop()?.scrollIntoView({ block: 'center' })`); await delay(200);
    { const p = await s.webviewPoint(home, '#home-conv .card[data-kind="started"]:last-of-type [data-action="ask-overseer-instead"]'); await cdp.click(p.x, p.y); }
    let gone = false;
    for (let i = 0; i < 80; i++) { const r = runs().find(r => r.id === mistaken.id); const ws = s.ctl('state').workspaces.find(w => w.id === r.workspace_id); if (r.status !== 'running' && ws && ws.removed_ms) { gone = true; break; } await delay(250); }
    const back = await home.eval(`document.querySelector('#task')?.value`);
    const chip2 = await home.eval(`document.querySelector('#target')?.dataset.target`);
    check('*Ask Overseer instead* stops the agent just started, removes its untouched worktree, and puts the words back for Overseer', gone && /what is the plan/.test(back || '') && chip2 === 'overseer', { gone, back, chip2 });
    await cdp.key('Escape');
    fs.writeFileSync(modeFile, 'overseer');

    // The docked chat shows the same conversation.
    await cdp.command('Overseer: Talk to Overseer'); await delay(2500);
    const docked = await cdp.webview(`!!document.querySelector('#conv') && /What is everyone doing/.test(document.querySelector('#conv').textContent)`, 30000);
    const dockedText = await docked.eval(`document.querySelector('#conv').innerText`);
    const homeText = await home.eval(`document.querySelector('#home-conv').innerText`);
    const same = ['What is everyone doing?', 'add a changelog'].every(t => dockedText.includes(t) && homeText.includes(t));
    check('the docked chat and home show the same conversation', same, { docked: dockedText.slice(0, 300), home: homeText.slice(0, 300) });
    await s.screenshot('docked-same');

    // Start fresh archives the conversation; a hold stays.
    const held = runs().find(r => r.id === started.id);
    s.ctl('agent.hold', { run_id: held.id, reason: 'kept through a fresh start', by: 'owner' });
    await goHome();
    await home.eval(`document.querySelector('#home-fresh')?.scrollIntoView({ block: 'center' })`); await delay(300);
    { const p = await s.webviewPoint(home, '#home-fresh'); await cdp.click(p.x, p.y); }
    await home.waitFor(`document.querySelectorAll('#home-conv .home-msg, #home-conv .card').length === 0`, 20000);
    const holds = s.ctl('agent.holds').holds;
    check('*Start fresh* begins a new conversation and leaves a hold in place', holds.some(h => h.run_id === held.id), { holds });
    await s.screenshot('home-fresh');

    // The three themes.
    for (const theme of ['Overseer Dark', 'Overseer Light']) {
      await cdp.command('Preferences: Color Theme'); await delay(800);
      await cdp.type(theme); await delay(600); await cdp.key('Enter'); await delay(1500);
      await s.screenshot('home-' + theme.toLowerCase().replace(/\s+/g, '-'));
    }
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
