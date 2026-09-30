// Packaged-UI scenario for AC-242 (keys act only on what you can see), fixture runs only. Two
// agents wait on a permission. With agent A's chat on screen, ⌥⌘Y answers A and never B. With no
// agent on screen (home), ⌥⌘Y first shows which agent asks for which tool and answers nothing until
// one is picked. With Overseer's proposal open, ⌥⌘J reaches it (Talk to Overseer). From home, Merge
// Back, Stop, Clean Up and Send Follow-up each ask which agent instead of acting on none.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('keys-on-screen');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'keys-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE', OVERSEER_TEST_SYSTEM_HOME: path.join(s.root, 'system') });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitFor = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 300) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(300); } return run(id)?.status; };
    fs.writeFileSync(modeFile, 'echo');
    const api = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write the API', title: 'API tests' });
    await waitFor(api.run.id, /completed/);
    fs.writeFileSync(modeFile, 'permission');
    const a = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Write file A' });
    await waitFor(a.run.id, /waiting_for_user/);
    const b = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Write file B' });
    await waitFor(b.run.id, /waiting_for_user/);
    const long = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'while true; do echo tick; sleep 1; done'], prompt: '', title: 'Long loop' });
    await waitFor(long.run.id, /running/);
    const key = async (k, o = {}) => { await cdp.focusWorkbench(); await cdp.key(k, o); await delay(900); };

    // Agent A on screen: ⌥⌘Y answers A, never B.
    await cdp.command('Overseer: Open Overseer View');
    const view = await s.editorView();
    await s.selectAgent('Write file A');
    await view.waitFor(`document.body.dataset.mode === 'chat' && window.__overseer.selected() === ${JSON.stringify(a.run.id)}`, 10000);
    await key('y', { meta: true, alt: true });
    const aDone = await waitFor(a.run.id, /completed/);
    await delay(800);
    const bStill = run(b.run.id).status;
    check('⌥⌘Y with agent A on screen answers A and never B', aDone === 'completed' && bStill === 'waiting_for_user', { aDone, bStill });

    // No agent on screen (home): ⌥⌘Y says which agent and tool first, and answers nothing yet.
    await key('n', { meta: true, alt: true });
    await view.waitFor(`document.body.dataset.mode === 'composer'`, 10000);
    await key('y', { meta: true, alt: true });
    await cdp.waitQuickTitle('Allow which request?', 10000);
    const pickState = await cdp.quickInputState();
    await s.screenshot('which-request');
    await delay(800);
    const whileAsking = run(b.run.id).status;
    await cdp.key('Escape'); await delay(800);
    const afterEscape = run(b.run.id).status;
    check('⌥⌘Y with no agent on screen first shows which agent asks for which tool, and answers nothing until one is picked',
      pickState.rows.length === 1 && /Write file B/.test(pickState.rows[0]) && /wants to use/.test(pickState.rows[0]) && whileAsking === 'waiting_for_user' && afterEscape === 'waiting_for_user', { pickState, whileAsking, afterEscape });
    await key('y', { meta: true, alt: true });
    await cdp.waitQuickTitle('Allow which request?', 10000);
    await cdp.key('Enter');
    const bDone = await waitFor(b.run.id, /completed/);
    check('picking the request allows exactly that one', bDone === 'completed', { bDone });

    // Palette commands with no agent on screen ask which one.
    const pickers = {};
    for (const [command, title] of [['Overseer: Merge Back…', 'Merge which agent?'], ['Overseer: Stop Selected Agent', 'Stop which agent?'], ['Overseer: Clean Up Worktree…', 'Clean up which agent'], ['Overseer: Send Follow-up…', 'Send a follow-up to which agent?']]) {
      await key('n', { meta: true, alt: true });
      await cdp.command(command);
      const shown = await cdp.waitQuickTitle(title, 8000).then(() => true, () => false);
      pickers[command] = shown ? (await cdp.quickInputState()).rows : null;
      if (shown && command === 'Overseer: Stop Selected Agent') await s.screenshot('stop-which');
      await cdp.key('Escape'); await delay(500);
    }
    const stillRunning = run(long.run.id).status;
    check('Merge Back, Stop, Clean Up and Send Follow-up from the palette with no agent on screen each show a picker (and do nothing until one is picked)',
      Object.values(pickers).every(rows => rows && rows.length > 0) && pickers['Overseer: Stop Selected Agent'].some(r => /Long loop/.test(r)) && stillRunning === 'running', { pickers, stillRunning });

    // Overseer's proposal open: ⌥⌘J reaches it.
    fs.writeFileSync(modeFile, 'overseer');
    s.ctl('overseer.send', { text: 'Tell API tests to add tests', surface: 'vscode', harness: 'claude' });
    let open = 0; for (let i = 0; i < 60 && !open; i++) { await delay(500); open = s.ctl('state').overseer?.open_proposals || 0; }
    await key('n', { meta: true, alt: true });
    await key('j', { meta: true, alt: true });
    const talk = await cdp.webview(`document.visibilityState === 'visible' && !!document.querySelector('.proposal:not(.answered)')`, 15000).catch(() => null);
    const card = talk && await talk.eval(`document.querySelector('.proposal:not(.answered)').innerText`);
    await s.screenshot('next-reaches-proposal');
    check('with Overseer\'s proposal open, ⌥⌘J reaches it (Talk to Overseer shows the proposal)', open > 0 && !!talk && /API tests/.test(card || ''), { open, card });
    // Leave nothing running.
    await s.ctl('run.interrupt', { run_id: long.run.id });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    // Whatever happened, no fixture agent is left waiting (daemon.stop_all interrupts them, then the daemon exits).
    if (!process.env.KEEP_OPEN) { await s.quit(); try { s.ctl('daemon.stop_all'); } catch {} s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
