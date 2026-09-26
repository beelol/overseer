// Packaged-UI scenario for AC-76 (a review that stays clean at any width), fixtures only. A
// generic agent edits distant lines (one of them 260 characters long) and writes an 800-line
// file. At 900, 1280 and 1600 px in Overseer Dark and Overseer Light, in the review: the header
// and each file header stay on one line; no hunk Accept/Revert control covers any code; nothing
// overflows sideways; file rows carry codicons; the large diff starts collapsed with its count.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const WIDTHS = [900, 1280, 1600];
const THEMES = ['Overseer Dark', 'Overseer Light'];

(async () => {
  const s = new Session('review-width');
  const result = { checks: [], audits: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'width-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0], 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const long = 'L20: ' + 'a very long edited line that keeps going '.repeat(6).trim();
    const script = `sed -i '' -e 's/^L20: original$/${long}/' -e 's/^L140: original$/L140: agent edit/' -e 's/^L260: original$/L260: agent edit/' a.txt; ` +
      `sed -i '' -e 's/^L5: original$/L5: agent edit/' b.txt; i=1; while [ $i -le 800 ]; do echo "generated line $i"; i=$((i+1)); done > big.txt; echo done`;
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title: 'Wide edits' });
    for (let i = 0; i < 30 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type('Wide edits'); await delay(300); await cdp.key('Enter'); await delay(2500);
    const review = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(t.run.id)}`, 30000);
    await review.waitFor(`document.querySelectorAll('.hunk-actions').length >= 3`, 30000);
    const theme = async name => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = name; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1800); };
    const width = async w => { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 900, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1600); };
    const audit = () => review.eval(`(() => {
      const box = e => e.getBoundingClientRect();
      const oneLine = el => { const kids = [...el.children].filter(k => k.offsetParent && box(k).width > 0 && getComputedStyle(k).position !== 'absolute'); const mids = kids.map(k => { const b = box(k); return b.top + b.height / 2; }); return { ok: mids.every(m => Math.abs(m - mids[0]) <= 6), height: Math.round(box(el).height) }; };
      const toolbar = oneLine(document.getElementById('toolbar'));
      const headers = [...document.querySelectorAll('.file-header')].filter(h => h.offsetParent).map(h => ({ file: h.querySelector('.file-path')?.textContent, ...oneLine(h) }));
      // Hunk controls against every text glyph run in the modified editor of the same file.
      const overlaps = [];
      for (const bar of document.querySelectorAll('.hunk-actions')) {
        const b = box(bar); if (!b.width) continue;
        const file = bar.closest('.diff-file');
        for (const span of file.querySelectorAll('.editor.modified .view-line span span, .modified-in-monaco-diff-editor .view-line span span')) {
          if (!span.textContent.trim()) continue;
          const r = box(span);
          if (r.right > b.left + 1 && r.left < b.right - 1 && r.bottom > b.top + 1 && r.top < b.bottom - 1) { overlaps.push({ file: file.querySelector('.file-path')?.textContent, text: span.textContent.slice(0, 30) }); break; }
        }
      }
      const doc = document.documentElement;
      const overflow = doc.scrollWidth > doc.clientWidth + 1 || document.getElementById('toolbar').scrollWidth > document.getElementById('toolbar').clientWidth + 1;
      const rowsWithoutIcon = [...document.querySelectorAll('#tree .file')].filter(b => !b.querySelector('.codicon')).map(b => b.textContent);
      const card = [...document.querySelectorAll('.large-diff')].map(c => c.textContent);
      return { width: innerWidth, toolbar, headers, overlaps, overflow, rowsWithoutIcon, card, hunks: document.querySelectorAll('.hunk-actions').length };
    })()`);
    for (const th of THEMES) {
      await theme(th);
      for (const w of WIDTHS) {
        await width(w);
        // Scroll through the diffs so every hunk renders once.
        await review.eval(`document.getElementById('diffs').scrollTop = 0`); await delay(600);
        const a = await audit();
        result.audits.push({ theme: th, window: w, ...a });
        await s.screenshot(`review-${th.split(' ')[1].toLowerCase()}-${w}`);
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    s.note('audits', result.audits.map(a => ({ theme: a.theme, window: a.window, review: a.width, toolbar: a.toolbar, overlaps: a.overlaps.length, overflow: a.overflow, hunks: a.hunks })));
    const all = result.audits;
    check('the review header stays on one line at 900, 1280 and 1600 px in both themes', all.every(a => a.toolbar.ok && a.toolbar.height <= 50), all.map(a => [a.theme, a.window, a.toolbar]));
    check('each file header stays on one line', all.every(a => a.headers.every(h => h.ok && h.height <= 46)), all.map(a => [a.window, a.headers.filter(h => !h.ok || h.height > 46)]));
    check('no hunk Accept/Revert control covers code', all.every(a => a.hunks >= 3 && a.overlaps.length === 0), all.map(a => [a.theme, a.window, a.hunks, a.overlaps]));
    check('nothing in the review overflows sideways', all.every(a => !a.overflow), all.map(a => [a.window, a.overflow]));
    check('file rows carry codicons', all.every(a => a.rowsWithoutIcon.length === 0), all[0].rowsWithoutIcon);
    check('a very large diff starts collapsed with its line count', all.every(a => a.card.some(c => /800 changed lines/.test(c))), all[0].card);
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
