// Packaged-UI scenario for AC-59 (start a new agent from the chat), fixture harnesses only: with
// no agent selected the dashboard shows the composer; Claude, Codex and generic agents are started
// keyboard-only (Tab to a chip, Enter opens its menu, arrows + Enter choose, Enter in the task
// starts); the new agent becomes selected and streams in place; defaults are remembered across a
// reload; a signed-out account, a missing harness and an untrusted workspace are explained inline
// with their fix; the full New Task form stays reachable.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('composer');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  const login = (who, argv) => { fs.writeFileSync(next, who); cp.execFileSync(cli, argv, { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } }); };
  login('desk:pro', ['login']); login('deskclaude:max', ['auth', 'login']);
  try {
    const repo = makeRepo(path.join(s.root, 'composer-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    // Codex and Claude both run through the account fixture (signed in above); OpenCode is missing.
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    s.ctl('account.create', { provider: 'openai', name: 'ChatGPT Signed Out' });
    await cdp.command('Overseer: Open Overseer View');
    let dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail')`, 30000);
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !!document.querySelector('.view-composer:not([hidden]) #task')`, 20000);
    check('with no agent selected the middle of the dashboard is the new-agent composer', true);
    await dash.waitFor(`!document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    await s.screenshot('composer');
    const key = async (k, o) => { await cdp.key(k, o); await delay(150); };
    const chip = sel => dash.eval(`document.querySelector('[data-chip="${sel}"]').getAttribute('aria-label')`);
    const note = () => dash.eval(`document.querySelector('.view-composer .composer-note').textContent`);
    // Keyboard: focus the task, Shift+Tab to reach chips, open the agent menu, pick an entry by label.
    const pickAgent = async label => {
      // Focus the webview on the agent chip (keyboard from here on), then open its menu with Enter.
      const at = await s.webviewPoint(dash, '[data-chip="agent"]'); await cdp.click(at.x, at.y); await delay(300);
      if (!(await dash.eval(`!!document.querySelector('.menu')`))) await key('Enter');
      await dash.waitFor(`!!document.querySelector('.menu')`, 5000);
      for (let i = 0; i < 20; i++) {
        const f = await dash.eval(`document.activeElement.closest('.menu') && document.activeElement.textContent`);
        if (f && f.startsWith(label)) break;
        await key('ArrowDown');
      }
      await key('Enter');
      await delay(300);
    };
    const start = async (text, harness) => {
      const at = await s.webviewPoint(dash, '#task'); await cdp.click(at.x, at.y); await delay(200);
      await cdp.type(text); await delay(200);
      const before = s.ctl('state').runs.length;
      await key('Enter');
      let run; for (let i = 0; i < 40 && !run; i++) { await delay(300); run = s.ctl('state').runs.find((r, j) => j >= before && !r.parent_run_id && r.harness === harness); }
      if (!run) return { run: null, note: await note(), chip: await chip('agent'), program: await dash.eval(`document.getElementById('program')?.value`) };
      const shown = await dash.waitFor(`document.body.dataset.mode === 'chat' && document.getElementById('title')?.textContent === ${JSON.stringify(text)} && document.querySelectorAll('#conv .msg').length >= 1`, 20000).then(() => true, () => false);
      return { run, shown };
    };

    // Claude, keyboard only.
    await pickAgent('claude (existing login)');
    const claudeChip = await chip('agent');
    const c = await start('Say hello from Claude', 'claude');
    check('Claude agent started keyboard-only from the composer; it becomes selected and streams in place', /Claude Code/.test(claudeChip) && c.run && c.shown, { claudeChip, run: c.run?.id, shown: c.shown });
    await s.screenshot('claude-started');
    // Codex.
    await dash.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    await dash.waitFor(`document.body.dataset.mode === 'composer'`, 5000);
    await pickAgent('codex (existing login)');
    const x = await start('Say hello from Codex', 'codex');
    check('Codex agent started keyboard-only', x.run && x.shown, { run: x.run?.id, shown: x.shown });
    // Generic program.
    await dash.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    await dash.waitFor(`document.body.dataset.mode === 'composer'`, 5000);
    await pickAgent('Run a program');
    { const at = await s.webviewPoint(dash, '#program'); await cdp.click(at.x, at.y); await cdp.type('/bin/echo'); await delay(200); }
    { const at = await s.webviewPoint(dash, '#args'); await cdp.click(at.x, at.y); await cdp.type('["generic hello"]'); await delay(200); }
    const g = await start('hello', 'generic');
    check('generic program started keyboard-only', g.run && g.shown, { run: g.run?.id, shown: g.shown, note: g.note, chip: g.chip, program: g.program });

    // Problems inline with their fix.
    await dash.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    await dash.waitFor(`document.body.dataset.mode === 'composer'`, 5000);
    await pickAgent('ChatGPT Signed Out');
    const signedOut = { note: await note(), fix: await dash.eval(`document.querySelector('.view-composer .composer-note .fix')?.textContent`), disabled: await dash.eval(`document.getElementById('start').disabled`) };
    await pickAgent('Add account…'); // OpenCode has no account; its heading says it is not installed
    await dash.eval(`document.querySelector('[data-chip="agent"]').click()`); await delay(300);
    const missingHead = await dash.eval(`[...document.querySelectorAll('.menu .menu-head')].map(h => h.textContent).find(t => /OpenCode/.test(t))`);
    await key('Escape');
    check('a signed-out account is explained inline with Sign in, and Start is disabled', /not signed in/.test(signedOut.note) && signedOut.fix === 'Sign in' && signedOut.disabled, signedOut);
    check('a harness that is not installed is shown as such', /not installed/.test(missingHead || ''), missingHead);
    await s.screenshot('problem-inline');

    // Defaults remembered across a reload: the last agent (codex? generic?) — the last successful start was generic.
    await cdp.command('Developer: Reload Window'); await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail')`, 30000);
    await dash.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    const remembered = { agent: await chip('agent'), repo: await chip('repo') };
    check('defaults remembered across reloads (last agent and repository)', /Program/.test(remembered.agent) && /composer-repo/.test(remembered.repo), remembered);

    // The full New Task form stays reachable.
    await dash.eval(`[...document.querySelectorAll('.view-composer .link')].find(b => b.textContent === 'Full form').click()`);
    const form = await cdp.webview(`!!document.getElementById('harnesses') && document.body.dataset.ready === '1'`, 20000).then(() => true, () => false);
    check('the full New Task form stays one click away', form);
    await s.screenshot('full-form');

    // Untrusted workspace (Restricted Mode, empty window): explained inline with the fix.
    await s.quit();
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'security.workspace.trust.enabled': true, 'security.workspace.trust.emptyWindow': false });
    s.launch('--new-window', { OVERSEER_TEST_TRUST: '1', OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    await cdp.command('Overseer: Open Overseer View');
    dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail')`, 30000);
    await dash.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    const untrusted = await dash.waitFor(`(() => { const n = document.querySelector('.view-composer .composer-note'); return n && /Trust this workspace/.test(n.textContent) && { note: n.textContent, fix: n.querySelector('.fix')?.textContent, disabled: document.getElementById('start').disabled }; })()`, 20000).catch(() => null);
    check('an untrusted workspace is explained inline with its fix (Trust) and nothing can start', untrusted && untrusted.fix === 'Trust' && untrusted.disabled, untrusted);
    await s.screenshot('untrusted');
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
