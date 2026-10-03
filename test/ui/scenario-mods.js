// Slice1B packaged UI. All turns use the synthetic Claude fixture; no paid or owner login calls.
// Rendered evidence is pending until this scenario actually runs against this branch's VSIX.
const fs = require('fs'), path = require('path');
const { Session, makeRepo, latestVsix, delay, until, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');
(async () => {
  const s = new Session('mods'); const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mode = path.join(s.root, 'fixture-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'mods-repo'), { dirty: false }); fs.writeFileSync(mode, 'slow');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' }); s.install(process.env.MODS_VSIX || latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: mode, FIXTURE_SLOW_MS: '60000', OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS' });
    const cdp = await s.connect(); await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer [0-9]+ active/.test(e.textContent))`, 90000, 'extension activation');
    await cdp.command('Overseer: Mods');
    const view = await until(async () => (await cdp.webviews(`!!document.getElementById('mods')`))[0], Boolean, 20000);
    if (!view) throw new Error('Mods webview did not open');
    const ready = () => view.waitFor(`document.getElementById('mods').getAttribute('aria-busy') === 'false'`, 10000);
    const click = async label => { await view.eval(`(() => { const b = [...document.querySelectorAll('#mods button')].find(b => b.textContent === ${JSON.stringify(label)}); if (!b || b.disabled) throw new Error('Missing enabled control: ' + ${JSON.stringify(label)}); b.click(); })()`); };
    const dialog = async label => { const b = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); const b = d && [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(label)}); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, label + ' confirmation'); await cdp.click(b.x, b.y); };
    await view.waitFor(`document.body.textContent.includes('No mods installed')`);
    check('Clear prose off and Less tool noise planned without active switch', !s.ctl('mods.list').installed.length && await view.eval(`(() => { const r = [...document.querySelectorAll('.mod-row')].find(r => r.textContent.startsWith('Less tool noise')); return /Planned; not available/.test(r?.textContent || '') && !r.querySelector('button,input'); })()`));
    await click('Preview Clear prose'); await view.waitFor(`!!document.getElementById('mod-preview')`);
    check('preview displays contents and does not install', await view.eval(`document.getElementById('mod-preview').textContent.includes('style.md')`) && s.ctl('mods.list').installed.length === 0);
    await click('Install after confirmation…'); await dialog('Install'); await view.waitFor(`document.body.textContent.includes('Installed; no enabled bindings')`);
    const v = s.ctl('mods.list').installed[0]; check('confirmed installation keeps bindings empty', !!v && s.ctl('mods.list').bindings.length === 0);
    await view.eval(`document.querySelector('form').closest('details').open = true`); await click('Enable for scope');
    await until(() => s.ctl('mods.list').bindings.some(b => b.scope.kind === 'all_agents' && b.enabled), Boolean, 10000);
    const run = s.ctl('task.create', { repo, harness: 'claude', title: 'Mods fixture agent', prompt: 'Synthetic long fixture turn' }).run.id;
    await until(() => s.ctl('mods.why', { run_id: run }).last_turn?.outcome === 'transport_accepted', Boolean, 10000);
    await click('Refresh'); await ready();
    await view.eval(`(() => { const n = document.getElementById('mod-inspect-run'); n.value = ${JSON.stringify(run)}; n.dispatchEvent(new Event('change')); })()`);
    await view.waitFor(`document.body.textContent.includes('Message text accepted by transport')`);
    const before = s.ctl('mods.why', { run_id: run }).last_turn;
    await click('Disable binding'); await view.waitFor(`document.body.textContent.includes('Pending next turn')`);
    const after = s.ctl('mods.why', { run_id: run });
    check('mid-turn disable leaves immutable applied text and shows pending desired change', after.pending && after.last_turn.digest === before.digest && after.last_turn.applied_fingerprints.includes(v.fingerprint) && after.desired.versions.length === 0);
    // A separate Overseer scope is configured through the same visible form, without a paid turn.
    await view.eval(`(() => { const f = document.querySelector('form'); f.closest('details').open = true; const n = f.querySelector('select'); n.value = 'overseer'; n.dispatchEvent(new Event('change')); })()`);
    await click('Enable for scope'); await until(() => s.ctl('mods.list').bindings.some(b => b.scope.kind === 'overseer' && b.enabled), Boolean, 10000);
    check('Overseer has its own binding while the all-agents binding remains disabled', s.ctl('mods.list').bindings.some(b => b.scope.kind === 'overseer' && b.enabled) && s.ctl('mods.list').bindings.some(b => b.scope.kind === 'all_agents' && !b.enabled));
    await cdp.command('Overseer: Reconnect to Daemon'); await view.waitFor(`document.body.textContent.includes('Overseer · Enabled')`);
    check('reconnect reads retained bindings without enabling anything new', s.ctl('mods.list').bindings.length === 2 && await view.eval(`document.body.textContent.includes('Recent mod changes')`));
    const settingsFile = path.join(s.profile, 'User/settings.json');
    for (const theme of ['Overseer Dark', 'Overseer Light', 'Overseer']) {
      const settings = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); settings['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2));
      await view.waitFor(`document.body.getAttribute('data-vscode-theme-name') === ${JSON.stringify(theme)}`, 20000);
      for (const width of [1440, 640]) {
        await cdp.call('Emulation.setDeviceMetricsOverride', { width, height: 1000, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1200);
        const audit = await view.eval(auditExpression());
        const names = await view.eval(`([...document.querySelectorAll('input,select')].filter(e => e.offsetParent !== null && !e.closest('label') && !e.getAttribute('aria-label'))).length`);
        const size = await view.eval(`({ width: innerWidth, overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1 })`);
        check(`${theme} at ${width}: named controls and no horizontal overflow`, !size.overflow && !audit.overflow?.length && !audit.unnamed?.length && names === 0, { size, audit, unnamedFields: names });
        await view.eval(`window.scrollTo(0,0)`); await s.screenshot(`mods-${theme.toLowerCase().replace(/ /g, '-')}-${width}`);
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench);
    s.ctl('run.interrupt', { run_id: run });
    await click('Remove pinned version…'); await dialog('Remove'); await view.waitFor(`document.body.textContent.includes('No mods installed')`);
    check('confirmed removal ends bindings and keeps historical snapshot', s.ctl('mods.list').installed.length === 0 && s.ctl('mods.list').bindings.length === 0 && s.ctl('mods.why', { run_id: run }).last_turn.digest === before.digest);
  } catch (e) { result.error = e.message; s.note('ERROR ' + e.stack); try { await s.screenshot('error'); } catch {} }
  finally { s.writeLog(); fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2)); await s.quit(); s.stopDaemon(); const failed = result.error || result.checks.some(c => !c.ok); console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root); process.exit(failed ? 1 : 0); }
})();
