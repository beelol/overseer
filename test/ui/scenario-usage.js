// Packaged-UI scenario for AC-62 (usage and limits), fixture harnesses only. A Claude fixture run
// streams Claude's rate_limit_event (95% of the 5-hour window): the account then shows that usage
// in the dashboard's accounts menu, the Accounts view and the composer; starting a task on it
// warns and suggests another compatible account; a Codex fixture account shows the limits its
// session log recorded (20%); accounts whose harness reported nothing say "not reported". Per-run
// tokens and cost appear in the chat. The live comparison with real harness output is in the
// session record.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('usage');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  const modeFile = path.join(s.root, 'claude-mode');
  fs.writeFileSync(next, 'desk:pro'); cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  try {
    const repo = makeRepo(path.join(s.root, 'usage-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_CODEX_USED: '20', OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,CLAUDE_FIXTURE_MODE_FILE,FIXTURE_CODEX_USED' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const wait = async id => { for (let i = 0; i < 40 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === id).status); i++) await delay(300); };
    // A second Claude account (fixed) so the warning can suggest another compatible account.
    const second = s.ctl('account.create', { provider: 'anthropic', name: 'Claude Second' }).account;
    fs.writeFileSync(modeFile, 'limits');
    const near = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Near the limit', prompt: 'hi' });
    await wait(near.run.id);
    fs.writeFileSync(modeFile, 'limits-low');
    const low = s.ctl('task.create', { repo, harness: 'claude', profile_id: second.id, title: 'Plenty left', prompt: 'hi' });
    await wait(low.run.id);
    fs.writeFileSync(modeFile, 'showcase');
    const cost = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Costed run', prompt: 'refresh sessions once' });
    await wait(cost.run.id);
    const cx = s.ctl('task.create', { repo, harness: 'codex', profile_id: 'system-codex', title: 'Codex usage', prompt: 'hi' });
    await wait(cx.run.id);
    const usage = id => s.ctl('account.usage', { id });
    const u = { claude: usage('system-claude'), second: usage(second.id), codex: usage('system-codex'), opencode: usage('system-opencode') };
    s.note('account.usage', u);
    check('usage is what each harness reported: Claude 95% / 12% of 5 hours (rate_limit_event), Codex 20% (session log), OpenCode not reported',
      u.claude.windows.find(w => w.label === '5 hours')?.used === 0.95 && u.second.windows.find(w => w.label === '5 hours')?.used === 0.12 && u.codex.windows.find(w => w.label === '5 hours')?.used === 0.2 && u.opencode.reported === false, u);

    await cdp.command('Overseer: Refresh Account Status'); await delay(2000);
    await cdp.command('Overseer: Open Overseer View');
    const dash = await s.editorView();
    await delay(1000);
    // Gate K: accounts are in the side bar's Accounts view; usage and reset times are in each row's hover.
    await s.openOverseerView();
    const hoverOf = async label => {
      const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(label)}).pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 10000, label);
      // Rest on the row (VS Code shows a tree hover only after the pointer settles); retry once.
      for (let attempt = 0; attempt < 2; attempt++) {
        await cdp.move(pt.x + 400, pt.y + 300); await delay(600);
        await cdp.move(pt.x, pt.y); await delay(200); await cdp.move(pt.x + 2, pt.y); await delay(1600);
        const text = await cdp.waitFor(`(() => { const h = [...document.querySelectorAll('.monaco-hover, .workbench-hover')].find(h => h.offsetParent && h.innerText.includes(${JSON.stringify(label.split(' ')[0])})); return h && h.innerText; })()`, 5000).catch(() => '');
        if (text) { await cdp.move(pt.x + 400, pt.y + 300); await delay(300); return text; }
      }
      return '';
    };
    const claudeHover = await hoverOf('claude (existing login)');
    const codexHover = await hoverOf('codex (existing login)');
    await s.screenshot('accounts-usage');
    check('the Accounts view shows reported usage per account, with reset times, on hover',
      /5 hours 95%/.test(claudeHover) && /resets/.test(claudeHover) && /5 hours 20%/.test(codexHover), { claudeHover, codexHover });

    // Per-run tokens and cost in the chat.
    await s.selectRun(cost.run.id);
    const foot = await dash.waitFor(`(() => { const u = document.querySelector('#conv .turn-foot .usage'); return u && u.textContent && { text: u.textContent, title: u.title }; })()`, 20000);
    check('per-run tokens and cost as the harness reported them (chat turn footer)', /tokens/.test(foot.text) && /\$0\.04/.test(foot.text) && /18,423 in/.test(foot.title), foot);

    // Composer: the near-limit account warns and suggests another compatible account.
    await cdp.command('Overseer: New Agent'); await delay(800);
    await dash.waitFor(`!document.querySelector('[data-chip="agent"]').textContent.includes('Loading')`, 20000);
    await dash.eval(`document.querySelector('[data-chip="agent"]').click()`); await delay(300);
    const agentMenu = await dash.eval(`[...document.querySelectorAll('.menu .menu-item')].map(b => (b.querySelector('.menu-label')?.textContent || '') + ' | ' + (b.querySelector('.menu-hint')?.textContent || ''))`);
    await dash.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => b.querySelector('.menu-label')?.textContent === 'claude (existing login)').click()`); await delay(400);
    await dash.eval(`document.getElementById('task').focus()`); await cdp.type('a small task'); await delay(300);
    const warn = await dash.eval(`(() => { const n = document.querySelector('.view-composer .composer-note'); return { text: n.textContent, fix: n.querySelector('.fix')?.textContent, cls: n.className, startDisabled: document.getElementById('start').disabled }; })()`);
    await s.screenshot('near-limit-warning');
    await dash.eval(`document.querySelector('.view-composer .composer-note .fix').click()`); await delay(400);
    const switched = await dash.eval(`document.querySelector('[data-chip="agent"]').getAttribute('aria-label')`);
    check('starting on an account near its limit warns (95% of 5 hours, reset time) and offers another compatible account; the warning does not block',
      /95% of its 5 hours limit/.test(warn.text) && /resets/.test(warn.text) && warn.fix === 'Use Claude Second' && !warn.startDisabled && /Claude Second/.test(switched) && agentMenu.some(a => /5 hours 95%/.test(a)), { warn, switched, agentMenu });

    // Accounts view: the near-limit account says so; others show nothing extra.
    await s.openOverseerView();
    const rows = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => (r.querySelector('.label-name')?.textContent || '') + ' | ' + (r.querySelector('.label-description')?.textContent || ''))`);
    check('the Accounts view marks an account near its limit', rows.some(r => /claude \(existing login\).*95% of 5 hours/.test(r)), rows);
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
