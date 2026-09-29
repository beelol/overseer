// Packaged-UI scenario for AC-261 (one Sign In, clearly Overseer's or clearly not), fixture only.
// A fresh profile with VS Code's built-in AI features left on (the harness turns them off for other
// scenarios): VS Code's own title-bar "Sign In" (its chat's sign-in, workbench.action.chat.signInIndicator)
// sits beside Overseer's own Accounts list unless Overseer hides it. Overseer contributes the default
// chat.titleBar.signIn.enabled = false, so the first Overseer view shows no competing sign-in control
// in Overseer Dark, Overseer Light and the bold Overseer theme. The owner can still turn it back on.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Overseer'];
// Every visible control in the title bar (and the activity bar) whose label or text says "Sign In".
const SIGN_IN = `[...document.querySelectorAll('.part.titlebar .action-item, .part.titlebar .monaco-button, .part.activitybar .action-item')]
  .filter(e => e.offsetParent && /sign\\s*in/i.test(e.getAttribute('aria-label') || e.textContent || ''))
  .map(e => (e.getAttribute('aria-label') || e.textContent || '').trim().slice(0, 60))`;

(async () => {
  const s = new Session('one-signin');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'signin-repo'), { dirty: false });
    // A fresh profile as a new owner has it: VS Code's chat and its sign-in are not turned off.
    s.settings({ 'chat.disableAIFeatures': false, 'workbench.colorTheme': THEMES[0] });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', OVERSEER_TEST_SYSTEM_HOME: path.join(s.root, 'system') });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    // VS Code's chat registers its title-bar entry a moment after start: give it the time it takes.
    await delay(4000);
    await s.openOverseerView();
    await cdp.command('Overseer: Open Overseer View');
    await s.editorView();
    await delay(1500);
    const chatOn = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.titlebar .action-item, .part.auxiliarybar, .statusbar-item')].some(e => e.offsetParent && /chat|copilot|agents/i.test(e.getAttribute('aria-label') || e.id || ''))`);
    check('VS Code\'s own chat is on in this profile (the title bar still shows its chat controls)', chatOn, { chatOn });
    for (const theme of THEMES) {
      if (theme !== THEMES[0]) {
        const file = path.join(s.profile, 'User/settings.json');
        const settings = JSON.parse(fs.readFileSync(file, 'utf8')); settings['workbench.colorTheme'] = theme;
        fs.writeFileSync(file, JSON.stringify(settings, null, 2));
        const kind = theme.includes('Light') ? 'vs' : 'vs-dark';
        await cdp.waitFor(`!!document.querySelector('.monaco-workbench.${kind}')`, 20000, 'theme ' + theme);
        await delay(1500);
      }
      const found = await cdp.evalWorkbench(SIGN_IN);
      const slug = theme.replace(/\W+/g, '-').toLowerCase();
      await s.screenshot(`first-view-${slug}`);
      const accounts = await cdp.evalWorkbench(`[...document.querySelectorAll('.pane-header')].some(h => h.offsetParent && /^Accounts/.test(h.textContent.trim()))`);
      check(`${theme}: the first Overseer view shows no competing Sign In (only Overseer's own Accounts)`, found.length === 0 && accounts, { found, accounts });
    }
    const setting = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.titlebar .action-item')].filter(e => e.offsetParent).map(e => e.getAttribute('aria-label') || e.textContent.trim()).filter(Boolean)`);
    s.note('title bar controls left', setting);
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
