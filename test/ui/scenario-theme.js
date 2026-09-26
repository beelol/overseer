// Packaged-UI scenario for AC-47 (no paid tokens): the tile-based New Task form, run panel,
// review and Overseer view in Dark, Light, High Contrast and High Contrast Light themes;
// keyboard-only task creation; accessible names on every control in each webview; and a lint
// that UI sources use VS Code theme tokens instead of hard-coded colors.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const THEMES = ['Default Dark Modern', 'Default Light Modern', 'Default High Contrast', 'Default High Contrast Light'];
// Accessible-name audit run inside a webview.
const AUDIT = `(() => { const bad = []; for (const e of document.querySelectorAll('button, [role=radio], [role=treeitem], [role=tab], input, select, textarea, a[href]')) {
  if (e.closest('[hidden], [aria-hidden="true"]') || e.offsetParent === null) continue;
  const label = e.getAttribute('aria-label') || e.getAttribute('aria-labelledby') || e.getAttribute('title') || (e.id && document.querySelector('label[for="' + e.id + '"]')?.textContent) || e.closest('label')?.textContent.trim() || e.textContent.trim() || e.getAttribute('placeholder');
  if (!label) bad.push(e.outerHTML.slice(0, 80)); } return { checked: document.querySelectorAll('button, [role=radio], [role=treeitem], input, select, textarea').length, bad }; })()`;

function lint() {
  const files = ['tokens.css', 'base.css', 'chat.css', 'dashboard.css', 'new-task.css', 'run-panel.css', 'ui.js', 'chat.js', 'conversation.js', 'dashboard.js', 'composer.js', 'grid.js',
    'prompt-tools.js', 'markdown.js', 'files.js', 'new-task.js', 'run-panel.js'].map(f => 'extension/media/' + f)
    .concat(['extension/src/output-panel.js', 'extension/src/command-center.js', 'extension/src/new-task.js', 'extension/src/webview-html.js', 'extension/branch-diff/review/browser.css', 'extension/branch-diff/review/panel.js']);
  const hits = [];
  for (const f of files) {
    fs.readFileSync(path.join(repoRoot, f), 'utf8').split('\n').forEach((line, i) => {
      if (/^\s*(\/\/|\*|\/\*)/.test(line)) return;
      for (const m of line.matchAll(/#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(|\b(?:white|black)\b(?=\s*[;}])/g)) hits.push(`${f}:${i + 1}: ${m[0]}`);
    });
  }
  return { files: files.length, hits };
}

(async () => {
  const s = new Session('theme');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  fs.writeFileSync(next, 'desk:pro');
  cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  try {
    const l = lint();
    check('UI sources use theme tokens only (no hard-coded colors)', l.hits.length === 0, l);
    const repo = makeRepo(path.join(s.root, 'theme-demo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': THEMES[0] });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next, OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'),
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,CLAUDE_FIXTURE_MODE', CLAUDE_FIXTURE_MODE: 'nested' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const nested = s.ctl('task.create', { repo, harness: 'claude', prompt: 'delegate to a child', title: 'Nested agents' });
    for (let i = 0; i < 40 && s.ctl('state').runs.find(r => r.id === nested.run.id).status !== 'completed'; i++) await delay(300);

    // Keyboard-only task creation in the New Task form.
    await cdp.command('Overseer: Open Overseer View');
    await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list')`, 30000);
    await cdp.command('Overseer: New Task');
    const form = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('harnesses')`, 30000);
    await form.waitFor(`document.querySelectorAll('#harnesses .tile').length >= 4 && document.querySelectorAll('#repos .tile').length >= 2`, 20000);
    const key = async (k, opts) => { await cdp.key(k, opts); await delay(120); };
    const typeText = async t => { await cdp.type(t); await delay(150); };
    const focused = () => form.eval(`({ id: document.activeElement.id, role: document.activeElement.getAttribute('role'), label: document.activeElement.getAttribute('aria-label') || document.activeElement.textContent.slice(0, 40), group: document.activeElement.closest('[role=radiogroup]')?.id })`);
    // Focus the first control from inside the webview, then only use the keyboard.
    const first = await s.webviewPoint(form, '#repos .tile'); await cdp.click(first.x, first.y); await delay(300);
    const trail = [await focused()];                             // repository tile
    await key('Tab'); trail.push(await focused());               // harness group
    for (let i = 0; i < 6 && !/^Program/.test((await focused()).label || ''); i++) { await key('ArrowRight'); }
    trail.push(await focused());                                 // Program (generic) tile (selected by arrows)
    await key('Tab'); trail.push(await focused());               // program path
    await typeText('/bin/sh');
    await key('Tab'); trail.push(await focused());               // arguments
    await key('a', { meta: true }); await typeText('["-c","echo keyboard > kb.txt"]');
    await key('Tab'); trail.push(await focused());               // workspace tiles
    const beforeRuns = s.ctl('state').runs.length;
    for (let i = 0; i < 10 && (await focused()).id !== 'start'; i++) await key('Tab');
    trail.push(await focused());
    await s.screenshot('new-task-keyboard');
    await key('Enter');
    let created;
    for (let i = 0; i < 30 && !created; i++) { await delay(300); created = s.ctl('state').runs.find((r, j) => j >= beforeRuns && r.harness === 'generic'); }
    const kbWs = created && s.ctl('state').workspaces.find(w => w.id === created.workspace_id);
    for (let i = 0; i < 20 && kbWs && !fs.existsSync(path.join(kbWs.path, 'kb.txt')); i++) await delay(300);
    check('keyboard-only task creation (tiles by arrow keys, Tab between groups, Enter to start)', !!created && fs.existsSync(path.join(kbWs.path, 'kb.txt')) && trail.some(t => t.group === 'harnesses' && /^Program/.test(t.label)) && trail.some(t => t.id === 'program') && trail.some(t => t.id === 'args') && trail.some(t => t.group === 'modes') && trail[trail.length - 1].id === 'start', { trail, run: created?.id });

    // Theme screenshots and accessibility audits.
    for (const theme of THEMES) {
      const settingsFile = path.join(s.profile, 'User/settings.json');
      const settings = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); settings['workbench.colorTheme'] = theme;
      fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2));
      await cdp.waitFor(`document.body.classList.contains(${JSON.stringify(theme.includes('High Contrast Light') ? 'hc-light' : theme.includes('High Contrast') ? 'hc-black' : theme.includes('Light') ? 'vs' : 'vs-dark')}) || !!document.querySelector('.monaco-workbench.${theme.includes('High Contrast Light') ? 'hc-light' : theme.includes('High Contrast') ? 'hc-black' : theme.includes('Light') ? 'vs' : 'vs-dark'}')`, 20000, 'theme ' + theme);
      await delay(1500);
      const slug = theme.replace(/^Default /, '').replace(/\W+/g, '-').toLowerCase();
      await cdp.command('Overseer: New Task');
      const f = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('harnesses')`, 30000);
      await f.waitFor(`document.querySelectorAll('#harnesses .tile').length >= 4`, 20000);
      await f.eval(`(() => { const t = [...document.querySelectorAll('#harnesses .tile')].find(t => /Codex app-server/.test(t.textContent)); t?.click(); })()`);
      await delay(600);
      const formAudit = await f.eval(AUDIT);
      const bodyClass = await f.eval(`document.body.className`);
      await s.screenshot(`new-task-${slug}`);
      // Overseer view + review + conversation for the nested run.
      const center = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list')`, 30000);
      const rootOf = h => s.ctl('state').runs.find(r => !r.parent_run_id && r.harness === h)?.id;
      await center.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(rootOf('generic'))}]')?.click()`);
      await delay(2500);
      // The conversation is part of the dashboard.
      const conv = center;
      await conv.waitFor(`!!document.querySelector('#conv .turn')`, 20000);
      const review = await cdp.webview(`!!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 20000).catch(() => null);
      const audits = { form: formAudit, center: await center.eval(AUDIT), conversation: await conv.eval(AUDIT), review: review ? await review.eval(AUDIT) : null };
      await s.screenshot(`views-${slug}`);
      await center.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(rootOf('claude'))}]')?.click()`);
      await delay(2000);
      await s.screenshot(`conversation-${slug}`);
      if (theme === THEMES[0]) {
        const cv = center; await cv.waitFor(`!!document.querySelector('#conv details.child')`, 20000);
        const nest = await cv.eval(`(() => { const child = [...document.querySelectorAll('#conv details.child')].find(d => d.querySelector(':scope > summary .child-title').textContent === 'child task'); return { grandInsideChild: !!child && [...child.querySelectorAll('details.child .child-title')].some(t => t.textContent === 'grandchild task'), childUnderAgent: child?.parentElement?.classList.contains('tool-children') }; })()`);
        check('delayed parent (grandchild reported first) ends up nested inside its child in the conversation', nest.grandInsideChild && nest.childUnderAgent, nest);
      }
      check(`${theme}: views render with the theme and every control has an accessible name`, /vscode-(light|dark|high-contrast)/.test(bodyClass) && Object.values(audits).every(a => a && a.bad.length === 0 && a.checked > 0), { bodyClass, audits });
    }
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
