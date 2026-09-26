// Packaged-UI scenario for AC-67 (one agents list: the native side bar), AC-68 (provider logos in
// the side bar) and AC-70 (quiet row actions), fixture harnesses only. The Gate J audit fixture set
// (a finished Claude run with changes, one waiting for permission, a nested one with a child and
// grandchild, a long generic run, a failed one) in two repositories: the Agents view shows Needs
// you first, then agents by repository with children nested and archived agents behind a filter;
// the editor-area view has no agent rail; the Agents view's visible text stays within the Gate J
// agents budget (238 characters). Rows carry provider logos (light, dark, high contrast) and status
// badges; hover actions and the context menu work by mouse, menu and keyboard; the Overseer icon's
// badge matches Needs you.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const BUDGET = 238;

(async () => {
  const s = new Session('sidebar');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  let runs = {};
  try {
    const web = makeRepo(path.join(s.root, 'web-app'), { dirty: false });
    const api = makeRepo(path.join(s.root, 'api-server'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'window.titleBarStyle': 'custom' });
    s.install(latestVsix());
    s.launch(web, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 300) { if (re.test(state(id)?.status || '')) return; await delay(300); } };
    const claude = async (repo, mode, title, prompt, re) => { fs.writeFileSync(modeFile, mode); const t = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title, prompt }); await waitStatus(t.run.id, re); return t; };
    runs.showcase = await claude(web, 'showcase', 'Refresh sessions once', 'Expired sessions trigger a refresh in every tab. Make them refresh once.', /completed|failed/);
    runs.permission = await claude(web, 'showcase-permission', 'Add a changelog entry', 'Add a changelog entry for the session refresh change.', /waiting_for_user/);
    runs.nested = await claude(api, 'nested', 'Split the payment service', 'Split the payment service into modules.', /completed|failed/);
    runs.watch = s.ctl('task.create', { repo: api, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo watching the build; sleep 900'], prompt: '', title: 'Watch the build' });
    runs.failed = s.ctl('task.create', { repo: api, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo "error: relation missing" 1>&2; exit 1'], prompt: '', title: 'Migration dry-run' });
    await waitStatus(runs.failed.run.id, /failed/);
    await waitStatus(runs.watch.run.id, /running/);

    await cdp.command('View: Show Overseer');
    await delay(2500);
    // Rows of the Agents view, in order.
    const agentRows = () => cdp.evalWorkbench(`(() => {
      const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      if (!pane) return null;
      return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => {
        const icon = r.querySelector('.custom-view-tree-node-item-icon');
        const bg = icon ? getComputedStyle(icon).backgroundImage : '';
        return { aria: r.getAttribute('aria-label'), level: Number(r.getAttribute('aria-level')), label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '',
          badge: (() => { for (const e of [r.querySelector('.monaco-icon-label'), r.querySelector('.label-name'), r.querySelector('.monaco-icon-label-container')]) { const c = e && getComputedStyle(e, '::after').content; if (c && c !== 'none' && c !== 'normal') return c.replace(/^"|"$/g, ''); } return ''; })(),
          logo: (bg.match(/logos\\/([a-z-]+)\\.svg/) || [])[1] || '', codicon: (icon?.className.match(/codicon-([a-z-]+)/) || [])[1] || '', expanded: r.getAttribute('aria-expanded') };
      });
    })()`);
    let rows = await agentRows();
    s.note('agents view', rows);
    // Agent rows follow Needs you, so the last row with a label is the agent's own.
    const find = (label, from = rows) => from.filter(r => r.label === label).pop();
    const needsIdx = rows.findIndex(r => r.label === 'Needs you');
    const webIdx = rows.findIndex(r => r.label === 'web-app'), apiIdx = rows.findIndex(r => r.label === 'api-server');
    const needsItems = rows.slice(needsIdx + 1).filter(r => r.level === 2).slice(0, 2).map(r => r.label);
    check('the side bar shows Needs you first, then agents by repository with native children nested',
      needsIdx === 0 && webIdx > 0 && apiIdx > 0 && needsItems.includes('Add a changelog entry') && needsItems.includes('Migration dry-run') &&
      find('child task')?.level === 3 && find('grandchild task')?.level === 4 && find('Refresh sessions once')?.level === 2,
      { needsIdx, webIdx, apiIdx, needsItems, child: find('child task'), grandchild: find('grandchild task') });

    // The editor area has no agent rail.
    await cdp.command('Overseer: Open Overseer View');
    const dash = await cdp.webview(`document.body.dataset.ready === '1'`, 30000);
    const railless = await dash.eval(`!document.querySelector('.rail, .rail-list') && !!document.querySelector('.view-composer, .view-chat')`);
    check('the editor-area Overseer view has no agent rail of its own', railless);

    // Text budget: the Agents view's visible text (labels, descriptions, badges) for the same fixtures.
    const text = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => r.innerText.replace(/\\s+/g, '')).join('').length; })()`);
    check(`the Agents view's visible text stays within the Gate J agents budget (${BUDGET} characters)`, text <= BUDGET, { chars: text });

    // Logos and badges.
    check('rows carry provider logos (Claude Code) and codicons for programs; status shows as a row badge',
      find('Refresh sessions once')?.logo.startsWith('claudecode') && find('child task')?.logo.startsWith('claudecode') && find('Watch the build')?.codicon === 'terminal',
      { showcase: find('Refresh sessions once'), watch: find('Watch the build') });
    const badgeOf = label => find(label)?.badge;
    check('status badges: ✓ done, ! needs you, ✕ failed, ● working', badgeOf('Refresh sessions once') === '✓' && badgeOf('Add a changelog entry') === '!' && badgeOf('Migration dry-run') === '✕' && badgeOf('Watch the build') === '●',
      ['Refresh sessions once', 'Add a changelog entry', 'Migration dry-run', 'Watch the build'].map(l => [l, badgeOf(l)]));
    const vsix = cp.execFileSync('unzip', ['-l', latestVsix()], { encoding: 'utf8' });
    const variants = ['claudecode', 'codex', 'opencode'].every(n => vsix.includes(`media/logos/${n}-light.svg`) && vsix.includes(`media/logos/${n}-dark.svg`)) && vsix.includes('NOTICE.md');
    check('the VSIX ships light and dark variants of each logo and the notices', variants);
    const theme = async name => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = name; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1800); };
    const shots = [];
    for (const t of ['Overseer Dark', 'Overseer Light', 'Default High Contrast']) {
      await theme(t); rows = await agentRows();
      shots.push({ theme: t, logo: find('Refresh sessions once', rows)?.logo });
      await s.screenshot('sidebar-' + t.toLowerCase().replace(/ /g, '-'));
    }
    check('logos switch variant with the theme (light theme uses the light variant; dark and high contrast the dark one)',
      shots[0].logo === 'claudecode-dark' && shots[1].logo === 'claudecode-light' && shots[2].logo === 'claudecode-dark', shots);
    await theme('Overseer Dark');

    // Badge on the Overseer activity icon = Needs you.
    const attention = await cdp.evalWorkbench(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item')].find(i => /Overseer/.test(i.querySelector('.action-label')?.getAttribute('aria-label') || '')); return a?.querySelector('.badge-content')?.textContent.trim(); })()`);
    rows = await agentRows();
    const needsCount = rows.find(r => r.label === 'Needs you')?.description;
    check('the Overseer activity icon badge matches Needs you', attention && attention === needsCount, { badge: attention, needs: needsCount });

    // Hover actions: stop on a working agent, archive and pin on a finished one; all named.
    const hover = async label => {
      const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(label)} && r.getAttribute('aria-level') === '2').pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2 }; })()`, 10000, label);
      await cdp.move(pt.x, pt.y); await delay(500);
      return cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(label)} && r.getAttribute('aria-level') === '2').pop();
        return [...r.querySelectorAll('.actions .action-label')].filter(a => a.offsetParent).map(a => ({ name: a.getAttribute('aria-label') || a.title, x: a.getBoundingClientRect().left + 8, y: a.getBoundingClientRect().top + 8 })); })()`);
    };
    const onWatch = await hover('Watch the build');
    const onDone = await hover('Refresh sessions once');
    check('hover actions: Stop on a working agent; Archive and Pin to Grid on a finished one; each has a name',
      onWatch.some(a => a.name.startsWith('Stop')) && onDone.some(a => a.name.startsWith('Archive')) && onDone.some(a => a.name.startsWith('Pin to Grid')) && [...onWatch, ...onDone].every(a => a.name), { onWatch, onDone });
    await s.screenshot('hover-actions');
    // Pin by mouse.
    const pin = onDone.find(a => a.name.startsWith('Pin to Grid')); await cdp.click(pin.x, pin.y); await delay(1200);
    const pinnedAfter = (await hover('Refresh sessions once')).some(a => a.name.startsWith('Unpin from Grid'));
    // Archive by keyboard: focus the row, Delete (⌘⌫ on macOS).
    const rowPt = await cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Split the payment service'); const b = r.getBoundingClientRect(); return { x: b.left + 120, y: b.top + b.height / 2 }; })()`);
    await cdp.click(rowPt.x, rowPt.y); await delay(1500);
    await cdp.command('View: Focus on Agents View'); await delay(500);
    await cdp.key('Backspace', { meta: true }); await delay(1500);
    const archived = s.ctl('state').tasks.find(t => t.id === runs.nested.task.id).archived_ms;
    // The context menu offers the rest.
    const ctx = await cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Refresh sessions once').pop(); const b = r.getBoundingClientRect(); return { x: b.left + 120, y: b.top + b.height / 2 }; })()`);
    await cdp.click(ctx.x, ctx.y, { button: 'right' }); await delay(800);
    const menu = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-menu .action-item .action-label')].map(a => a.getAttribute('aria-label') || a.textContent.trim()).filter(Boolean)`);
    await cdp.key('Escape');
    check('pin by mouse, archive by keyboard (⌘⌫ in the Agents view), and the context menu offers Open to the Side, Open Review, Merge Back and the rest',
      pinnedAfter && !!archived && ['Open to the Side', 'Open Review', 'Merge Back…', 'Unpin from Grid', 'Archive'].every(x => menu.some(m => m.startsWith(x.replace('…', '')))), { pinnedAfter, archived: !!archived, menu });
    // Archived agents are behind a filter.
    rows = await agentRows();
    const hidden = !rows.some(r => r.label === 'Split the payment service');
    await cdp.command('Overseer: Show Archived Agents'); await delay(1200);
    const inArchive = (await agentRows()).map(r => r.label);
    await cdp.command('Overseer: Show Active Agents'); await delay(800);
    check('archived agents leave the list and appear under Show Archived Agents', hidden && inArchive.includes('Split the payment service') && !inArchive.includes('Watch the build'), { hidden, inArchive });

    // Every row has a screen-reader label.
    rows = await agentRows();
    check('every row in the Agents view has a screen-reader label', rows.length > 5 && rows.every(r => r.aria && r.aria.length > 2), rows.map(r => r.aria));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { for (const r of Object.values(runs)) s.ctl('run.interrupt', { run_id: r.run.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
