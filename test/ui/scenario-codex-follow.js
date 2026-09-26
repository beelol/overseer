// LIVE paid scenario (one small Codex turn through the app-server transport): the agent
// alternates edits between two files at distant lines while the packaged UI Follows them;
// scrolling pauses Follow (view stays put during further live edits); Resume restarts it.
// DRY_RUN=1 is not supported here: the point is live model edits.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const EDITS = [['a.txt', 20], ['b.txt', 280], ['a.txt', 60], ['b.txt', 240], ['a.txt', 150], ['b.txt', 200], ['a.txt', 100], ['b.txt', 30]];
const PROMPT = 'Edit files one at a time, one patch per step, in exactly this order and nothing else: ' +
  EDITS.map(([f, l], i) => `(${i + 1}) in ${f} change the line "L${l}: original" to "L${l}: codex edit ${i + 1}"`).join('; ') +
  '. Do not read other files, run commands or tests. When finished reply exactly: done';

(async () => {
  const s = new Session('codex-follow-live');
  const result = { checks: [], reveals: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'repo'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer [0-9]+ active/.test(e.textContent))`, 60000, 'status bar');
    await cdp.command('Overseer: Start Task with Quick Picks');
    await cdp.pick('New task: repository');
    await cdp.pick('New task: harness', 'codex-app');
    await cdp.pick('New task: account for', 'codex (existing login)');
    await cdp.pick('New task: workspace');
    await cdp.pick('Start the worktree from');
    await cdp.input('Model (optional)', 'gpt-5.6-luna');
    await cdp.pick('Codex approval policy', 'never');
    await cdp.input('Task prompt', PROMPT);
    const review = await cdp.webview('!!document.getElementById("diffs") && !!document.getElementById("follow")', 60000);
    const run = s.ctl('state').runs.find(r => r.harness === 'codex-app');
    check('follow on for launched run', await review.waitFor(`(document.getElementById('follow').dataset.state !== 'off')`, 15000));
    const seen = [];
    let pausedAt, pausedTop, paused = false, resumed = false, afterResume = [];
    const end = Date.now() + 360000;
    while (Date.now() < end) {
      const st = await review.eval(`({ text: document.getElementById('follow-state').textContent, top: document.getElementById('diffs').scrollTop, resume: (document.getElementById('follow').dataset.state === 'paused') })`);
      if (st.text.startsWith('Following:') && seen[seen.length - 1] !== st.text) {
        seen.push(st.text); s.note('follow', st.text);
        if (resumed) afterResume.push(st.text);
        if (seen.length === 2) await s.screenshot('following');
      }
      if (!paused && seen.length >= 2) {
        const pt = await s.webviewPoint(review, '#diffs');
        await cdp.wheel(pt.x, pt.y + 100, 500);
        await delay(400);
        const p = await review.eval(`({ text: document.getElementById('follow-state').textContent, top: document.getElementById('diffs').scrollTop, resume: (document.getElementById('follow').dataset.state === 'paused') })`);
        check('scroll pauses Follow during live edits', p.resume && /paused/i.test(p.text), p);
        paused = true; pausedAt = Date.now(); pausedTop = p.top;
        result.editsAtPause = s.ctl('events.list', { run_id: run.id, limit: 5000 }).events.filter(e => e.kind === 'file_activity').length;
        await s.screenshot('paused');
      }
      const runNow = s.ctl('state').runs.find(r => r.id === run.id);
      const finished = !['queued', 'starting', 'running', 'waiting_for_user'].includes(runNow.status);
      if (paused && !resumed && (Date.now() - pausedAt > 5000 || finished)) {
        const top = await review.eval(`document.getElementById('diffs').scrollTop`);
        const edits = s.ctl('events.list', { run_id: run.id, limit: 5000 }).events.filter(e => e.kind === 'file_activity').length;
        check('paused view stays put while the agent keeps editing', Math.abs(top - pausedTop) < 2 && edits > result.editsAtPause, { pausedTop, top, editsAtPause: result.editsAtPause, editsNow: edits });
        const r = await s.webviewPoint(review, '#follow');
        await cdp.click(r.x, r.y);
        await delay(1200);
        const after = await review.eval(`({ text: document.getElementById('follow-state').textContent, top: document.getElementById('diffs').scrollTop })`);
        check('Resume jumps to the latest agent edit', /^Following: /.test(after.text) && after.text !== seen[seen.length - 1] && Math.abs(after.top - top) > 2, after);
        resumed = true;
      }
      if (finished && (!paused || resumed)) { await delay(1500); break; }
      await delay(200);
    }
    const final = s.ctl('state').runs.find(r => r.id === run.id);
    result.reveals = seen;
    const files = new Set(seen.map(t => (t.match(/Following: (\S+?):/) || [])[1]));
    const lines = new Set(seen.map(t => (t.match(/:(\d+) /) || [])[1]));
    check('live run completed', final.status === 'completed', { status: final.status, reason: final.exit_reason });
    check('Follow crossed files on live edits', files.has('a.txt') && files.has('b.txt'), seen);
    check('Follow revealed distant lines within files', lines.size >= 3, [...lines]);
    check('attribution is agent-reported', seen.every(t => /agent-reported/.test(t)), seen[0]);
    check('pause/resume exercised during the live run', paused && resumed, { paused, resumed });
    const ws = s.ctl('state').workspaces.find(w => w.id === run.workspace_id).path;
    const a = fs.readFileSync(path.join(ws, 'a.txt'), 'utf8'), b = fs.readFileSync(path.join(ws, 'b.txt'), 'utf8');
    result.appliedEdits = EDITS.map(([f, l], i) => (f === 'a.txt' ? a : b).includes(`L${l}: codex edit ${i + 1}`));
    await s.screenshot('completed');
    const ids = s.ctl('profile.status', { id: 'system-codex' });
    result.identity = { plan: ids.identity && ids.identity.plan, account: ids.identity && ids.identity.account_fingerprint, version: ids.version };
    result.usage = s.ctl('events.list', { run_id: run.id, limit: 5000 }).events.filter(e => e.kind === 'usage').map(e => e.payload).slice(-2);
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    await s.quit(); s.stopDaemon();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
