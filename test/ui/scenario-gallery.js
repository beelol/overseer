// Packaged-UI gallery for the Gate M design review (AC-108), fixture runs only: every Gate M view in
// the three Overseer themes (Overseer Dark, Overseer Light and Overseer). The chat beside the review,
// the review's All files navigator with an unchanged file open, the grid tracking an agent, the
// composer, Talk to Overseer, Where am I. Screenshots only; the checks
// are that each view was reached in each theme.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot, git } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Overseer'];
const slug = t => t.toLowerCase().replace(/\s+/g, '-');

(async () => {
  const s = new Session('gallery');
  const result = { checks: [], reached: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  const cli = fx('account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true }); fs.writeFileSync(next, 'desk:pro');
  cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  try {
    const web = makeRepo(path.join(s.root, 'web-app'), { dirty: false });
    fs.mkdirSync(path.join(web, 'src/auth'), { recursive: true });
    fs.writeFileSync(path.join(web, 'src/auth/session.ts'), 'export function currentSession() {\n  return readCookie("session");\n}\n');
    git(web, 'add', '.'); git(web, 'commit', '-q', '-m', 'auth');
    const api = makeRepo(path.join(s.root, 'api-server'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0] });
    s.install(latestVsix());
    s.launch(web, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 300) { if (re.test(state(id)?.status || '')) return; await delay(300); } };
    const claude = async (repo, mode, title, prompt, re) => { fs.writeFileSync(modeFile, mode); const t = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title, prompt }); await waitStatus(t.run.id, re); return t; };
    const showcase = await claude(web, 'showcase', 'Refresh sessions once', 'Expired sessions trigger a refresh in every tab. Make them refresh once and share the result, and add tests.', /completed|failed/);
    await claude(web, 'showcase-permission', 'Add a changelog entry', 'Add a changelog entry for the session refresh change.', /waiting_for_user/);
    await claude(api, 'nested', 'Split the payment service', 'Split the payment service into modules; delegate the refactor to a sub-agent.', /completed|failed/);
    s.ctl('task.create', { repo: api, harness: 'generic', program: '/bin/sh', args: ['-c', 'for i in $(seq 1 600); do echo "build step $i"; sleep 2; done'], prompt: '', title: 'Watch the build' });
    fs.writeFileSync(modeFile, 'overseer');
    const setTheme = async theme => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2000); };
    const reached = (view, theme, ok) => { (result.reached[view] ||= {})[theme] = !!ok; };

    // Talk to Overseer: one question, answered by the fixture (home keeps it for every theme, AC-227).
    await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
    const talk = await s.editorView(`!!document.getElementById('home-conv')`, 20000);
    await talk.eval(`(() => { window.overseerApi.postMessage({ type: 'overseerSend', text: 'What is everyone doing?' }); return true; })()`);
    await talk.waitFor(`/Here is what everyone is doing/.test(document.getElementById('home-conv').textContent)`, 40000);

    for (const theme of THEMES) {
      await setTheme(theme);
      const t = slug(theme);
      // Chat beside the review, then the review's All files navigator with an unchanged file open.
      await s.selectAgent('Refresh sessions once', { settle: 3000 });
      const reviewProbe = `!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(showcase.run.id)} && document.querySelectorAll('.diff-file').length > 0`;
      let review = await cdp.webview(reviewProbe, 12000).catch(() => null);
      if (!review) { await cdp.command('Overseer: Open Review'); review = await cdp.webview(reviewProbe, 15000).catch(() => null); }
      await delay(1200); await s.screenshot(`chat-and-review-${t}`); reached('chat and review', theme, !!review);
      if (review) {
        await review.eval(`(() => { if (document.body.dataset.nav !== 'all') document.getElementById('changes-only').click(); return true; })()`);
        await review.waitFor(`[...document.querySelectorAll('#tree details.folder > summary')].some(s => s.textContent === 'src')`, 10000).catch(() => {});
        await review.eval(`(() => { const s = [...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'src'); if (s && !s.parentElement.open) s.click(); return true; })()`); await delay(600);
        await review.eval(`(() => { const s = [...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'auth'); if (s && !s.parentElement.open) s.click(); return true; })()`); await delay(600);
        await review.eval(`document.querySelector('#tree .file[data-path="src/auth/session.ts"]')?.click()`);
        const open = await review.waitFor(`[...document.querySelectorAll('.diff-file.browsed')].some(e => e.dataset.loadState === 'rendered')`, 15000).then(() => true, () => false);
        await delay(800); await s.screenshot(`review-all-files-${t}`); reached('review, all files', theme, open);
        await review.eval(`(() => { if (document.body.dataset.nav !== 'changes') document.getElementById('changes-only').click(); document.querySelector('.diff-file.browsed .close-file')?.click(); return true; })()`);
      }
      // The grid, tracking an agent.
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
      const dash = await s.editorView(`document.body.dataset.mode === 'grid'`).catch(() => null);
      if (dash) {
        await dash.eval(`(() => { const t = [...document.querySelectorAll('.grid .tile')].find(t => /Watch the build/.test(t.textContent)); t?.querySelector('.tile-title')?.click(); return true; })()`);
        await delay(3000); await s.screenshot(`grid-tracking-${t}`);
        reached('grid tracking', theme, await dash.eval(`!!document.querySelector('.grid').dataset.tracked`));
        await dash.eval(`document.getElementById('grid-untrack')?.click()`); await delay(1500);
      }
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);
      // The composer.
      await cdp.command('Overseer: New Agent'); await delay(1500);
      await s.screenshot(`composer-${t}`); reached('composer', theme, !!(await s.editorView(`document.body.dataset.mode === 'composer'`).catch(() => null)));
      // Talk to Overseer.
      await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
      await s.screenshot(`talk-${t}`); reached('talk to Overseer', theme, await talk.eval(`document.body.dataset.mode === 'composer' && /Here is what everyone is doing/.test(document.getElementById('home-conv').innerText)`).catch(() => false));
      // Where am I.
      await s.selectAgent('Refresh sessions once', { settle: 2000 });
      await cdp.command('Overseer: Where Am I'); await delay(800);
      await s.screenshot(`where-am-i-${t}`); reached('where am I', theme, await cdp.evalWorkbench(`/Where am I/.test(document.querySelector('.quick-input-widget')?.textContent || '')`));
      await cdp.key('Escape'); await delay(400);
    }
    for (const [view, themes] of Object.entries(result.reached)) check(`${view}: reached in ${THEMES.join(', ')}`, THEMES.every(t => themes[t]), themes);
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
