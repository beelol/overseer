// Packaged-UI scenario for AC-72 (chat in the middle when there is nothing to review) and AC-73
// (changes bring the diff forward), fixture harnesses only. No agent selected: one editor group
// with the new-agent composer, no review. A Claude fixture agent waiting for permission (no
// changes yet): one group with its chat. Allowing it makes its first file edit: within 500 ms the
// editor area becomes the editable review on the left (about two thirds, on the changed file) and
// the chat on the right. Closing the review puts the chat back in the middle; selecting another
// agent with changes keeps that; Open Review brings the review back. No settings change; the
// arrangement survives a window reload (AC-49).
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('arrangement');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'arr-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    // VS Code migrates some of its own settings on start (extensions.autoUpdate false → "off"); compare after that.
    const norm = text => { const o = JSON.parse(text); if (o['extensions.autoUpdate'] === false) o['extensions.autoUpdate'] = 'off'; return JSON.stringify(o); };
    const settingsBefore = norm(fs.readFileSync(settingsFile, 'utf8'));
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 200) { if (re.test(state(id)?.status || '')) return; await delay(200); } };
    const layout = () => cdp.evalWorkbench(`(() => {
      const groups = [...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent);
      const total = groups.reduce((n, g) => n + g.getBoundingClientRect().width, 0);
      return groups.map(g => ({ share: Math.round(g.getBoundingClientRect().width / total * 100) / 100, active: g.querySelector('.tab.active')?.getAttribute('aria-label') || '', tabs: [...g.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label')) }));
    })()`);
    const kind = l => l.map(g => /^Review/.test(g.active) ? 'review' : /^Overseer/.test(g.active) ? 'chat' : g.active).join('+');

    // Nothing selected: the composer alone.
    await cdp.command('Overseer: New Agent'); await delay(1500);
    let dash = await cdp.webview(`document.body.dataset.ready === '1'`, 30000);
    let l0 = await layout();
    const composerShown = await dash.eval(`document.body.dataset.mode === 'composer' && !document.querySelector('.view-composer').hidden`);
    check('no agent selected: one editor group with the new-agent composer and no review', l0.length === 1 && kind(l0) === 'chat' && composerShown, l0);
    await s.screenshot('composer-alone');

    // A fresh agent with no changes: its chat alone.
    fs.writeFileSync(modeFile, 'permission');
    const perm = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', prompt: 'write perm.txt', title: 'Write perm.txt' });
    await waitStatus(perm.run.id, /waiting_for_user/);
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent');
    await cdp.type('Write perm.txt'); await delay(300); await cdp.key('Enter'); await delay(2000);
    const l1 = await layout();
    check('an agent with no changes: one editor group with its chat and no review', l1.length === 1 && kind(l1) === 'chat' && !l1[0].tabs.some(t => /^Review/.test(t)), l1);
    await s.screenshot('chat-alone');

    // Its first edit brings the review forward within 500 ms.
    await cdp.command('Overseer: Allow Pending Request');
    let switched, l2; const t0 = Date.now();
    for (let i = 0; i < 400; i++) { l2 = await layout(); if (kind(l2) === 'review+chat') { switched = Date.now(); break; } await delay(20); }
    // The edit is the file's write (the fixture names the file earlier, when it asks for permission).
    const written = fs.statSync(path.join(perm.workspace.path, 'perm.txt')).mtimeMs;
    const lag = switched ? Math.round(switched - written) : undefined;
    await delay(1500); l2 = await layout();
    const reviewFrame = await cdp.webview(`!!document.getElementById('diffs')`, 20000).catch(() => null);
    const reviewFiles = reviewFrame ? await reviewFrame.waitFor(`document.getElementById('tree')?.innerText.includes('perm.txt') && document.getElementById('tree').innerText.split('\\n').map(x => x.trim()).filter(Boolean)`, 10000).catch(() => []) : [];
    check('the first file edit brings the review forward within 500 ms: review on the left (about two thirds) on the changed file, chat on the right',
      kind(l2) === 'review+chat' && l2[0].share >= 0.6 && l2[0].share <= 0.72 && lag !== undefined && lag <= 500 && reviewFiles.some(f => /perm\.txt/.test(f)), { lag, waitedMs: switched - t0, layout: l2, reviewFiles });
    await s.screenshot('review-and-chat');

    // A second agent that already has changes (for the "keeps the arrangement" check).
    fs.writeFileSync(modeFile, 'showcase');
    const other = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', prompt: 'Make sessions refresh once.', title: 'Refresh sessions once' });
    await waitStatus(other.run.id, /completed/);

    // Closing the review puts the chat back in the middle.
    const closePt = await cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => /^Review/.test(t.getAttribute('aria-label') || '')); const c = t?.querySelector('.codicon-close, .tab-actions .action-label'); const b = (c || t).getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    await cdp.click(closePt.x, closePt.y); await delay(1500);
    const l3 = await layout();
    const chatTab = await cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => /^Overseer/.test(t.getAttribute('aria-label') || '')); return t && { cls: t.className, italic: getComputedStyle(t.querySelector('.label-name') || t).fontStyle }; })()`);
    check('the chat is a normal tab after moving (not a preview another file would replace)', chatTab && !/\bpreview\b/.test(chatTab.cls) && chatTab.italic !== 'italic', chatTab);
    check('closing the review returns the chat to the middle (one group)', l3.length === 1 && kind(l3) === 'chat', l3);
    // Selecting an agent with changes keeps the chat-only choice.
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent');
    await cdp.type('Refresh sessions once'); await delay(300); await cdp.key('Enter'); await delay(2000);
    const l4 = await layout();
    check('selecting another agent (with changes) keeps the chosen arrangement: chat alone', l4.length === 1 && kind(l4) === 'chat', l4);
    // Open Review brings the review back.
    await cdp.command('Overseer: Open Review'); await delay(2000);
    const l5 = await layout();
    check('Open Review brings the review back beside the chat', kind(l5) === 'review+chat', l5);

    // Survives a reload; no settings changed.
    await cdp.command('Developer: Reload Window'); await delay(7000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'after reload');
    await delay(4000);
    const l6 = await layout();
    check('the arrangement survives a window reload (review left, chat right)', kind(l6) === 'review+chat' && l6[0].share >= 0.6, l6);
    await s.screenshot('after-reload');
    check('no settings changed (apart from VS Code\'s own migration)', norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
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
