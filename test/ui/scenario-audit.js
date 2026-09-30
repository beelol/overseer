// Presentation audit for AC-54 (and the AC-66 review page): the same deterministic fixture
// session (SYNTHETIC Claude fixture sessions and generic runs; no paid tokens) is shown in every
// Overseer view at 1280 px and 900 px wide in each theme; each view is screenshotted and audited
// for visible text, horizontal overflow, unbroken text runs over 80 characters outside code, and
// icon-only controls without a name or tooltip.
//
//   AUDIT_UI=baseline  today's UI (before Gate J); views found by their old selectors
//   AUDIT_UI=new       the Gate J UI; views carry data-audit-view (needs a Gate J VSIX)
//   AUDIT_UI=gatek     the Gate K layout (default): agents in the side bar, review left and chat right
//   AUDIT_VSIX=path    VSIX to install (default: the latest build)
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

const UI = process.env.AUDIT_UI || 'gatek';
const THEMES = UI === 'baseline' ? ['Default Dark Modern', 'Default Light Modern'] : ['Overseer Dark', 'Overseer Light', 'Default Dark Modern', 'Overseer'];
const WIDTHS = [1280, 900];
const HEIGHT = 860;

(async () => {
  const s = new Session({ baseline: 'audit-baseline', new: 'audit', gatek: 'audit-gatek' }[UI]);
  const result = { ui: UI, checks: [], views: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  const cli = fx('account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  fs.writeFileSync(next, 'desk:pro');
  cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  const runs = {};
  try {
    const web = makeRepo(path.join(s.root, 'web-app'), { dirty: false });
    const api = makeRepo(path.join(s.root, 'api-server'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0], 'window.dialogStyle': 'custom' });
    s.install(process.env.AUDIT_VSIX || latestVsix());
    s.launch(web, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,CLAUDE_FIXTURE_MODE_FILE' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 300) { const st = state(id)?.status; if (re.test(st || '')) return st; await delay(300); } return state(id)?.status; };
    const claude = async (repo, mode, title, prompt, re) => {
      fs.writeFileSync(modeFile, mode);
      const t = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title, prompt });
      await waitStatus(t.run.id, re);
      return t;
    };
    runs.showcase = await claude(web, 'showcase', 'Refresh sessions once', 'Expired sessions trigger a refresh in every tab. Make them refresh once and share the result, and add tests.', /completed|failed/);
    runs.permission = await claude(web, 'showcase-permission', 'Add a changelog entry', 'Add a changelog entry for the session refresh change.', /waiting_for_user/);
    runs.nested = await claude(api, 'nested', 'Split the payment service', 'Split the payment service into modules; delegate the refactor to a sub-agent.', /completed|failed/);
    runs.watch = s.ctl('task.create', { repo: api, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo watching the build; sleep 900'], prompt: '', title: 'Watch the build' });
    runs.failed = s.ctl('task.create', { repo: api, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo "migration dry-run: 3 pending"; echo "error: relation users_v2 missing" 1>&2; exit 1'], prompt: '', title: 'Migration dry-run' });
    await waitStatus(runs.failed.run.id, /failed|completed/);
    s.note('runs', Object.fromEntries(Object.entries(runs).map(([k, v]) => [k, { id: v.run.id, status: state(v.run.id)?.status }])));
    s.ctl('account.create', { provider: 'openai', name: 'ChatGPT Work' });

    const setTheme = async theme => {
      const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme;
      fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2000);
    };
    const setWidth = async w => { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: HEIGHT, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1500); };
    // A view is measured once it has settled: two equal readings 500 ms apart. A transient state
    // (text that comes and goes while the view updates) is not what the owner reads; it is recorded
    // with the words that differed, so it can be found, and never counted.
    // The account an agent runs on (AC-235, the owner's, added after Gate J) is on every surface:
    // its words are measured on their own (`dropped`), so each view's budget stays Gate J's.
    const accountTexts = () => {
      let profiles = [];
      try { profiles = s.ctl('state', {}, { wait: false }).profiles || []; } catch { /* no daemon yet */ }
      const texts = profiles.flatMap(p => p.account ? [`Runs on ${p.account.label}`, p.account.label, p.account.short, [p.account.plan, p.account.email].filter(Boolean).join(' · '), p.account.email] : []).concat(["Mac's default login"]).filter(Boolean);
      return [...new Set(texts.flatMap(t => [` · ${t}`, t]))];
    };
    const auditOnce = (frame, opts) => { const o = { drop: accountTexts(), ...opts, words: true }; return frame ? frame.eval(auditExpression(o)) : cdp.evalWorkbench(auditExpression(o)); };
    const wordDiff = (a, b) => {
      const count = ws => ws.reduce((m, w) => m.set(w, (m.get(w) || 0) + 1), new Map());
      const ca = count(a || []), cb = count(b || []);
      const only = (x, y) => [...x].flatMap(([w, n]) => Array(Math.max(0, n - (y.get(w) || 0))).fill(w));
      return { gone: only(ca, cb), came: only(cb, ca) };
    };
    const same = (a, b) => !a.missing && !b.missing && a.chars === b.chars && JSON.stringify(a.words) === JSON.stringify(b.words) && a.overflow.length === b.overflow.length && a.unnamed.length === b.unnamed.length;
    const audit = async (frame, opts) => {
      let prev = await auditOnce(frame, opts);
      const transient = [];
      for (let i = 0; i < 12; i++) {
        await delay(500);
        const cur = await auditOnce(frame, opts);
        if (same(prev, cur)) break;
        if (!prev.missing && !cur.missing) transient.push({ from: prev.chars, to: cur.chars, ...wordDiff(prev.words, cur.words), overflow: [prev.overflow.length, cur.overflow.length] });
        prev = cur;
      }
      const { words, ...res } = prev;
      if (transient.length) { res.transient = transient; s.note('transient reading before the view settled', transient); }
      return res;
    };
    const record = (view, key, res) => {
      (result.views[view] ||= {})[key] = res;
      if (res.missing) { s.note(`view ${view} ${key}: missing ${res.missing}`); return; }
      s.note(`view ${view} ${key}: ${res.chars} chars, ${res.longRuns.length} long runs, ${res.overflow.length} overflow, ${res.unnamed.length} unnamed`);
    };

    // Open the dashboard on the showcase run.
    const views = {};
    if (UI === 'baseline') {
      await cdp.command('Overseer: Open Overseer View');
      const center = await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('#tree .row').length > 3`, 30000);
      const id = runs.showcase.run.id;
      await center.eval(`(() => { const r = [...document.querySelectorAll('#tree .row')].find(r => r.dataset.run === ${JSON.stringify(id)}); r.scrollIntoView(); r.querySelector('.label').id = 'pick'; return true; })()`);
      const p = await s.webviewPoint(center, '#pick'); await cdp.click(p.x, p.y); await delay(3000);
      views.agents = { frame: center, opts: { root: 'body', exclude: ['#files-section'] } };
      views.files = { frame: center, opts: { root: '#files-section' } };
      views.chat = { frame: await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(id)} && !!document.querySelector('#conv .turn')`, 30000), opts: { root: 'body' } };
      views.review = { frame: await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(runs.showcase.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0 && !(document.getElementById('loading-stage')?.textContent || '').trim()`, 30000), opts: { root: 'body' } };
    } else if (UI === 'gatek') {
      // Gate K: the showcase agent has changes, so the review opens left and the chat right.
      await s.selectRun(runs.showcase.run.id, { settle: 3000 });
      const dash = await s.editorView(`!!document.querySelector('[data-audit-view="chat"] .msg')`);
      // Gate M (AC-100): the chat has no Files pane; the review's navigator lists the files.
      const tagged = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); if (!pane) return false; pane.dataset.audit = 'agents'; return true; })()`);
      views.dashboard = { frame: dash, opts: { root: 'body' } };
      // The rollup row and the header's count (AC-254, AC-255, added after Gate J) are measured on their own.
      views.agents = tagged ? { frame: null, opts: { root: '[data-audit="agents"]', nativeHover: true, exclude: ['[data-audit="agents"] .monaco-list-row[aria-label^="Agents: "]', '[data-audit="agents"] .pane-header .description'] } } : undefined;
      // VS Code's managed hover really shows the name of a title action (checked once by pointer).
      const act = await cdp.evalWorkbench(`(() => { const a = document.querySelector('[data-audit="agents"] .pane-header .action-label[aria-label^="Search Agents"]'); if (!a) return null; const r = a.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
      if (act) { await cdp.move(act.x, act.y); await delay(300); await cdp.move(act.x + 1, act.y); }
      const hoverText = act && await cdp.waitFor(`[...document.querySelectorAll('.workbench-hover, .monaco-hover')].filter(h => h.offsetParent).map(h => h.innerText).find(t => /Search Agents/.test(t))`, 4000).catch(() => '');
      if (act) await cdp.move(act.x + 300, act.y + 300);
      check('agents: side-bar title actions show their name in VS Code\'s hover', !!hoverText, hoverText);
      views.chat = { frame: dash, opts: { root: '[data-audit-view="chat"]' } };
      views.review = { frame: await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(runs.showcase.run.id)} && !(document.getElementById('loading-stage')?.textContent || '').trim()`, 30000), opts: { root: 'body' } };
      if (!views.agents) delete views.agents;
    } else {
      await cdp.command('Overseer: Enter Focus Mode');
      const dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('[data-audit-view="agents"]')`, 30000);
      const id = runs.showcase.run.id;
      await dash.eval(`(() => { const r = document.querySelector('[data-run=${JSON.stringify(id)}]'); r.scrollIntoView(); r.id = 'pick'; return true; })()`);
      const p = await s.webviewPoint(dash, '#pick'); await cdp.click(p.x, p.y); await delay(3000);
      await dash.waitFor(`!!document.querySelector('[data-audit-view="chat"] .msg')`, 20000);
      await dash.eval(`document.getElementById('files-toggle').click()`);
      await dash.waitFor(`document.body.dataset.filesReady === '1'`, 20000);
      views.dashboard = { frame: dash, opts: { root: 'body' } };
      views.agents = { frame: dash, opts: { root: '[data-audit-view="agents"]' } };
      views.chat = { frame: dash, opts: { root: '[data-audit-view="chat"]' } };
      views.files = { frame: dash, opts: { root: '[data-audit-view="files"]' } };
      views.review = { frame: await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(runs.showcase.workspace.path)}) || document.body.dataset.workspace === ${JSON.stringify(runs.showcase.workspace.path)} && !(document.getElementById('loading-stage')?.textContent || '').trim()`, 30000), opts: { root: 'body' } };
    }

    for (const theme of THEMES) {
      await setTheme(theme);
      for (const w of WIDTHS) {
        await setWidth(w);
        const key = `${theme}@${w}`;
        await s.screenshot(`dashboard-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
        for (const [name, v] of Object.entries(views)) record(name, key, await audit(v.frame, v.opts));
      }
    }

    // Grid (new UI only).
    if (UI !== 'baseline') {
      let dash = views.dashboard.frame;
      const toggleGrid = () => UI === 'gatek' ? cdp.command('Overseer: Toggle Agent Grid') : dash.eval(`document.querySelector('[data-action="grid"]').click()`);
      await toggleGrid();
      if (UI === 'gatek') dash = await cdp.webview(`!!document.querySelector('[data-audit-view="grid"] .tile')`, 20000);
      await dash.waitFor(`!!document.querySelector('[data-audit-view="grid"] .tile')`, 20000);
      for (const theme of THEMES) {
        await setTheme(theme);
        for (const w of WIDTHS) {
          await setWidth(w);
          await s.screenshot(`grid-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
          record('grid', `${theme}@${w}`, await audit(dash, { root: '[data-audit-view="grid"]', exclude: ['#grid-rollup'] }));
          record('grid-rollup', `${theme}@${w}`, await audit(dash, { root: '#grid-rollup' }));
        }
      }
      await toggleGrid();
      await delay(1500);
    }

    // New agent: the New Task form (baseline) or the composer shown with no agent selected (new).
    if (UI === 'baseline') await cdp.command('Overseer: Start an Agent with the Full Form');
    else if (UI === 'gatek') await cdp.command('Overseer: New Agent');
    else await views.dashboard.frame.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    const composer = UI === 'baseline'
      ? await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('#harnesses .tile').length >= 3`, 30000)
      : UI === 'gatek' ? await cdp.webview(`!!document.querySelector('[data-audit-view="composer"]')`, 20000) : views.dashboard.frame;
    if (UI !== 'baseline') await composer.waitFor(`!!document.querySelector('[data-audit-view="composer"]')`, 20000);
    for (const theme of THEMES) {
      await setTheme(theme);
      for (const w of WIDTHS) {
        await setWidth(w);
        await s.screenshot(`new-agent-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
        // Home's head (the Voice button with its shortcut and the Needs-you count, AC-217 and
        // AC-227, added after Gate J) is measured on its own, so the composer's budget stays Gate J's.
        record('new-agent', `${theme}@${w}`, await audit(composer, { root: UI === 'baseline' ? 'body' : '[data-audit-view="composer"]', exclude: UI === 'baseline' ? [] : ['.home-head'] }));
        if (UI !== 'baseline') record('home-head', `${theme}@${w}`, await audit(composer, { root: '.home-head' }));
      }
    }

    // Accounts (native view in the Overseer side bar).
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench);
    await s.openOverseerView();
    await cdp.command('Overseer: Refresh Account Status'); await delay(2500);
    const tagged = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Accounts/i.test(p.querySelector('.pane-header')?.textContent.trim() || '')); if (!pane) return false; pane.dataset.audit = 'accounts'; return true; })()`);
    for (const theme of THEMES) {
      await setTheme(theme);
      for (const w of WIDTHS) {
        await setWidth(w);
        await s.screenshot(`accounts-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
        record('accounts', `${theme}@${w}`, tagged ? await audit(null, { root: '[data-audit="accounts"]' }) : { missing: 'accounts pane' });
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench);

    // Summary per view: text (max over settings; text does not depend on width), and any failures.
    const summary = {};
    for (const [view, byKey] of Object.entries(result.views)) {
      const vals = Object.values(byKey).filter(v => !v.missing);
      summary[view] = { chars: Math.max(0, ...vals.map(v => v.chars)), longRuns: [...new Set(vals.flatMap(v => v.longRuns))], overflow: vals.flatMap(v => v.overflow).length, unnamed: [...new Set(vals.flatMap(v => v.unnamed.map(u => u.html)))] };
    }
    result.summary = summary;
    s.note('summary', summary);
    if (UI !== 'baseline') {
      const base = JSON.parse(fs.readFileSync(path.join(repoRoot, 'docs/verification/evidence/ui/audit-baseline/result.json'), 'utf8')).summary;
      for (const [view, v] of Object.entries(summary)) {
        check(`${view}: no horizontal overflow at 1280/900 px in every theme`, v.overflow === 0, v.overflow);
        check(`${view}: no unbroken text run over 80 characters outside code`, v.longRuns.length === 0, v.longRuns);
        check(`${view}: every icon-only control has a name and a tooltip`, v.unnamed.length === 0, v.unnamed);
        const b = base[view === 'dashboard' || view === 'grid' ? null : view];
        if (b) check(`${view}: at least 40% less visible text than today's UI (${b.chars} → ${v.chars})`, v.chars <= b.chars * 0.6, { before: b.chars, after: v.chars, reduction: Math.round((1 - v.chars / b.chars) * 100) + '%' });
      }
      if (UI === 'gatek') {
        // AC-81: the text budget re-measured against Gate J; AC-67's agents budget is Gate J's 238.
        const gatej = JSON.parse(fs.readFileSync(path.join(repoRoot, 'docs/verification/evidence/ui/audit/result.json'), 'utf8')).summary;
        result.gatej = gatej;
        const rows = Object.entries(summary).map(([view, v]) => ({ view, gatej: gatej[view]?.chars, gatek: v.chars }));
        s.note('text: Gate J → Gate K', rows);
        check('agents: the side bar stays within the Gate J agents budget (238 characters)', (summary.agents?.chars ?? Infinity) <= 238, summary.agents?.chars);
        for (const r of rows.filter(r => r.gatej != null && r.view !== 'agents' && r.view !== 'dashboard')) check(`${r.view}: no more visible text than Gate J (${r.gatej} → ${r.gatek})`, r.gatek <= r.gatej, r);
        check('home\'s head: the Voice button with its shortcut and the Needs-you count stay within 24 characters (AC-217, AC-227)', (summary['home-head']?.chars ?? Infinity) <= 24, summary['home-head']?.chars);
      }
    }
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (runs.watch) s.ctl('run.interrupt', { run_id: runs.watch.run.id }); } catch {}
    try { if (runs.permission) s.ctl('run.interrupt', { run_id: runs.permission.run.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
