// Packaged-UI scenario for AC-56 (Overseer themes) and AC-65 (provider logos), fixture runs only.
// In Overseer Dark, Overseer Light and High Contrast: screenshots of the dashboard with a chat and
// a diff, a terminal with ANSI colors, the agent grid, the new-agent composer and the Accounts view;
// provider logos appear on agent rows (side-bar tree icons in Gate K), the chat header, the composer's agent chip and menu, grid
// tiles and the Accounts view (native SVG icons); switching themes restyles open Overseer views
// live; the VSIX ships the third-party notices and license texts for every bundled logo.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Default High Contrast'];

(async () => {
  const s = new Session('look');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  fs.writeFileSync(next, 'desk:pro'); cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  const modeFile = path.join(s.root, 'claude-mode');
  let busy;
  try {
    // Licenses shipped in the VSIX (AC-65).
    const listing = cp.execFileSync('unzip', ['-l', latestVsix()], { encoding: 'utf8' });
    const shipped = ['extension/NOTICE.md', 'extension/media/vendor/licenses/simple-icons-LICENSE.md', 'extension/media/vendor/licenses/lobehub-icons-LICENSE.txt', 'extension/media/logos.js', 'extension/media/logos/openai-dark.svg', 'extension/media/logos/claudecode-light.svg'].map(f => [f, listing.includes(f)]);
    const notice = fs.readFileSync(path.join(repoRoot, 'extension/NOTICE.md'), 'utf8');
    check('the VSIX ships the third-party notices and the license of every bundled logo source', shipped.every(([, ok]) => ok) && /Simple Icons 16\.32\.0 \| CC0-1\.0/.test(notice) && /LobeHub Icons .* \| MIT/.test(notice), shipped);

    const repo = makeRepo(path.join(s.root, 'look-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0], 'overseer.grid.maxTiles': 4 });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next, CLAUDE_FIXTURE_MODE_FILE: modeFile,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    fs.writeFileSync(modeFile, 'showcase');
    const show = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Refresh sessions once', prompt: 'Make expired sessions refresh once.' });
    const codex = s.ctl('task.create', { repo, harness: 'codex', profile_id: 'system-codex', title: 'Codex check', prompt: 'say hi' });
    busy = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'while true; do echo working; sleep 1; done'], prompt: '', title: 'Busy loop' });
    for (let i = 0; i < 40 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === show.run.id).status); i++) await delay(300);
    for (let i = 0; i < 40 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === codex.run.id).status); i++) await delay(300);
    s.ctl('account.create', { provider: 'openai', name: 'ChatGPT Work' });
    await cdp.command('Overseer: Refresh Account Status'); await delay(1500);

    // Gate K: agents are picked in the side bar; the editor view shows the chat, composer or grid.
    await s.selectRun(show.run.id, { settle: 2500 });
    const dash = await s.editorView(`!!document.querySelector('#conv .msg.agent')`);
    const setTheme = async t => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = t; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2000); };
    const rowLogos = () => cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.getAttribute('aria-level') === '2').map(r => { const i = r.querySelector('.custom-view-tree-node-item-icon'); return ((i ? getComputedStyle(i).backgroundImage : '').match(/logos\\/([a-z-]+)\\.svg/) || [])[1] || ''; }).filter(Boolean); })()`);

    // Logos: side-bar rows (tree icons) and the chat header.
    const logos = { rows: await rowLogos(), chatHeader: await dash.eval(`[...document.querySelectorAll('.chat-meta svg.logo')].map(e => e.getAttribute('class'))`) };
    await cdp.command('Overseer: New Agent'); await delay(1500);
    await dash.waitFor(`!!document.querySelector('[data-chip="agent"]') && !document.querySelector('[data-chip="agent"]').textContent.includes('Loading') && !!document.querySelector('[data-chip="agent"] svg.logo')`, 20000).catch(() => {});
    logos.composerChip = await dash.eval(`document.querySelector('[data-chip="agent"] svg.logo')?.getAttribute('class')`);
    await dash.eval(`document.querySelector('[data-chip="agent"]').click()`); await delay(400);
    logos.agentMenu = await dash.eval(`[...document.querySelectorAll('.menu .menu-item svg.logo')].map(e => e.getAttribute('class'))`);
    await s.screenshot('composer-agent-menu-dark');
    await cdp.key('Escape');
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    const grid = await cdp.webview(`!!document.querySelector('.grid .tile')`, 20000);
    logos.gridTiles = await grid.eval(`[...document.querySelectorAll('.grid .tile .tile-who svg.logo, .grid .tile .tile-who .codicon')].map(e => e.getAttribute('class'))`);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    await s.selectRun(show.run.id, { settle: 2000 });
    // Native Accounts view icons.
    await s.openOverseerView();
    logos.accounts = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => { const i = r.querySelector('.custom-view-tree-node-item-icon'); return i ? getComputedStyle(i).backgroundImage : ''; }).filter(u => /logos\\//.test(u)).map(u => u.split('logos/')[1].split(/[")]/)[0])`);
    s.note('logos', logos);
    check('provider logos on agent rows, the chat header, the composer agent chip and menu, grid tiles and the Accounts view',
      logos.rows.some(c => /^claudecode-/.test(c)) && logos.rows.some(c => /^codex-/.test(c)) && logos.chatHeader.some(c => /logo-claudecode/.test(c)) && /logo-/.test(logos.composerChip || '') &&
      logos.agentMenu.some(c => /logo-codex/.test(c)) && logos.agentMenu.some(c => /logo-claudecode/.test(c)) && logos.gridTiles.length > 0 && logos.accounts.some(u => /^openai-/.test(u)) && logos.accounts.some(u => /^claude-/.test(u)), logos);

    // Live theme switch: open views restyle without a reload.
    const bg = async () => (await s.editorView()).eval(`getComputedStyle(document.body).backgroundColor`);
    const darkBg = await bg();
    await setTheme('Overseer Light');
    const lightBg = await bg();
    check('switching themes restyles open Overseer views live', darkBg !== lightBg && /rgb/.test(lightBg), { darkBg, lightBg });

    // Screenshots per theme: dashboard (chat + diff), grid, terminal, accounts.
    for (const theme of THEMES) {
      await setTheme(theme);
      const slug = theme.replace(/^Default /, '').replace(/\W+/g, '-').toLowerCase();
      await s.selectRun(show.run.id, { settle: 2000 });
      await cdp.command('Overseer: Open Review'); await delay(2000);
      await s.screenshot(`dashboard-diff-${slug}`);
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
      await s.screenshot(`grid-${slug}`);
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);
      await cdp.command('View: Toggle Terminal'); await delay(1500);
      await cdp.type("printf '\\033[31mred \\033[32mgreen \\033[33myellow \\033[34mblue \\033[35mmagenta \\033[36mcyan \\033[0mdefault\\n'"); await cdp.key('Enter'); await delay(1000);
      await s.screenshot(`terminal-${slug}`);
      await cdp.command('View: Toggle Terminal'); await delay(800);
    }
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (busy) s.ctl('run.interrupt', { run_id: busy.run.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
