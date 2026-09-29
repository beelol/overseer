// Packaged-UI scenario for AC-259 and AC-260 (Claude Code fixture, no paid turns), keyboard only
// from the composer.
// AC-259: a task typed at home and sent with Enter clears the field at once and shows a sent
// confirmation within one second, before the field returns to "Send off a task".
// AC-260: the repository chip's own picker (inside the webview) lists recent repositories, filters
// them fuzzily, completes a typed path with Tab and adds a not-yet-open repository from its path;
// the task then starts there. VS Code's in-window file dialog is switched on for this profile, so
// any Open Folder request would show inside the window (never a native macOS dialog) and is
// recorded: the scenario checks none was opened.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('composer-friction');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'friction-site'), { dirty: false });
    // A second repository nobody has opened: only its path is known to the owner.
    const other = makeRepo(path.join(s.root, 'elsewhere', 'notes-repo'), { dirty: false });
    fs.mkdirSync(path.join(s.root, 'not-a-repo'), { recursive: true });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.home.sendTo': 'agent', 'overseer.followNewRuns': false, 'files.simpleDialog.enable': true });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    // Any file dialog (in-window with files.simpleDialog.enable) is recorded from here on.
    await cdp.evalWorkbench(`(() => { window.__dialogs = []; new MutationObserver(() => { for (const w of document.querySelectorAll('.quick-input-widget')) { if (w.style.display === 'none' || !w.offsetParent) continue; const t = w.querySelector('.quick-input-title')?.textContent || ''; const v = w.querySelector('input')?.value || ''; if (/Repository for the agent|Open Folder|^\\//.test(t + ' ' + v) && !window.__dialogs.includes(t)) window.__dialogs.push(t || v); } }).observe(document.body, { subtree: true, childList: true, attributes: true }); return true; })()`);
    const runs = () => s.ctl('state').runs.filter(r => !r.parent_run_id);
    const tasks = () => s.ctl('state').tasks;
    const key = async (k, o) => { await cdp.key(k, o); await delay(120); };

    await cdp.command('Overseer: New Agent'); await delay(800);
    const home = await s.editorView(`document.body.dataset.mode === 'composer' && !!document.querySelector('.view-composer:not([hidden]) #task')`);
    await home.waitFor(`!document.querySelector('[data-chip="repo"]').textContent.includes('Loading') && document.querySelector('[data-chip="agent"]').textContent.includes('Claude')`, 20000);
    const focusTask = async () => {
      if (await home.eval(`document.activeElement?.id === 'task' && document.hasFocus()`)) return;
      await cdp.command('Overseer: New Agent'); await delay(800);
      await home.waitFor(`document.activeElement?.id === 'task'`, 5000);
    };
    const tabTo = async (pred, { back = false, max = 20 } = {}) => {
      for (let i = 0; i < max; i++) {
        if (await home.eval(`(() => { const a = document.activeElement; return !!a && (${pred}); })()`)) return true;
        await key('Tab', { shift: back });
      }
      throw new Error('focus never reached ' + pred);
    };
    await focusTask();
    check('the composer takes the keyboard when home opens', true);

    // ---------- AC-259: sending clears the field and says so ----------
    // A recorder samples the field every animation frame from just before Enter.
    const record = () => home.eval(`(() => { const t = document.getElementById('task'), n = document.querySelector('.view-composer .composer-note'), box = t.closest('.composer');
      window.__t0 = performance.now(); window.__samples = [];
      const tick = () => { window.__samples.push({ t: Math.round(performance.now() - window.__t0), value: t.value, placeholder: t.placeholder, note: n.textContent, sent: box.dataset.sent || '' }); if (performance.now() - window.__t0 < 8000) requestAnimationFrame(tick); };
      tick(); return true; })()`);
    const text = 'Tidy the pricing copy';
    await cdp.type(text); await delay(200);
    const before = runs().length;
    await record();
    await key('Enter');
    // While the agent is starting, home still shows: the cleared field says it was sent. Once the
    // agent starts, the view moves to its chat (followNewRuns only decides Follow, not the view).
    await s.screenshot('sent-confirmation');
    let run; for (let i = 0; i < 40 && !run; i++) { await delay(250); run = runs().slice(before).find(r => r.title === text || r.harness === 'claude'); }
    await delay(5000);
    const samples = await home.eval(`window.__samples`);
    const cleared = samples.find(x => x.value === '');
    const confirmed = samples.find(x => /sent/i.test(x.placeholder + ' ' + x.note) && x.sent);
    const back = confirmed && samples.find(x => x.t > confirmed.t && x.placeholder === 'Send off a task' && !x.sent);
    const stayedEmpty = cleared && samples.filter(x => x.t >= cleared.t).every(x => x.value === '');
    check('AC-259: Enter starts the agent', !!run, run && { id: run.id, title: run.title });
    check('AC-259: the field is empty within one second of Enter and stays empty', cleared && cleared.t <= 1000 && stayedEmpty, { cleared: cleared && cleared.t });
    check('AC-259: a sent confirmation appears within one second', confirmed && confirmed.t <= 1000, confirmed);
    check('AC-259: the field then returns to "Send off a task"', !!back, back);

    // ---------- AC-260: another repository without a native dialog ----------
    await focusTask();
    await tabTo(`a.matches('[data-chip="repo"]')`);
    await key('Enter');
    const opened = await home.waitFor(`document.activeElement?.id === 'repo-query' && !!document.querySelector('.repo-picker')`, 5000).then(() => true, () => false);
    const listed = await home.eval(`[...document.querySelectorAll('.repo-picker [role="option"]')].map(o => o.textContent)`);
    check('AC-260: Enter on the repository chip opens its own picker, typing goes to its search field', opened, listed);
    check('AC-260: the picker lists the recent repositories', listed.some(t => t.includes('friction-site')), listed);
    await s.screenshot('repo-picker');
    // Fuzzy: letters in order, not a prefix.
    await cdp.type('frst'); await delay(300);
    const fuzzy = await home.eval(`[...document.querySelectorAll('.repo-picker [role="option"]')].map(o => o.textContent)`);
    check('AC-260: the search is fuzzy (frst finds friction-site)', fuzzy.some(t => t.includes('friction-site')), fuzzy);
    for (let i = 0; i < 4; i++) await key('Backspace');
    // A folder that is not a Git repository is refused inside the picker, with no dialog.
    await cdp.type(path.join(s.root, 'not-a-repo')); await delay(300);
    await key('Enter');
    const refused = await home.waitFor(`(() => { const e = document.querySelector('.repo-picker .repo-picker-error'); return e && e.textContent; })()`, 8000).catch(() => null);
    check('AC-260: a typed folder that is not a Git repository is explained inside the picker', /Git repository/.test(refused || ''), refused);
    await home.eval(`(() => { const q = document.getElementById('repo-query'); q.value = ''; q.dispatchEvent(new Event('input')); return true; })()`);
    // Tab completes a typed path: "<root>/elsew" becomes "<root>/elsewhere/".
    await cdp.type(path.join(s.root, 'elsew')); await delay(200);
    await home.waitFor(`[...document.querySelectorAll('.repo-picker [role="option"]')].some(o => o.dataset.hint === ${JSON.stringify(path.join(s.root, 'elsewhere') + '/')})`, 8000);
    await key('Tab');
    const completed = await home.eval(`document.getElementById('repo-query').value`);
    check('AC-260: Tab completes a typed path inside the picker', completed === path.join(s.root, 'elsewhere') + '/', completed);
    await cdp.type('notes-repo'); await delay(300);
    await s.screenshot('typed-path');
    await key('Enter');
    const chosen = await home.waitFor(`(() => { const c = document.querySelector('[data-chip="repo"]'); return !document.querySelector('.repo-picker') && /notes-repo/.test(c.getAttribute('aria-label')) && document.activeElement?.id === 'task' && c.getAttribute('aria-label'); })()`, 10000).catch(() => null);
    check('AC-260: Enter adds the typed repository, the chip shows it and the keyboard is back in the task', !!chosen, chosen);
    const before2 = runs().length;
    await cdp.type('Summarise the notes'); await delay(200);
    await key('Enter');
    let run2; for (let i = 0; i < 40 && !run2; i++) { await delay(250); run2 = runs().slice(before2)[0]; }
    const task2 = run2 && tasks().find(t => t.id === run2.task_id);
    check('AC-260: the task starts in the repository that was not open', task2 && fs.realpathSync(task2.repo_root) === fs.realpathSync(other), task2 && task2.repo_root);
    // The added repository is remembered: it is listed next time, and the search finds it.
    await delay(3000);
    await focusTask();
    await tabTo(`a.matches('[data-chip="repo"]')`);
    await key('Enter');
    await home.waitFor(`document.activeElement?.id === 'repo-query'`, 5000);
    await cdp.type('notes'); await delay(300);
    const remembered = await home.eval(`[...document.querySelectorAll('.repo-picker [role="option"]')].map(o => o.textContent)`);
    check('AC-260: the added repository is now a known repository the search finds', remembered.some(t => t.includes('notes-repo')), remembered);
    await key('Escape');
    const escaped = await home.eval(`!document.querySelector('.repo-picker') && document.activeElement?.dataset?.chip === 'repo'`);
    check('AC-260: Escape closes the picker and returns the keyboard to the chip', escaped);
    const browse = await (async () => { await key('Enter'); await home.waitFor(`!!document.querySelector('.repo-picker')`, 5000); const b = await home.eval(`[...document.querySelectorAll('.repo-picker [role="option"]')].some(o => /system dialog/i.test(o.textContent))`); await key('Escape'); return b; })();
    check('AC-260: the system folder dialog stays available, but only as a last option', browse);
    const dialogs = await cdp.evalWorkbench(`window.__dialogs`);
    check('AC-260: no folder dialog was opened at any point', Array.isArray(dialogs) && dialogs.length === 0, dialogs);
    await s.screenshot('done');
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
