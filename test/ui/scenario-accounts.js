// Packaged-UI scenario for AC-46 (and the AC-11 flows) with a SYNTHETIC account CLI
// (fixtures/fake-harness/account-cli.js stands in for `codex`/`claude` login commands; no real
// logins are touched). Adds one account per available provider through the UI and signs in
// through each provider's own flow (browser or device code for ChatGPT), shows Devin as
// unavailable, checks New Task only offers compatible accounts, shows desktop-linked accounts
// following the desktop login while fixed accounts do not, and that sign-out/re-sign-in and
// removal affect only that account.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('accounts');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  // Signed-in accounts show their plan and identity fingerprint ("team · c4a1579f").
  const SIGNED_IN = /[0-9a-f]{8}/;
  const sys = path.join(s.root, 'desktop-home');
  const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  // The "desktop apps" are signed in before Overseer starts.
  const desktopLogin = (who, harness) => { fs.writeFileSync(next, who); cp.execFileSync(cli, harness === 'claude' ? ['auth', 'login'] : ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } }); };
  desktopLogin('desk1:pro', 'codex'); desktopLogin('deskclaude:max', 'claude');
  try {
    const repo = makeRepo(path.join(s.root, 'acct-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'window.titleBarStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await s.openOverseerView();
    const rows = () => cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.closest('[id="workbench.view.extension.overseer"], .pane-body')).map(r => r.getAttribute('aria-label') || r.textContent)`);
    const accountRow = name => cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === ${JSON.stringify(name)}); return r ? (r.querySelector('.label-description')?.textContent || '') : null; })()`);
    const refresh = async () => { await cdp.command('Overseer: Refresh Account Status'); await delay(2500); };
    // The sign-in terminal runs asynchronously; refresh until the account shows the expected state.
    const refreshUntil = async (name, re) => { let last; for (let i = 0; i < 8; i++) { await refresh(); last = await accountRow(name); if (re.test(last || '')) return last; } return last; };
    const dialog = async button => {
      const d = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, 'dialog ' + button);
      await cdp.click(d.x, d.y); await delay(800); return d.text;
    };
    const toastButton = async (pattern, button) => {
      // Toasts slide in; measure once the toast has settled.
      const find = () => cdp.waitFor(`(() => { const t = [...document.querySelectorAll('.notification-toast')].find(t => ${pattern}.test(t.innerText)); if (!t) return null; const b = [...t.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, 'toast ' + button);
      await find(); await delay(600);
      const pt = await find();
      await cdp.click(pt.x, pt.y); await delay(800);
    };
    const menuEntries = async name => {
      const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === ${JSON.stringify(name)}); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2 }; })()`, 10000);
      await cdp.click(pt.x, pt.y); await delay(300);
      await cdp.key('F10', { shift: true });
      await cdp.waitFor(`!!document.querySelector('.monaco-menu .action-item')`, 10000, 'context menu');
      const entries = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-menu .action-item .action-label')].map(e => e.textContent.trim()).filter(Boolean)`);
      await cdp.key('Escape'); await delay(300);
      return entries;
    };
    const contextMenu = async (name, entry) => {
      const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === ${JSON.stringify(name)}); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2 }; })()`, 10000);
      await cdp.click(pt.x, pt.y); await delay(300);
      await cdp.key('F10', { shift: true }); // keyboard context menu on the selected row
      await cdp.waitFor(`!!document.querySelector('.monaco-menu .action-item')`, 10000, 'context menu');
      // Custom menus activate from the keyboard reliably: move focus to the entry, then Enter.
      for (let i = 0; i < 12; i++) {
        const focused = await cdp.evalWorkbench(`(document.querySelector('.monaco-menu .action-item.focused')?.textContent || '').trim()`);
        if (focused.startsWith(entry)) break;
        await cdp.key('ArrowDown'); await delay(120);
      }
      await cdp.key('Enter'); await delay(800);
    };
    await refresh();

    // Providers offered by Add Account (Devin shown but unavailable).
    await cdp.command('Overseer: Add Account');
    await cdp.waitQuickTitle('Add account: provider');
    const providerRows = (await cdp.quickInputState()).rows;
    check('Add Account lists providers: OpenAI/ChatGPT, Anthropic/Claude, OpenCode (local), Devin (unavailable)', ['OpenAI / ChatGPT', 'Anthropic / Claude', 'OpenCode (local models)', 'Devin'].every(p => providerRows.some(r => r.includes(p))) && providerRows.some(r => /Devin.*unavailable/.test(r)), providerRows);
    await cdp.type('Devin'); await delay(300); await cdp.key('Enter'); await delay(800);
    const devin = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].map(t => t.innerText).find(t => /Devin is not available/.test(t)) || null`, 10000).catch(() => null);
    check('Devin cannot be added (no account login; no API keys)', !!devin, devin);

    // OpenAI account through the UI, signed in with the device-code flow.
    fs.writeFileSync(next, 'work:team');
    await cdp.command('Overseer: Add Account');
    await cdp.pick('Add account: provider', 'OpenAI');
    await cdp.input('Name for the OpenAI', 'Work ChatGPT');
    await toastButton('/Created Work ChatGPT/', 'Sign In');
    await cdp.waitQuickTitle('Sign in Work ChatGPT');
    const methods = (await cdp.quickInputState()).rows;
    await cdp.type('device code'); await delay(300); await cdp.key('Enter');
    const work1 = await refreshUntil('Work ChatGPT', SIGNED_IN);
    const term1 = await cdp.evalWorkbench(`[...document.querySelectorAll('.xterm-rows, .terminal-wrapper')].map(e => e.innerText).join('\\n')`);
    check('OpenAI account added and signed in via the device-code flow (browser flow also offered)', methods.some(m => /browser/.test(m)) && methods.some(m => /device code/.test(m)) && /^team · [0-9a-f]{8}/.test(work1 || ''), { methods, work1, terminal: term1.slice(0, 200) });
    // Anthropic account through the UI.
    fs.writeFileSync(next, 'claudia:max');
    await cdp.command('Overseer: Add Account');
    await cdp.pick('Add account: provider', 'Anthropic');
    await cdp.input('Name for the Anthropic', 'Claude fixed');
    await toastButton('/Created Claude fixed/', 'Sign In');
    const claude1 = await refreshUntil('Claude fixed', SIGNED_IN);
    check('Anthropic account added and signed in through its own flow', /^max · [0-9a-f]{8}/.test(claude1 || ''), claude1);
    const desk1 = await accountRow('codex (existing login)');
    check('desktop logins are labeled as following the app; fixed accounts are not', /· desktop$/.test(desk1 || '') && /· desktop$/.test(await accountRow('claude (existing login)') || '') && !/· desktop$/.test(work1 || ''), { desk1, work1 });
    await s.screenshot('accounts-by-provider');

    // New Task offers only compatible accounts.
    const accountChoices = async harness => {
      await cdp.command('Overseer: Start Task with Quick Picks');
      await cdp.pick('New task: repository');
      await cdp.pick('New task: harness', harness);
      await cdp.waitQuickTitle(`New task: account for ${harness}`);
      await delay(500);
      const rowsNow = (await cdp.quickInputState()).rows;
      await s.screenshot('new-task-accounts-' + harness);
      await cdp.key('Escape'); await delay(500);
      return rowsNow;
    };
    const codexChoices = await accountChoices('codex');
    const claudeChoices = await accountChoices('claude');
    check('New Task offers only compatible accounts per harness', codexChoices.some(r => r.includes('Work ChatGPT')) && codexChoices.some(r => r.includes('codex (existing login)')) && !codexChoices.some(r => /Claude fixed|claude \(existing/.test(r)) &&
      claudeChoices.some(r => r.includes('Claude fixed')) && !claudeChoices.some(r => /Work ChatGPT|codex/.test(r)), { codexChoices, claudeChoices });

    // The desktop app switches accounts: the linked account follows, the fixed one does not.
    desktopLogin('desk2:plus', 'codex');
    await refresh();
    const desk2 = await accountRow('codex (existing login)');
    const work2 = await accountRow('Work ChatGPT');
    check('switching the desktop app account changes only the desktop-linked account', desk2 !== desk1 && /plus/.test(desk2) && work2 === work1, { desk1, desk2, work1, work2 });

    // Sign out and re-sign-in affect only that account.
    await contextMenu('Work ChatGPT', 'Sign Out');
    await dialog('Sign Out');
    await refresh();
    const signedOut = await accountRow('Work ChatGPT');
    // A signed-out account's menu offers Sign In, not Sign Out.
    const outMenu = await menuEntries('Work ChatGPT');
    check('a signed-out account offers Sign In, not Sign Out', outMenu.some(e => e.startsWith('Sign In')) && !outMenu.some(e => e.startsWith('Sign Out')), outMenu);
    const othersAfterOut = [await accountRow('Claude fixed'), await accountRow('codex (existing login)')];
    fs.writeFileSync(next, 'work-again:plus');
    await cdp.command('Overseer: Sign In');
    await cdp.pick('', 'Work ChatGPT');
    await cdp.pick('Sign in Work ChatGPT', 'browser');
    const resigned = await refreshUntil('Work ChatGPT', SIGNED_IN);
    check('sign-out and re-sign-in affect only that account', /signed out/.test(signedOut || '') && othersAfterOut[0] === claude1 && othersAfterOut[1] === desk2 && /^plus · [0-9a-f]{8}/.test(resigned || '') && resigned !== work1 &&
      (await accountRow('Claude fixed')) === claude1 && (await accountRow('codex (existing login)')) === desk2, { signedOut, resigned, othersAfterOut });

    // Removal affects only that account.
    await contextMenu('Claude fixed', 'Remove Account');
    const removeText = await dialog('Remove Account');
    await delay(1500);
    const afterRemove = await rows();
    check('removing an account deletes only it (desktop logins stay; others unchanged)', !afterRemove.some(r => /Claude fixed/.test(r)) && (await accountRow('Work ChatGPT')) === resigned && (await accountRow('claude (existing login)')) && fs.existsSync(path.join(sys, '.claude/.fixture-login.json')) && fs.existsSync(path.join(sys, '.codex/auth.json')),
      { removeText: removeText.slice(0, 200), afterRemove });
    await s.screenshot('after-remove');

    // Accounts added or removed outside VS Code (overseerd ctl) appear and disappear without a refresh.
    const outside = s.ctl('account.create', { provider: 'openai', name: 'Made in terminal' }).account;
    let appeared = null; for (let i = 0; i < 20 && appeared === null; i++) { await delay(250); appeared = await accountRow('Made in terminal'); }
    s.ctl('account.remove', { id: outside.id });
    let gone = false; for (let i = 0; i < 20 && !gone; i++) { await delay(250); gone = (await accountRow('Made in terminal')) === null; }
    check('an account created or removed outside VS Code shows up or leaves the Accounts view within 5 seconds, without Refresh', appeared !== null && gone, { appeared, gone });
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
