// Packaged-UI scenario for AC-50 against a SYNTHETIC GitHub API (fixtures/mock-github) and a
// local bare repository standing in for github.com (git insteadOf); no real GitHub, no tokens.
// Covers: Open PR explained when there is no remote and when the remote is not GitHub; the
// signed-out case (real GitHub API URL, no VS Code GitHub session); then, pointed at the mock,
// the branch is committed and pushed, a pull request is created with a generated description,
// re-opening finds the existing one, and nothing is merged. The live PR is owner-confirmed.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, git, repoRoot } = require('./harness');

(async () => {
  const s = new Session('pr');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mockLog = path.join(s.root, 'github.log'), portFile = path.join(s.root, 'github.port');
  const mock = cp.spawn(process.execPath, [path.join(repoRoot, 'fixtures/mock-github/server.js')], { env: { ...process.env, MOCK_PORT: '0', MOCK_PORT_FILE: portFile, MOCK_LOG: mockLog, MOCK_TOKEN: 'test-token' }, stdio: 'ignore' });
  try {
    const repo = makeRepo(path.join(s.root, 'pr-demo'), { dirty: false });
    const bare = path.join(s.root, 'github-standin.git');
    git(s.root, 'init', '-q', '--bare', bare);
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_TEST_GITHUB_TOKEN: 'test-token' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', `sed -i '' 's/^L4: original$/L4: agent change for the PR/' a.txt; printf 'notes\\n' > NOTES.md`], prompt: 'Change L4 and add NOTES.md', title: 'PR demo change' });
    for (let i = 0; i < 40 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);
    const mainBefore = git(repo, 'rev-parse', 'main');
    await s.openOverseerView();
    const pt = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
      const i = rows.findIndex(r => r.textContent.includes('PR demo change')); const r = rows[i + 1]; if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(pt.x, pt.y); await delay(1500);
    const panel = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(t.run.id)} && !!document.getElementById('more')`, 30000);
    // Open pull request lives in the chat's … menu.
    const clickPr = async () => {
      for (let i = 0; i < 20; i++) {
        if (!(await panel.eval(`!!document.getElementById('pr')`))) { const m = await s.webviewPoint(panel, '#more'); await cdp.click(m.x, m.y); await delay(300); }
        if (await panel.eval(`!document.getElementById('pr').disabled`)) break;
        await panel.eval(`document.getElementById('more').click()`); await delay(500);
      }
      const p = await s.webviewPoint(panel, '#pr'); await cdp.click(p.x, p.y); await delay(1200);
    };
    // Open PR answers with dialogs, not toasts (VS Code's Do Not Disturb hides toasts). Read one, then close it.
    const dialogText = pattern => cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); return d && ${pattern}.test(d.innerText) ? d.innerText : null; })()`, 20000).catch(() => null);
    const closeDialog = async () => {
      const b = await cdp.evalWorkbench(`(() => { const d = document.querySelector('.monaco-dialog-box'); const b = d && [...d.querySelectorAll('.monaco-button')].find(b => /^(Cancel|OK)$/.test(b.textContent.trim())); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
      if (b) await cdp.click(b.x, b.y); await delay(500);
    };
    const toast = async pattern => { const t = await dialogText(pattern); return t; };
    const clear = closeDialog;

    // Like the owner's VS Code: Do Not Disturb on, which hides info and warning toasts.
    await cdp.command('Notifications: Toggle Do Not Disturb Mode'); await delay(500);
    // No remote.
    await clickPr();
    const noRemote = await toast('/no Git remote/');
    check('Open PR explains a missing remote', !!noRemote, noRemote);
    await clear();
    // A non-GitHub remote.
    git(repo, 'remote', 'add', 'origin', 'https://gitlab.example.invalid/team/pr-demo.git');
    await clickPr();
    const notGitHub = await toast('/not on GitHub/');
    check('Open PR explains a non-GitHub remote', !!notGitHub, notGitHub);
    await clear();
    // A GitHub remote, but VS Code is not signed in to GitHub (real API URL, so no test token).
    git(repo, 'remote', 'set-url', 'origin', 'https://github.com/test-owner/pr-demo.git');
    git(repo, 'config', `url.${bare}.insteadOf`, 'https://github.com/test-owner/pr-demo.git');
    await clickPr();
    const signedOut = await toast('/not signed in to GitHub/');
    check('signed out of GitHub in VS Code: explained, with Sign in to GitHub (no token needed)', /Sign in to GitHub/.test(signedOut || '') && /No personal access token/.test(signedOut || ''), signedOut);
    await s.screenshot('signed-out');
    await clear();
    const nothingYet = !fs.existsSync(mockLog) && !git(bare, 'branch', '--list').includes('pr-demo');
    check('nothing was pushed or sent while signed out', nothingYet, nothingYet);

    // Point Open PR at the mock API (settings reload live), then open the pull request.
    const port = Number(fs.readFileSync(portFile, 'utf8'));
    const settingsFile = path.join(s.profile, 'User/settings.json');
    const settings = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); settings['overseer.github.apiUrl'] = `http://127.0.0.1:${port}`;
    fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2)); await delay(1500);
    await clickPr();
    const dialog = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === 'Open PR'); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, 'Open PR dialog');
    await s.screenshot('confirm-open-pr');
    await cdp.click(dialog.x, dialog.y);
    const opened = await toast('/Pull request #42 is open/');
    const requests = fs.readFileSync(mockLog, 'utf8').trim().split('\n').map(l => JSON.parse(l));
    const create = requests.find(r => r.method === 'POST');
    const branch = t.workspace.branch;
    const pushed = git(bare, 'rev-parse', `refs/heads/${branch}`);
    const worktreeHead = git(t.workspace.path, 'rev-parse', 'HEAD');
    check('Open PR commits the worktree, pushes the branch and creates the pull request with a generated description',
      !!opened && /test-owner\/pr-demo: overseer\/pr-demo-change → main/.test(dialog.text) && create?.authorized && create.body.head === branch && create.body.base === 'main' && create.body.title === 'PR demo change' &&
      /Opened by Overseer from run/.test(create.body.body) && /Change L4 and add NOTES\.md/.test(create.body.body) && /`M` a\.txt/.test(create.body.body) && /`A` NOTES\.md/.test(create.body.body) && /never merges automatically/.test(create.body.body) && pushed === worktreeHead,
      { opened, dialog: dialog.text, create: create && { ...create, body: { ...create.body, body: create.body.body.slice(0, 400) } }, pushed, worktreeHead });
    await s.screenshot('pr-opened');
    const recorded = s.ctl('events.list', { run_id: t.run.id, limit: 5000 }).events.find(e => e.kind === 'pull_request');
    check('the pull request is recorded on the run (URL and number only)', recorded?.payload.number === 42 && /\/pull\/42$/.test(recorded.payload.url), recorded?.payload);
    await clear();
    // Opening again finds the existing pull request instead of failing.
    await clickPr();
    const again = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); const b = d && [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === 'Open PR'); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000);
    await cdp.click(again.x, again.y);
    const existing = await toast('/Pull request #42 is open/');
    check('opening again reuses the existing pull request', !!existing, existing);
    const tokenLeak = [path.join(s.home, 'overseer.sqlite'), path.join(s.home, 'overseerd.log'), mockLog, path.join(t.workspace.path, '.git')].filter(f => fs.existsSync(f) && fs.statSync(f).isFile()).some(f => fs.readFileSync(f, 'utf8').includes('test-token'))
      || git(repo, 'config', '--list').includes('extraheader');
    check('no automatic merge; the token is not stored in Overseer, the logs or git config', git(repo, 'rev-parse', 'main') === mainBefore && !tokenLeak, { mainBefore, tokenLeak });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    mock.kill();
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
