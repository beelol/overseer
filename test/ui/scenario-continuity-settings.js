// Packaged-UI scenario for AC-88: the Continuity settings are edited in VS Code, pushed to the
// daemon, persisted and enforced there, with VS Code closed too. Fixture harnesses (SYNTHETIC
// Codex that fails on command), a fixture network and a synthetic Ollama; no account, no tokens.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const net = require('net');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const freePort = () => new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); }); });
const writeWhole = (file, text) => { fs.writeFileSync(file + '.tmp', text); fs.renameSync(file + '.tmp', file); };
const G = 2 ** 30;
const WANTED = { enabled: false, providerOrder: ['anthropic', 'openai'], allowModelDownloads: true, allowOllamaInstall: true, prefetch: true, ramCeilingPercent: 35, ramHeadroomGiB: 6, contextTarget: 32768, contextFloor: 8192,
  preferredModels: ['qwen3-coder:30b'], allowUnverifiedModels: true, localHarness: 'codex', returnOnline: 'stay', retryCapSeconds: 60, retryForHours: 12, stallSeconds: 45, probes: false, ollamaIdleMinutes: 5, registry: 'https://mirror.example.invalid' };

(async () => {
  const s = new Session('continuity-settings');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const netFile = path.join(s.root, 'net.json'), memFile = path.join(s.root, 'memory.json'), control = path.join(s.root, 'harness.json');
  writeWhole(netFile, JSON.stringify({ system: 'connected', baseline: { by_name: true, by_ip: true }, providers: { openai: true, anthropic: true } }));
  writeWhole(memFile, JSON.stringify({ total: 128 * G, available: 115 * G, pressure: 'normal' }));
  writeWhole(control, JSON.stringify({ codex: 'network', claude: 'ok' }));
  const sys = path.join(s.root, 'system-home'); fs.mkdirSync(path.join(sys, '.codex'), { recursive: true });
  const port = await freePort();
  const ollama = cp.spawn(fx('ollama-fixture.js'), ['serve'], { env: { ...process.env, OLLAMA_HOST: `127.0.0.1:${port}`, OLLAMA_FIXTURE_MODELS: path.join(repoRoot, 'fixtures/continuity/ui-models.json') }, stdio: 'ignore' });
  const settingsFile = path.join(s.profile, 'User/settings.json');
  const writeSettings = extra => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); for (const [k, v] of Object.entries(extra)) { if (v === undefined) delete cur[k]; else cur[k] = v; } fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); };
  try {
    const repo = makeRepo(path.join(s.root, 'settings-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(process.env.CONTINUITY_VSIX || latestVsix());
    s.launch(repo, {
      OVERSEER_CODEX_PATH: fx('continuity-harness.js'), OVERSEER_CLAUDE_PATH: fx('continuity-harness.js'), OVERSEER_OPENCODE_PATH: fx('opencode-serve-fixture.js'),
      OVERSEER_OLLAMA_URL: `http://127.0.0.1:${port}`, OVERSEER_OLLAMA_CANDIDATES: fx('ollama-fixture.js'), OVERSEER_TEST_NET: netFile, OVERSEER_TEST_MEMORY: memFile, OVERSEER_TEST_SYSTEM_HOME: sys,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CONTINUITY_FIXTURE', CONTINUITY_FIXTURE: control, OVERSEER_TEST_CONTINUITY_TICK_MS: '400', OVERSEER_TEST_PROBE_MS: '400', OVERSEER_TEST_PROBE_IDLE_MS: '400',
      OVERSEER_TEST_RETRY_BASE_MS: '3000', OVERSEER_TEST_RETRY_CAP_MS: '12000', OVERSEER_TEST_STALL_MS: '60000',
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /^Connection:/.test(e.querySelector('[aria-label]')?.getAttribute('aria-label') || ''))`, 30000, 'connection item');
    const defaults = s.ctl('settings.get');
    check('the daemon starts with its defaults, and reports them', defaults.settings.enabled === true && defaults.settings.ramCeilingPercent === 40 && defaults.settings.retryForHours === 36 && JSON.stringify(defaults.settings) === JSON.stringify(defaults.defaults), { enabled: defaults.settings.enabled });

    // Every setting changed in VS Code arrives in the daemon.
    writeSettings(Object.fromEntries(Object.entries(WANTED).map(([k, v]) => [`overseer.continuity.${k}`, v])));
    let got; for (let i = 0; i < 40; i++) { await delay(500); got = s.ctl('settings.get').settings; if (got.retryForHours === 12 && got.registry === WANTED.registry) break; }
    const wrong = Object.entries(WANTED).filter(([k, v]) => JSON.stringify(got[k]) !== JSON.stringify(v)).map(([k]) => `${k}: ${JSON.stringify(got[k])}`);
    check('every Continuity setting edited in VS Code is read back from the daemon', wrong.length === 0, wrong.length ? wrong : `${Object.keys(WANTED).length} settings`);
    await s.screenshot('settings-in-daemon');

    // A value out of range is refused with the reason; the daemon keeps its value and VS Code shows it again.
    writeSettings({ 'overseer.continuity.ramCeilingPercent': 80 });
    const toast = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast, .notification-list-item')].map(t => t.innerText).find(t => /ramCeilingPercent/.test(t)) || null`, 20000).catch(() => null);
    await delay(1500);
    const kept = s.ctl('settings.get').settings.ramCeilingPercent;
    const shown = JSON.parse(fs.readFileSync(settingsFile, 'utf8'))['overseer.continuity.ramCeilingPercent'];
    check('a value out of range is refused with the reason, the daemon keeps its value, and VS Code shows it again', /ramCeilingPercent must be between 10 and 50 percent/.test(toast || '') && /80 was refused/.test(toast || '') && kept === 35 && shown === 35, { toast, kept, shown });
    await s.screenshot('refused');
    const status = s.ctl('continuity.status');
    check('`overseerd ctl continuity.status` prints the state, the budget and the pick', status.connection.state === 'online' && typeof status.budget.budget === 'number' && status.settings.ramCeilingPercent === 35 && (status.pick === null || typeof status.pick.tag === 'string'), { state: status.connection.state, budget: status.budget.budget, pick: status.pick && status.pick.tag });

    // VS Code closed: the daemon still applies Continuity off (the run waits) and the download setting.
    await s.quit();
    const alive = (() => { try { return s.ctl('hello'); } catch { return null; } })();
    check('the daemon keeps running after VS Code quits', !!alive);
    writeWhole(netFile, JSON.stringify({ system: 'none' }));
    let conn; for (let i = 0; i < 40; i++) { await delay(400); conn = s.ctl('connection.status').status; if (conn.state === 'offline') break; }
    const t = s.ctl('task.create', { repo, harness: 'codex', title: 'While VS Code is closed', prompt: 'write closed.txt hello; say done' });
    let st; for (let i = 0; i < 60; i++) { await delay(400); st = s.ctl('state').runs.find(r => r.id === t.run.id)?.status; if (/waiting|handed|failed|completed/.test(st || '')) break; }
    await delay(4000);
    const later = s.ctl('state').runs.find(r => r.id === t.run.id);
    const handoffs = s.ctl('continuity.handoffs').handoffs;
    check('with Continuity off (set in VS Code) an offline agent waits instead of transitioning, VS Code closed', st === 'waiting_for_connection' && later.status === 'waiting_for_connection' && handoffs.length === 0 && s.ctl('settings.get').settings.enabled === false, { st, later: later.status, handoffs: handoffs.length });
    const pull = s.ctl('local.downloads');
    check('the download setting is the daemon\'s too (allowed, first pull not yet confirmed)', s.ctl('settings.get').settings.allowModelDownloads === true && pull.first_pull_confirmed === false, pull);
    s.ctl('run.interrupt', { run_id: t.run.id });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    ollama.kill();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
