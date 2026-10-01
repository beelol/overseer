// Packaged-UI scenario for AC-263 (the review opens on "Since task start", with the other
// comparisons one click away), with generic fixture agents in a disposable repository (no paid turns).
// 1. An agent in its own worktree commits one.txt in its first turn and leaves an untracked two.txt
//    in its second; main moves on afterwards. Its review opens on Since task start (both files);
//    one click shows Latest run (two.txt) and one Entire worktree (its branch against the commit
//    it started from: both files, not main's later commit).
// 2. An agent in the owner's checkout, on a feature branch with a commit of its own and an edit the
//    owner left before the task: the agent writes c1.txt, the owner writes owner.txt, the agent
//    writes c2.txt. Its review opens on Since task start (c1, c2 and owner.txt) and says it includes
//    any edits made in this folder; Latest run is c2.txt; Entire worktree is the feature branch
//    against main (feature.txt and a.txt too).
// Each time the header names the comparison shown (the pressed button) and the file count matches.
// 3. The agent's changes are Accepted or Rejected, per change and per file, never Keep, Undo or Save
//    (the owner, 2026-09-29): an agent edits two lines of a.txt and two of b.txt. One change of a.txt
//    is accepted (it reads Accepted) and one rejected (the file on disk has the line back); all of
//    b.txt is accepted (Accepted) and then rejected (b.txt is as before, and leaves the review).
//    "Save your edits" shows only once the owner types, and saving writes what they typed.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('review-compare');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'shop'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async id => { for (let i = 0; i < 80 && !['completed', 'failed', 'interrupted'].includes(runState(id).status); i++) await delay(250); return runState(id); };
    const turn = async id => { s.ctl('run.follow_up', { run_id: id, prompt: '' }); await delay(500); return waitDone(id); };
    const agent = (title, mode, script) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title, workspace_mode: mode });
    const click = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(400); };
    const chatOf = runId => cdp.webview(`window.__overseer?.selected?.() === ${JSON.stringify(runId)} && !!document.getElementById('review')`, 30000);
    // What the review shows once it has settled on `mode` with `count` files.
    const shown = (frame, mode, count) => frame.waitFor(`(() => {
      const bar = document.getElementById('compare'); if (!bar || bar.hidden || bar.dataset.mode !== ${JSON.stringify(mode)} || document.body.dataset.checking !== 'false') return null;
      const files = [...document.querySelectorAll('.diff-file:not(.browsed) .file-path')].map(e => e.textContent).sort();
      if (files.length !== ${count}) return null;
      const pressed = [...bar.querySelectorAll('button[aria-pressed="true"]')];
      return { files, count: document.getElementById('total').dataset.count, total: document.getElementById('total').textContent, label: bar.dataset.label,
        pressed: pressed.map(b => b.textContent.trim()), pressedName: pressed.map(b => b.getAttribute('aria-label')), note: document.getElementById('compare-note').textContent,
        buttons: [...bar.querySelectorAll('button.cmp')].filter(b => !b.hidden).map(b => b.textContent.trim()), more: document.getElementById('base-label').textContent }; })()`, 30000).catch(async () => ({ timedOut: true, now: await frame.eval(`({ mode: document.getElementById('compare')?.dataset.mode, checking: document.body.dataset.checking, files: [...document.querySelectorAll('.diff-file .file-path')].map(e => e.textContent) })`).catch(e => e.message) }));
    const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
    const reviewOf = async (title, runId) => {
      await s.selectAgent(title, { settle: 2000 });
      const chat = await chatOf(runId);
      await click(chat, '#review');
      return cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('diffs') && !document.getElementById('compare').hidden`, 60000);
    };
    const choose = async (frame, mode) => { await frame.eval(`document.querySelector('#compare button.cmp[data-mode="${mode}"]').id = 'cmp-target'`); await click(frame, '#cmp-target'); await frame.eval(`document.getElementById('cmp-target')?.removeAttribute('id')`); };

    // ---- 1. An agent in its own worktree.
    const w = agent('Worktree agent', 'worktree', "if [ -f one.txt ]; then printf 'two\\n' > two.txt; else printf 'one\\n' > one.txt && git add one.txt && git -c user.name=A -c user.email=a@x.invalid commit -qm one; fi");
    await waitDone(w.run.id);
    await turn(w.run.id);
    fs.writeFileSync(path.join(repo, 'main-later.txt'), 'later\n'); git(repo, 'add', 'main-later.txt'); git(repo, 'commit', '-qm', 'main later');
    check('fixture: turn 1 committed one.txt, turn 2 left two.txt untracked, main moved on', git(w.workspace.path, 'log', '-1', '--format=%s') === 'one' && git(w.workspace.path, 'status', '--porcelain') === '?? two.txt', git(w.workspace.path, 'status', '--porcelain'));
    const wr = await reviewOf('Worktree agent', w.run.id);
    const w1 = await shown(wr, 'task_start', 2);
    await s.screenshot('worktree-since-task-start');
    check('a finished agent in its own worktree: the review opens on Since task start (one.txt and two.txt, 2 files)', same(w1.files, ['one.txt', 'two.txt']) && w1.count === '2' && /^2 files/.test(w1.total), w1);
    check('the header names it: Since task start pressed, Latest run and Entire worktree one click away, no folder note', same(w1.pressed, ['Since task start']) && same(w1.buttons, ['Since task start', 'Latest run', 'Entire worktree']) && w1.more === 'More…' && w1.note === '', w1);
    await choose(wr, 'latest_run');
    const w2 = await shown(wr, 'latest_run', 1);
    await s.screenshot('worktree-latest-run');
    check('one click: Latest run (two.txt, 1 file), named in the header', same(w2.files, ['two.txt']) && w2.count === '1' && same(w2.pressed, ['Latest run']) && w2.label === 'Latest run', w2);
    await choose(wr, 'entire_worktree');
    const w3 = await shown(wr, 'entire_worktree', 2);
    await s.screenshot('worktree-entire-worktree');
    check("one click: Entire worktree (the branch against where it started: one.txt and two.txt, not main's later commit), named in the header", same(w3.files, ['one.txt', 'two.txt']) && w3.count === '2' && same(w3.pressed, ['Entire worktree']) && w3.label === 'Entire worktree', w3);
    await choose(wr, 'task_start');
    const w4 = await shown(wr, 'task_start', 2);
    check('one click back to Since task start', same(w4.pressed, ['Since task start']), w4);

    // ---- 2. An agent in the owner's checkout, on a feature branch.
    git(repo, 'switch', '-q', '-c', 'feature');
    fs.writeFileSync(path.join(repo, 'feature.txt'), 'feature\n'); git(repo, 'add', 'feature.txt'); git(repo, 'commit', '-qm', 'feature');
    fs.writeFileSync(path.join(repo, 'a.txt'), fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').replace('L3: original', 'L3: the owner, before the task'));
    const c = agent('Checkout agent', 'current', "if [ -f c2.txt ]; then :; elif [ -f c1.txt ]; then printf 'c2\\n' > c2.txt; else printf 'c1\\n' > c1.txt; fi");
    await waitDone(c.run.id);
    fs.writeFileSync(path.join(repo, 'owner.txt'), "the owner's own edit\n");
    await turn(c.run.id);
    const cr = await reviewOf('Checkout agent', c.run.id);
    const c1 = await shown(cr, 'task_start', 3);
    await s.screenshot('checkout-since-task-start');
    check("a finished agent in the owner's checkout: the review opens on Since task start (c1, c2 and the owner's edit between turns; not the edit before the task)", same(c1.files, ['c1.txt', 'c2.txt', 'owner.txt']) && c1.count === '3', c1);
    check('the header says it includes any edits made in this folder', same(c1.pressed, ['Since task start']) && c1.note === '(includes any edits made in this folder)' && same(c1.pressedName, ['Since task start (includes any edits made in this folder)']), c1);
    await choose(cr, 'latest_run');
    const c2 = await shown(cr, 'latest_run', 1);
    await s.screenshot('checkout-latest-run');
    check('one click: Latest run (c2.txt, 1 file), named in the header', same(c2.files, ['c2.txt']) && c2.count === '1' && same(c2.pressed, ['Latest run']) && c2.note === '(includes any edits made in this folder)', c2);
    await choose(cr, 'entire_worktree');
    const c3 = await shown(cr, 'entire_worktree', 5);
    await s.screenshot('checkout-entire-worktree');
    check('one click: Entire worktree (the feature branch against main: a.txt, c1, c2, feature.txt, owner.txt, 5 files), named in the header', same(c3.files, ['a.txt', 'c1.txt', 'c2.txt', 'feature.txt', 'owner.txt']) && c3.count === '5' && same(c3.pressed, ['Entire worktree']), c3);
    // Each agent keeps its own comparison: the checkout agent's Entire worktree does not carry over.
    const w5 = await reviewOf('Worktree agent', w.run.id);
    const again = await shown(w5, 'task_start', 2);
    check('each agent keeps its own choice (the worktree agent is still on Since task start)', same(again.pressed, ['Since task start']), again);

    // ---- 3. Accepted or Rejected, per change and per file; "Save your edits" only after typing.
    const edits = agent('Accept agent', 'worktree', "sed -i '' -e 's/^L10: original$/L10: agent edit/' -e 's/^L100: original$/L100: agent edit/' a.txt; sed -i '' -e 's/^L50: original$/L50: agent edit/' -e 's/^L250: original$/L250: agent edit/' b.txt");
    await waitDone(edits.run.id);
    const E = edits.workspace.path, line = (file, n) => fs.readFileSync(path.join(E, file), 'utf8').split('\n')[n - 1];
    const bBefore = git(E, 'show', 'HEAD:b.txt') + '\n';
    const er = await reviewOf('Accept agent', edits.run.id);
    await shown(er, 'task_start', 2);
    const fileOf = file => `[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)})`;
    const fileWords = (frame, file) => frame.eval(`(() => { const e = ${fileOf(file)}; if (!e) return null; const b = s => e.querySelector(s); const vis = x => !!x && !x.hidden;
      return { load: e.dataset.loadState, hunks: Number(e.dataset.hunks || 0), accepted: Number(e.dataset.reviewed || 0), accept: vis(b('.file-accept')) ? b('.file-accept').textContent : null, acceptPressed: b('.file-accept')?.getAttribute('aria-pressed'),
        reject: vis(b('.file-reject')) ? b('.file-reject').textContent : null, rejectDisabled: b('.file-reject')?.disabled, save: vis(b('.save-file')) ? b('.save-file').textContent : null,
        hunkWords: [...e.querySelectorAll('.hunk-actions')].map(h => [...h.querySelectorAll('button')].map(x => x.textContent.trim())), notice: document.getElementById('notice').textContent }; })()`);
    const waitWords = (frame, file, pred, ms = 20000) => frame.waitFor(`(() => { const e = ${fileOf(file)}; if (!e) return false; e.scrollIntoView({ block: 'start' }); return e.dataset.loadState === 'rendered' && (${pred}); })()`, ms).then(() => true, () => false);
    const clickIn = async (frame, file, selector) => {
      await frame.eval(`(() => { document.getElementById('click-target')?.removeAttribute('id'); const e = ${fileOf(file)}; e.scrollIntoView({ block: 'start' }); e.querySelector(${JSON.stringify(selector)}).id = 'click-target'; })()`);
      await delay(300); await click(frame, '#click-target');
    };
    // A change's button, the change chosen by the text of its first line.
    const hunkClick = async (frame, file, text, which) => {
      await frame.waitFor(`(() => { const e = ${fileOf(file)}; if (!e || e.dataset.loadState !== 'rendered') return false; e.scrollIntoView({ block: 'start' });
        const target = [...e.querySelectorAll('.editor.modified .view-line')].find(l => l.textContent.replace(/ /g, ' ').includes(${JSON.stringify(text)})); if (!target) return false;
        const top = target.getBoundingClientRect().top; const bar = [...e.querySelectorAll('.hunk-actions')].sort((a, b) => Math.abs(a.getBoundingClientRect().top - top) - Math.abs(b.getBoundingClientRect().top - top))[0]; if (!bar) return false;
        document.getElementById('click-target')?.removeAttribute('id'); bar.querySelector('.hunk-${which}').id = 'click-target'; document.getElementById('diffs').scrollTop += bar.getBoundingClientRect().top - 200; return true; })()`, 20000);
      await delay(300); await click(frame, '#click-target');
    };
    await waitWords(er, 'a.txt', "Number(e.dataset.hunks) === 2");
    const start = await fileWords(er, 'a.txt');
    check('each file offers Accept file and Reject file, each change Accept and Reject; no Keep, Undo or Save before typing', start && start.accept === 'Accept file' && start.reject === 'Reject file' && !start.rejectDisabled && start.save === null && start.hunks === 2 && start.hunkWords.every(w => JSON.stringify(w) === '["Accept","Reject"]'), start);
    // A narrow card (the owner, 2026-09-30): Accept file and Reject file are icons only, a check in
    // the accept colour and an X in the reject colour, solid; the words stay in tooltip and label;
    // each change keeps its words.
    const narrowAt = async w => { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 900, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1600); };
    const icons = () => er.eval(`(() => { const e = ${fileOf('a.txt')}; e.scrollIntoView({ block: 'start' }); const card = e.getBoundingClientRect().width;
      const one = sel => { const b = e.querySelector(sel), g = b.querySelector('.file-glyph'), w = b.querySelector('.file-word'), cs = getComputedStyle(g), r = g.getBoundingClientRect();
        return { word: getComputedStyle(w).display === 'none' ? '' : w.textContent, label: b.getAttribute('aria-label'), title: b.title, stroke: cs.stroke, opacity: Number(cs.opacity) * Number(getComputedStyle(b).opacity), size: Math.round(r.width), visible: r.width > 0 && b.getBoundingClientRect().right <= e.getBoundingClientRect().right }; };
      const probe = (v) => { const d = document.createElement('i'); d.style.color = 'var(' + v + ')'; document.body.append(d); const c = getComputedStyle(d).color; d.remove(); return c; };
      return { card: Math.round(card), accept: one('.file-accept'), reject: one('.file-reject'), done: probe('--ov-done'), removed: probe('--ov-removed'),
        hunkWords: [...e.querySelectorAll('.hunk-actions')].map(h => [...h.querySelectorAll('button')].map(x => getComputedStyle(x.querySelector('.hunk-word')).display === 'none' ? '' : x.textContent.trim())) }; })()`);
    await narrowAt(1280);
    const narrow = await icons();
    const solid = c => /^rgb\(/.test(c) || (/^rgba\(/.test(c) && parseFloat(c.split(',')[3]) >= 0.9);
    await s.screenshot('narrow-card-icons');
    const cardBox = await er.eval(`(() => { const r = ${fileOf('a.txt')}.querySelector('.file-header').getBoundingClientRect(); return { w: r.width, h: r.height }; })()`);
    await er.eval(`${fileOf('a.txt')}.querySelector('.file-header').id = 'narrow-head'`);
    const head = await s.webviewPoint(er, '#narrow-head');
    await er.eval(`document.getElementById('narrow-head')?.removeAttribute('id')`);
    // webviewPoint is the header's left edge + up to 40 px and its middle; the clip is the header and the change below it.
    await s.screenshot('narrow-card-icons-closeup', { x: Math.max(0, head.x - Math.min(cardBox.w / 2, 40) - 8), y: Math.max(0, head.y - Math.min(cardBox.h / 2, 12) - 8), width: cardBox.w + 16, height: 220 });
    check('a narrow card: Accept file and Reject file are a solid check in the accept colour and an X in the reject colour, words in tooltip and label; each change keeps its words',
      narrow && narrow.card <= 440 && narrow.accept.word === '' && narrow.reject.word === '' && narrow.accept.visible && narrow.reject.visible && narrow.accept.size >= 16 && narrow.reject.size >= 16
        && narrow.accept.stroke === narrow.done && narrow.reject.stroke === narrow.removed && solid(narrow.done) && solid(narrow.removed) && narrow.accept.opacity === 1 && narrow.reject.opacity === 1
        && /Accept every change to a\.txt/.test(narrow.accept.label) && /^Accept file/.test(narrow.accept.title) && /Reject every change to a\.txt/.test(narrow.reject.label) && /^Reject file/.test(narrow.reject.title)
        && narrow.hunkWords.length === 2 && narrow.hunkWords.every(w => JSON.stringify(w) === '["Accept","Reject"]'), narrow);
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench); await delay(1600);
    await hunkClick(er, 'a.txt', 'L10: agent edit', 'accept');
    const oneAccepted = await waitWords(er, 'a.txt', "Number(e.dataset.reviewed) === 1 && [...e.querySelectorAll('.hunk-accept')].some(b => b.getAttribute('aria-pressed') === 'true' && b.textContent.trim() === 'Accepted')");
    const afterAccept = await fileWords(er, 'a.txt');
    await s.screenshot('change-accepted');
    check('a change accepted reads Accepted; the file on disk keeps the agent\'s line', oneAccepted && line('a.txt', 10) === 'L10: agent edit' && afterAccept.accept === 'Accept file' && afterAccept.save === null, afterAccept);
    await hunkClick(er, 'a.txt', 'L100: agent edit', 'reject');
    const oneRejected = await waitWords(er, 'a.txt', "Number(e.dataset.hunks) === 1");
    const afterReject = await fileWords(er, 'a.txt');
    await s.screenshot('change-rejected');
    check('a change rejected: the line is back on disk, the review says Rejected, and "Save your edits" does not appear', oneRejected && line('a.txt', 100) === 'L100: original' && line('a.txt', 10) === 'L10: agent edit' && /^Rejected: the agent's change at lines 100–100 of a\.txt was taken out\.$/.test(afterReject.notice) && afterReject.save === null, { afterReject, l100: line('a.txt', 100) });
    await waitWords(er, 'b.txt', "Number(e.dataset.hunks) === 2");
    await clickIn(er, 'b.txt', '.file-accept');
    const fileAccepted = await waitWords(er, 'b.txt', "Number(e.dataset.reviewed) === 2 && e.querySelector('.file-accept').textContent === 'Accepted' && e.querySelector('.file-accept').getAttribute('aria-pressed') === 'true'");
    const bAccepted = await fileWords(er, 'b.txt');
    await s.screenshot('file-accepted');
    check('a whole file accepted: every change reads Accepted and the file\'s button reads Accepted', fileAccepted && bAccepted.hunkWords.every(w => w[0] === 'Accepted') && bAccepted.save === null, bAccepted);
    await clickIn(er, 'b.txt', '.file-reject');
    const bGone = await er.waitFor(`!${fileOf('b.txt')}`, 20000).then(() => true, () => false);
    const bText = fs.readFileSync(path.join(E, 'b.txt'), 'utf8');
    const afterFileReject = await er.eval(`({ notice: document.getElementById('notice').textContent, count: document.getElementById('total').dataset.count, saves: [...document.querySelectorAll('.save-file')].filter(b => !b.hidden).length })`);
    await s.screenshot('file-rejected');
    check('a whole file rejected: b.txt is as it was before the agent, leaves the review, and the review says Rejected', bGone && bText === bBefore && afterFileReject.notice === "Rejected: the agent's changes to b.txt were taken out." && afterFileReject.count === '1' && afterFileReject.saves === 0, { ...afterFileReject, same: bText === bBefore });
    // Typing shows "Save your edits"; saving writes what was typed and hides it again.
    await er.waitFor(`(() => { const e = ${fileOf('a.txt')}; if (!e || e.dataset.loadState !== 'rendered') return false; e.scrollIntoView(); const l = [...e.querySelectorAll('.editor.modified .view-lines .view-line')].find(l => /L10:.agent.edit/.test(l.textContent)); if (!l) return false; l.scrollIntoView({ block: 'center' }); document.getElementById('click-target')?.removeAttribute('id'); l.id = 'click-target'; return true; })()`, 15000);
    await delay(500);
    const p = await s.webviewPoint(er, '#click-target');
    await cdp.click(p.x - 20, p.y); await cdp.key('End'); await cdp.type(' OWNER-TYPED'); await delay(800);
    const typed = await er.waitFor(`(() => { const e = ${fileOf('a.txt')}; const b = e && e.querySelector('.save-file'); return b && !b.hidden && !b.disabled ? b.textContent : null; })()`, 10000).catch(() => null);
    await s.screenshot('save-your-edits');
    check('"Save your edits" appears once the owner types', typed === 'Save your edits', typed);
    await clickIn(er, 'a.txt', '.save-file');
    const saved = await (async () => { for (let i = 0; i < 40; i++) { if (line('a.txt', 10) === 'L10: agent edit OWNER-TYPED') return true; await delay(250); } return false; })();
    const hiddenAgain = await er.waitFor(`(() => { const e = ${fileOf('a.txt')}; return !!e && e.querySelector('.save-file').hidden; })()`, 10000).then(() => true, () => false);
    check('Save your edits writes what was typed, then goes away', saved && hiddenAgain, { l10: line('a.txt', 10), hiddenAgain });
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
