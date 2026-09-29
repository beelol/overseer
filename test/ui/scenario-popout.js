// Packaged-UI scenario for AC-251 (follow an agent on another screen), fixture agents only.
// An agent keeps editing a file. Its review sits beside its chat. "Overseer: Pop Out Follow into Its
// Own Window" moves the review into a separate VS Code window (VS Code's floating editor windows);
// the main window keeps Overseer and the chat. The separate window keeps updating as the agent edits
// (two screenshots of it seconds apart). Selecting another agent makes that window follow it.
// "Return Follow to the Main Window" brings it back, and closing the separate window also returns
// the review beside the chat. The separate window, like the test window, never becomes the active
// app (the background launch creates it hidden and shows it inactive; test/ui/quiet-launch.js).
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('popout');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  // Which app is frontmost, four times a second, for the whole scenario (the owner's screen).
  const front = {};
  const sampler = setInterval(() => { try { const n = cp.execFileSync('lsappinfo', ['info', '-only', 'name', cp.execFileSync('lsappinfo', ['front'], { encoding: 'utf8' }).trim()], { encoding: 'utf8' }).trim().replace(/^.*=/, '').replace(/"/g, ''); front[n] = (front[n] || 0) + 1; } catch {} }, 250);
  try {
    const repo = makeRepo(path.join(s.root, 'pop-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, {});
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const other = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L5: original$/L5: other agent/' a.txt"], prompt: '', title: 'Other agent' });
    const live = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'i=0; while [ $i -lt 400 ]; do i=$((i+1)); echo "step $i" >> live.txt; sleep 1; done'], prompt: '', title: 'Live edits' });
    await delay(2500);
    const windows = () => s.quiet.main(`require('electron').BrowserWindow.getAllWindows().map(w => ({ id: w.id, title: w.getTitle(), visible: w.isVisible(), focused: w.isFocused() }))`);
    const mainTabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent).map(g => ({ tabs: [...g.querySelectorAll('.tab')].map(t => (t.getAttribute('aria-label') || '').split(',')[0]), active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(',')[0] }))`);
    // The separate window's page (its own CDP target), for its screenshots and DOM.
    let auxSession;
    const attachAux = async () => {
      const { targetInfos } = await cdp.call('Target.getTargets');
      const page = targetInfos.find(t => t.type === 'page' && !/workbench\.html/.test(t.url) && !/^devtools/.test(t.url) && !/vscode-webview/.test(t.url));
      if (!page) return null;
      auxSession = (await cdp.call('Target.attachToTarget', { targetId: page.targetId, flatten: true })).sessionId;
      await cdp.call('Runtime.enable', {}, auxSession); await cdp.call('Page.enable', {}, auxSession).catch(() => {});
      return auxSession;
    };
    const auxEval = async expr => (await cdp.call('Runtime.evaluate', { expression: expr, returnByValue: true }, auxSession)).result?.value;
    const auxShot = async label => { const { data } = await cdp.call('Page.captureScreenshot', { format: 'png' }, auxSession); const f = path.join(s.evidence, `${String(++s.shot).padStart(2, '0')}-${label}.png`); fs.writeFileSync(f, Buffer.from(data, 'base64')); s.note('screenshot ' + path.relative(path.resolve(__dirname, '../..'), f)); };
    const reviewOf = title => cdp.webview(`!!document.getElementById('diffs') && document.getElementById('comparison')?.textContent === ${JSON.stringify(title)}`, 20000).catch(() => null);
    const steps = t => (String(t).match(/step \d+/g) || []).length;

    await s.selectRun(live.run.id);
    await reviewOf('Live edits');
    const t0 = await mainTabs();
    check('the working agent\'s review sits beside its chat in the main window', t0.length === 2 && /^Review: Live edits/.test(t0[0].active) && t0[1].active === 'Overseer', t0);
    await s.screenshot('review-beside-chat');

    await cdp.command('Overseer: Pop Out Follow into Its Own Window');
    let w1 = [];
    for (let i = 0; i < 40; i++) { w1 = await windows(); if (w1.length === 2) break; await delay(250); }
    await delay(1500);
    await attachAux();
    const t1 = await mainTabs();
    const auxTabs = auxSession ? await auxEval(`[...document.querySelectorAll('.tab')].map(t => (t.getAttribute('aria-label') || '').split(',')[0])`) : null;
    check('Pop Out Follow moves the review into a separate window', w1.length === 2 && w1.some(w => /^Review: Live edits/.test(w.title)) && auxTabs && auxTabs.some(t => /^Review: Live edits/.test(t)), { windows: w1, auxTabs });
    check('the main window keeps Overseer and the chat (no review there)', t1.flatMap(g => g.tabs).every(t => !/^Review/.test(t)) && t1.some(g => g.active === 'Overseer'), t1);
    await s.screenshot('main-window-after-pop-out');
    const r1 = await reviewOf('Live edits');
    const text1 = r1 ? await r1.eval(`document.getElementById('diffs').innerText`) : '';
    await auxShot('separate-window-1');
    await delay(4000);
    const r2 = await reviewOf('Live edits');
    const text2 = r2 ? await r2.eval(`document.getElementById('diffs').innerText`) : '';
    await auxShot('separate-window-2');
    check('the separate window keeps following the agent live (two screenshots seconds apart; its file grew)', steps(text2) > steps(text1) && steps(text1) > 0, { first: steps(text1), later: steps(text2) });

    // Selecting another agent: the separate window follows it.
    await s.selectRun(other.run.id);
    let w2 = [];
    for (let i = 0; i < 40; i++) { w2 = await windows(); if (w2.some(w => /^Review: Other agent/.test(w.title))) break; await delay(250); }
    const t2 = await mainTabs();
    check('selecting another agent: the separate window shows its review; the main window its chat', w2.length === 2 && w2.some(w => /^Review: Other agent/.test(w.title)) && t2.flatMap(g => g.tabs).every(t => !/^Review/.test(t)), { windows: w2, main: t2 });
    await s.selectRun(live.run.id);
    for (let i = 0; i < 40; i++) { if ((await windows()).some(w => /^Review: Live edits/.test(w.title))) break; await delay(250); }

    // Return Follow to the Main Window: the review is back beside the chat; the separate window closes.
    await cdp.command('Overseer: Return Follow to the Main Window');
    let w3 = [], t3 = [];
    for (let i = 0; i < 40; i++) { w3 = await windows(); t3 = await mainTabs(); if (w3.length === 1 && t3.some(g => /^Review: Live edits/.test(g.active))) break; await delay(250); }
    check('Return Follow brings the review back beside the chat and its window closes', w3.length === 1 && t3.length === 2 && /^Review: Live edits/.test(t3[0].active) && t3[1].active === 'Overseer', { windows: w3, main: t3 });
    await s.screenshot('returned');

    // Closing the separate window (its close button) also returns the review.
    await cdp.command('Overseer: Pop Out Follow into Its Own Window');
    for (let i = 0; i < 40; i++) { if ((await windows()).length === 2) break; await delay(250); }
    await delay(1000);
    await s.quiet.main(`(() => { const w = require('electron').BrowserWindow.getAllWindows().find(w => /^Review:/.test(w.getTitle())); if (w) w.close(); return !!w; })()`);
    let w4 = [], t4 = [];
    for (let i = 0; i < 40; i++) { w4 = await windows(); t4 = await mainTabs(); if (w4.length === 1 && t4.some(g => /^Review: Live edits/.test(g.active))) break; await delay(250); }
    check('closing the separate window returns the review to the main window beside the chat', w4.length === 1 && t4.length === 2 && /^Review: Live edits/.test(t4[0].active) && t4[1].active === 'Overseer', { windows: w4, main: t4 });
    await s.screenshot('closed-window-returned');

    const events = await s.quiet.activations();
    const activated = events.filter(e => /became active|window focused|webContents\.focus/.test(e.what));
    check('the separate window never took the focus from the owner\'s apps (main-process events)', activated.length === 0, { events });
    s.ctl('run.interrupt', { run_id: live.run.id });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    clearInterval(sampler);
    s.note('frontmost app samples (four a second)', front);
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify({ ...result, frontmost: front }, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
