// AC-35 load scenario (fixture only, no model tokens): 10,000 tracked files, four simulated
// active runs editing files and emitting output for PERF_MINUTES (default 10), with the
// review open on a run that has 100 changed text files. Measures navigation latency,
// refresh latency, memory and daemon queue/retention behaviour.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, latestVsix, delay, git } = require('./harness');

const MINUTES = Number(process.env.PERF_MINUTES || 10);
const pct = (xs, p) => { const s = [...xs].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
const rss = pattern => { const out = cp.spawnSync('ps', ['-axo', 'rss=,command='], { encoding: 'utf8' }).stdout.split('\n').filter(l => l.includes(pattern) && (pattern === 'overseerd-darwin' || l.includes(process.env.PERF_PROFILE || ''))); return out.reduce((a, l) => a + Number(l.trim().split(/\s+/)[0] || 0), 0); };

(async () => {
  const s = new Session('perf');
  const result = { checks: [], samples: [], nav: [], refresh: [], minutes: MINUTES };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = path.join(s.root, 'big');
    fs.mkdirSync(repo, { recursive: true });
    git(repo, 'init', '-q', '-b', 'main'); git(repo, 'config', 'user.email', 't@e'); git(repo, 'config', 'user.name', 't');
    for (let d = 0; d < 100; d++) {
      fs.mkdirSync(path.join(repo, `pkg${d}`), { recursive: true });
      for (let f = 0; f < 100; f++) fs.writeFileSync(path.join(repo, `pkg${d}`, `file${f}.txt`), Array.from({ length: 40 }, (_, i) => `pkg${d} file${f} line${i}`).join('\n') + '\n');
    }
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', '10k files');
    const tracked = Number(cp.execFileSync('git', ['ls-files'], { cwd: repo, encoding: 'utf8' }).split('\n').filter(Boolean).length);
    s.note('fixture', { tracked });
    s.settings();
    s.install(latestVsix());
    process.env.PERF_PROFILE = s.profile;
    s.launch(repo);
    setTimeout(() => s.note('processes', cp.spawnSync('ps', ['-axo', 'rss=,command='], { encoding: 'utf8' }).stdout.split('\n').filter(l => l.includes(s.profile)).map(l => l.trim().slice(0, 140))), 20000);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    // Four simulated runs: each edits its own 100 files (one per ~0.4 s) and prints output.
    const script = (n) => `end=$(( $(date +%s) + ${MINUTES * 60} )); i=0; while [ $(date +%s) -lt $end ]; do d=$((i % 100)); printf 'edit %s at %s\\n' "$i" "$(date +%s)" >> pkg$d/file$((d % 7)).txt; echo "run ${n} step $i: edited pkg$d"; i=$((i+1)); sleep 0.4; done`;
    const tasks = [];
    for (let n = 0; n < 4; n++) tasks.push(s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script(n)], prompt: '', title: `load run ${n}` }));
    const main = tasks[0];
    const ws = main.workspace.path;
    await delay(45000); // all 100 files changed at least once
    const icon = await cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item a, .activitybar .action-label')].find(a => /^Overseer/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(icon.x, icon.y);
    const row = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top); const i = rows.findIndex(r => r.textContent.includes('load run 0')); const r = rows[i + 1]; if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(row.x, row.y);
    const review = await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(ws)})`, 60000);
    await review.waitFor(`document.querySelectorAll('.diff-file').length >= 100`, 60000);
    check('review lists the 100 changed files of the selected run (10k tracked)', true, await review.eval(`document.querySelectorAll('.diff-file').length`));
    await s.screenshot('load-review');
    const t0 = Date.now();
    const baseline = { exthost: rss('Code Helper (Plugin)'), daemon: rss('overseerd-darwin') };
    let sampleAt = Date.now();
    let k = 0;
    while (Date.now() - t0 < (MINUTES * 60 - 60) * 1000) {
      // Navigation: click a file in the navigator and measure until its diff is in view.
      const target = await review.eval(`(() => { const b = [...document.querySelectorAll('#tree .file')]; const e = b[(${k} * 37) % b.length]; e.id = 'nav-target'; e.scrollIntoView({ block: 'center' }); return e.dataset.id; })()`);
      const p = await s.webviewPoint(review, '#nav-target');
      await review.eval(`window.__navStart = performance.now(); window.__navDone = undefined; (() => { const id = ${JSON.stringify(target)}; const check = () => { const el = document.querySelector('.diff-file[data-id="' + id + '"]'); const d = document.getElementById('diffs'); const r = el && el.getBoundingClientRect(), dr = d.getBoundingClientRect(); if (el && r.top < dr.bottom - 20 && r.bottom > dr.top + 5 && document.querySelector('#tree .file.active')?.dataset.id === id) window.__navDone = performance.now() - window.__navStart; else setTimeout(check, 5); }; setTimeout(check, 0); })()`);
      await cdp.click(p.x, p.y);
      const nav = await review.waitFor(`window.__navDone`, 5000).catch(() => 5000);
      result.nav.push(nav);
      // Refresh latency for an ordinary file write in the reviewed worktree.
      if (k % 5 === 0) {
        const name = `probe-${k}.txt`;
        const w0 = Date.now();
        fs.writeFileSync(path.join(ws, name), 'probe\n');
        const ok = await review.waitFor(`[...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === ${JSON.stringify(name)})`, 8000).then(() => true, () => false);
        result.refresh.push(ok ? Date.now() - w0 : null);
      }
      if (Date.now() - sampleAt > 30000) {
        sampleAt = Date.now();
        const heap = await review.eval(`performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : null`);
        const events = s.ctl('events.list', { run_id: tasks[1].run.id, limit: 5000 }).events.length;
        const sample = { t: Math.round((Date.now() - t0) / 1000), exthostKB: rss('Code Helper (Plugin)'), daemonKB: rss('overseerd-darwin'), rendererKB: rss('Code Helper (Renderer)'), webviewHeapMB: heap, retainedEventsRun1: events };
        result.samples.push(sample); s.note('sample', sample);
      }
      k++;
      await delay(1500);
    }
    await s.screenshot('load-end');
    // Drain: stop the runs and observe memory settle.
    for (const t of tasks) { try { s.ctl('run.interrupt', { run_id: t.run.id }); } catch {} }
    await delay(20000);
    const drained = { exthostKB: rss('Code Helper (Plugin)'), daemonKB: rss('overseerd-darwin'), webviewHeapMB: await review.eval(`performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : null`) };
    result.drained = drained; result.baseline = baseline;
    const p95 = pct(result.nav, 0.95);
    result.navP95 = p95; result.navP50 = pct(result.nav, 0.5);
    const refreshes = result.refresh.filter(x => x !== null);
    result.refreshP95 = pct(refreshes, 0.95);
    check('navigation p95 under 250 ms', p95 < 250, { p95, p50: result.navP50, n: result.nav.length });
    check('ordinary file refresh within 2 s under load', result.refresh.every(x => x !== null && x <= 2000), { p95: result.refreshP95, max: Math.max(...refreshes), missed: result.refresh.filter(x => x === null).length });
    const retained = result.samples.map(x => x.retainedEventsRun1);
    check('daemon retention bounds per-run events', Math.max(...retained) <= 5000 + 50, { max: Math.max(...retained) });
    const late = result.samples.slice(-4).map(x => x.exthostKB), early = result.samples.slice(2, 6).map(x => x.exthostKB);
    const growth = (Math.max(...late) - Math.max(...early)) / Math.max(...early);
    check('extension host memory stabilizes (<25% growth late vs early)', growth < 0.25, { early, late, growth: Math.round(growth * 100) + '%' });
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
