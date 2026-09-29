// Packaged-UI scenario for AC-258 (VS Code's own chat panel stays out of Overseer's way all
// session), fixture agents only. A fresh profile with VS Code's AI features on and its secondary
// side bar shown by default, so VS Code's own chat view is open when the window starts. At 1440×900
// and 1920×1080: start an agent from Overseer's home, follow it, open its review; none of the three
// screenshots shows VS Code's chat view. Then the owner opens VS Code's chat themselves: selecting
// agents and opening reviews leaves it open.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

// VS Code's chat view on screen: the chat's view container (workbench.panel.chat) in a visible part.
const CHAT_SHOWN = `(() => {
  const shown = e => !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none';
  const parts = [...document.querySelectorAll('.part.auxiliarybar, .part.panel, .part.sidebar')].filter(p => shown(p) && !p.classList.contains('hidden'));
  return parts.some(p => [...p.querySelectorAll('.interactive-session, .chat-widget, [id^="workbench.panel.chat"], .pane-body.chat-viewpane')].some(shown));
})()`;

(async () => {
  const s = new Session('vscode-chat');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'chat-repo'), { dirty: false });
    // As a new owner has it: VS Code's AI features on, its secondary side bar (with its chat) shown.
    s.settings({ 'chat.disableAIFeatures': false, 'workbench.secondarySideBar.defaultVisibility': 'visible', 'workbench.colorTheme': 'Overseer Dark', 'overseer.followNewRuns': true });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await delay(3000);
    const atStart = await cdp.evalWorkbench(CHAT_SHOWN);
    await s.screenshot('window-start-vscode-chat-open');
    check('at the start VS Code\'s own chat view is open (a fresh profile with AI features on)', atStart);

    const sizes = [[1440, 900], [1920, 1080]];
    let n = 0;
    for (const [width, height] of sizes) {
      const size = `${width}x${height}`;
      await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1200);
      const title = `Tidy the copy ${++n}`;
      // Start an agent from Overseer's home.
      fs.writeFileSync(modeFile, 'showcase');
      await cdp.command('Overseer: New Agent');
      await s.editorView(`!document.querySelector('.view-composer').hidden`);
      await delay(2500);
      const shown = [];
      shown.push({ step: 'start', chat: await cdp.evalWorkbench(CHAT_SHOWN) });
      await s.screenshot(`${size}-1-home`);
      const run = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', prompt: 'Make sessions refresh once.', title });
      for (let i = 0; i < 60 && s.ctl('state').runs.find(r => r.id === run.run.id).status !== 'completed'; i++) await delay(300);
      // Follow it: its chat.
      await s.selectRun(run.run.id);
      shown.push({ step: 'follow', chat: await cdp.evalWorkbench(CHAT_SHOWN) });
      await s.screenshot(`${size}-2-following`);
      // Its review.
      await cdp.command('Overseer: Open Review');
      await cdp.webview(`!!document.getElementById('diffs')`, 20000).catch(() => null);
      await delay(1500);
      shown.push({ step: 'review', chat: await cdp.evalWorkbench(CHAT_SHOWN) });
      await s.screenshot(`${size}-3-review`);
      check(`${size}: starting an agent, following it and opening its review never shows VS Code's own chat view`, shown.every(x => !x.chat), shown);
    }

    // The owner opens VS Code's chat themselves: it stays.
    await cdp.command('Chat: Open Chat'); await delay(2500);
    const reopened = await cdp.evalWorkbench(CHAT_SHOWN);
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent');
    await cdp.type('Tidy the copy 1'); await delay(300); await cdp.key('Enter'); await delay(2500);
    await cdp.command('Overseer: Open Review'); await delay(2500);
    const still = await cdp.evalWorkbench(CHAT_SHOWN);
    await s.screenshot('owner-reopened-chat-stays');
    check('the owner reopening VS Code\'s chat is left alone (it stays open while agents are selected and reviews open)', reopened && still, { reopened, still });
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
