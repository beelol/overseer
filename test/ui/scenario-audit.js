// Presentation audit for AC-54 (and the AC-66 review page): the same deterministic fixture
// session (SYNTHETIC Claude fixture sessions and generic runs; no paid tokens) is shown in every
// Overseer view at 1280 px and 900 px wide in each theme; each view is screenshotted and audited
// for visible text, horizontal overflow, unbroken text runs over 80 characters outside code, and
// icon-only controls without a name or tooltip.
//
//   AUDIT_UI=baseline  today's UI (before Gate J); views found by their old selectors
//   AUDIT_UI=new       the Gate J UI; views carry data-audit-view
//   AUDIT_VSIX=path    VSIX to install (default: the latest build)
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

const UI = process.env.AUDIT_UI || 'new';
const THEMES = UI === 'baseline' ? ['Default Dark Modern', 'Default Light Modern'] : ['Overseer Dark', 'Overseer Light', 'Default Dark Modern'];
const WIDTHS = [1280, 900];
const HEIGHT = 860;

(async () => {
  const s = new Session(UI === 'baseline' ? 'audit-baseline' : 'audit');
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
    const audit = async (frame, opts) => (frame ? frame.eval(auditExpression(opts)) : cdp.evalWorkbench(auditExpression(opts)));
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
      views.review = { frame: await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(runs.showcase.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0`, 30000), opts: { root: 'body' } };
    } else {
      await cdp.command('Overseer: Open Dashboard');
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
      views.review = { frame: await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(runs.showcase.workspace.path)}) || document.body.dataset.workspace === ${JSON.stringify(runs.showcase.workspace.path)}`, 30000), opts: { root: 'body' } };
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
      const dash = views.dashboard.frame;
      await dash.eval(`document.querySelector('[data-action="grid"]').click()`);
      await dash.waitFor(`!!document.querySelector('[data-audit-view="grid"] .tile')`, 20000);
      for (const theme of THEMES) {
        await setTheme(theme);
        for (const w of WIDTHS) {
          await setWidth(w);
          await s.screenshot(`grid-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
          record('grid', `${theme}@${w}`, await audit(dash, { root: '[data-audit-view="grid"]' }));
        }
      }
      await dash.eval(`document.querySelector('[data-action="grid"]').click()`);
      await delay(800);
    }

    // New agent: the New Task form (baseline) or the composer shown with no agent selected (new).
    if (UI === 'baseline') await cdp.command('Overseer: New Task');
    else await views.dashboard.frame.eval(`document.querySelector('[data-action="new-agent"]').click()`);
    const composer = UI === 'baseline'
      ? await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('#harnesses .tile').length >= 3`, 30000)
      : views.dashboard.frame;
    if (UI !== 'baseline') await composer.waitFor(`!!document.querySelector('[data-audit-view="composer"]')`, 20000);
    for (const theme of THEMES) {
      await setTheme(theme);
      for (const w of WIDTHS) {
        await setWidth(w);
        await s.screenshot(`new-agent-${theme.replace(/\s+/g, '-').toLowerCase()}-${w}`);
        record('new-agent', `${theme}@${w}`, await audit(composer, { root: UI === 'baseline' ? 'body' : '[data-audit-view="composer"]' }));
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
