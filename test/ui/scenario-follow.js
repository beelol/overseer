// Packaged-UI scenario for AC-74 (follow or manual review), mock OpenCode (no paid tokens): an
// agent edits a.txt, b.txt and c.txt in turn at distant lines, 2 s apart. Its first edit brings
// the review forward following it; follow mode moves with each edit across the three files; one
// icon in the review header switches to manual, after which the file and scroll position stay
// exactly where they are (measured) while the agent keeps editing; the choice is remembered per
// agent (another agent starts in its own mode; coming back finds manual still set).
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, startMock, openCodeConfig, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('follow');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mock = startMock(s.root, { MOCK_STEP_DELAY_MS: '2000' });
  try {
    const repo = makeRepo(path.join(s.root, 'follow-repo'), { dirty: false });
    // A third file with the same numbered lines as a.txt and b.txt.
    fs.writeFileSync(path.join(repo, 'c.txt'), fs.readFileSync(path.join(repo, 'a.txt')));
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', 'three files');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const profile = s.ctl('profile.create', { name: 'OpenCode mock', harness: 'opencode' });
    fs.mkdirSync(path.join(profile.home, 'config/opencode'), { recursive: true });
    fs.writeFileSync(path.join(profile.home, 'config/opencode/opencode.json'), openCodeConfig(await mock.port()));
    const other = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L7: original$/L7: other agent/' a.txt"], prompt: '', title: 'Other agent' });
    for (let i = 0; i < 30 && s.ctl('state').runs.find(r => r.id === other.run.id).status !== 'completed'; i++) await delay(300);
    const task = s.ctl('task.create', { repo, harness: 'opencode', profile_id: profile.id, model: 'mock/mock-coder', prompt: 'sequence3 15', title: 'Three files' });
    const switchTo = async title => { await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type(title); await delay(300); await cdp.key('Enter'); await delay(1500); };
    await switchTo('Three files');
    // The first edit brings the review forward, following.
    const review = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(task.run.id)}`, 60000);
    const follow = () => review.eval(`({ state: document.getElementById('follow').dataset.state, pressed: document.getElementById('follow').getAttribute('aria-pressed'), name: document.getElementById('follow').getAttribute('aria-label'),
      text: document.getElementById('follow-state').textContent, top: Math.round(document.getElementById('diffs').scrollTop), active: document.querySelector('#tree .file.active, #tree .file[aria-selected="true"]')?.textContent.trim() || '' })`);
    await review.waitFor(`document.getElementById('follow').dataset.state === 'following'`, 30000);
    const seen = [];
    for (let i = 0; i < 90 && new Set(seen.map(t => (t.match(/Following: (\w\.txt)/) || [])[1]).filter(Boolean)).size < 3; i++) {
      const f = await follow(); if (seen[seen.length - 1] !== f.text) seen.push(f.text); await delay(250);
    }
    const files = new Set(seen.map(t => (t.match(/Following: (\w\.txt)/) || [])[1]).filter(Boolean));
    const f1 = await follow();
    await s.screenshot('following');
    check('follow mode (the default when the agent\'s edits bring the review in) moves with each edit across a.txt, b.txt and c.txt',
      f1.state === 'following' && f1.pressed === 'true' && files.size === 3, { files: [...files], seen: seen.slice(0, 8), icon: f1 });

    // One icon switches to manual: file and scroll stay put while the agent keeps editing.
    const icon = await s.webviewPoint(review, '#follow'); await cdp.click(icon.x, icon.y); await delay(700);
    const m0 = await follow();
    const edits0 = s.ctl('events.list', { run_id: task.run.id, limit: 2000 }).events.filter(e => e.kind === 'file_activity').length;
    const samples = [];
    for (let i = 0; i < 24; i++) { const f = await follow(); samples.push(f.top); await delay(250); }
    const edits1 = s.ctl('events.list', { run_id: task.run.id, limit: 2000 }).events.filter(e => e.kind === 'file_activity').length;
    await s.screenshot('manual');
    check('one icon switches to manual; while the agent keeps editing, the scroll position does not move',
      m0.state === 'off' && m0.pressed === 'false' && /Follow the agent/.test(m0.name) && edits1 > edits0 && samples.every(t => t === m0.top), { icon: m0, editsDuring: edits1 - edits0, scroll: [...new Set(samples)] });

    // Remembered per agent: another agent has its own mode; coming back, this one is still manual.
    await switchTo('Other agent');
    const otherReview = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(other.run.id)}`, 30000);
    const otherState = await otherReview.eval(`document.getElementById('follow').dataset.state`);
    await switchTo('Three files');
    const back = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(task.run.id)}`, 30000);
    await delay(800);
    const backState = await back.eval(`document.getElementById('follow').dataset.state`);
    check('the mode is remembered per agent (the other agent keeps its own; this one is still manual)', otherState === 'off' && backState === 'off', { otherState, backState });
    // And following again is one click.
    const icon2 = await s.webviewPoint(back, '#follow'); await cdp.click(icon2.x, icon2.y); await delay(700);
    check('one click turns Follow back on', (await back.eval(`document.getElementById('follow').dataset.state`)) === 'following');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); mock.child.kill(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
