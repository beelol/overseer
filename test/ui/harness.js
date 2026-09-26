// Shared setup for packaged-UI scenarios: isolated VS Code profile + extensions dir,
// VSIX installed with the real `code` CLI, isolated OVERSEER_HOME, fixture repos.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Cdp, delay } = require('./cdp');

const repoRoot = path.resolve(__dirname, '../..');
const CODE = process.env.OVERSEER_CODE || '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code';

function git(cwd, ...args) { return cp.execFileSync('git', args, { cwd, encoding: 'utf8' }).trim(); }

function makeRepo(dir, { dirty = true } = {}) {
  fs.mkdirSync(dir, { recursive: true });
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.name', 'Overseer Test'); git(dir, 'config', 'user.email', 'overseer-test@example.invalid');
  git(dir, 'config', 'commit.gpgsign', 'false'); git(dir, 'config', 'core.hooksPath', '/dev/null');
  const lines = Array.from({ length: 300 }, (_, i) => `L${i + 1}: original`).join('\n') + '\n';
  fs.writeFileSync(path.join(dir, 'a.txt'), lines); fs.writeFileSync(path.join(dir, 'b.txt'), lines);
  fs.writeFileSync(path.join(dir, 'README.md'), '# fixture\n'); fs.writeFileSync(path.join(dir, 'c.txt'), 'c base\n');
  git(dir, 'add', '.'); git(dir, 'commit', '-q', '-m', 'fixture base');
  if (dirty) {
    fs.appendFileSync(path.join(dir, 'README.md'), 'pre-existing unstaged edit\n');
    fs.writeFileSync(path.join(dir, 'c.txt'), 'c staged edit\n'); git(dir, 'add', 'c.txt');
    fs.writeFileSync(path.join(dir, 'notes.txt'), 'pre-existing untracked\n');
  }
  return dir;
}

function snapshotTree(dir) {
  const out = {};
  const walk = rel => {
    for (const name of fs.readdirSync(path.join(dir, rel))) {
      if (name === '.git') continue;
      const r = path.join(rel, name), abs = path.join(dir, r);
      if (fs.statSync(abs).isDirectory()) walk(r); else out[r] = fs.readFileSync(abs, 'utf8');
    }
  };
  walk('');
  return { files: out, status: git(dir, 'status', '--porcelain=v1'), stash: git(dir, 'stash', 'list'), head: git(dir, 'rev-parse', 'HEAD'), index: git(dir, 'diff', '--cached') };
}

class Session {
  /** ownerDaemon: use the owner's own daemon and data (no OVERSEER_HOME) — live sessions only. */
  constructor(name, { ownerDaemon = false } = {}) {
    this.name = name;
    this.root = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-ui-'));
    this.home = ownerDaemon ? null : path.join(this.root, 'overseer-home');
    this.profile = path.join(this.root, 'profile');
    this.extensions = path.join(this.root, 'extensions');
    this.evidence = path.join(repoRoot, 'docs/verification/evidence/ui', name);
    fs.rmSync(this.evidence, { recursive: true, force: true });
    fs.mkdirSync(this.evidence, { recursive: true });
    this.log = [];
    this.shot = 0;
  }

  baseEnv() {
    const env = { ...process.env };
    if (this.home) env.OVERSEER_HOME = this.home; else delete env.OVERSEER_HOME;
    return env;
  }

  note(msg, data) {
    const line = `[${new Date().toISOString()}] ${msg}${data === undefined ? '' : ' ' + JSON.stringify(data)}`;
    console.log(line); this.log.push(line);
  }

  settings(extra = {}) {
    fs.mkdirSync(path.join(this.profile, 'User'), { recursive: true });
    fs.writeFileSync(path.join(this.profile, 'User/settings.json'), JSON.stringify({
      'telemetry.telemetryLevel': 'off', 'extensions.autoUpdate': false, 'extensions.autoCheckUpdates': false,
      'git.autofetch': false, 'git.openRepositoryInParentFolders': 'always', 'workbench.startupEditor': 'none',
      'security.workspace.trust.enabled': false, 'files.autoSave': 'off', 'update.mode': 'none',
      'workbench.tips.enabled': false, 'chat.disableAIFeatures': true, 'window.restoreWindows': 'none',
      'editor.minimap.enabled': false, 'workbench.secondarySideBar.defaultVisibility': 'hidden', 'window.dialogStyle': 'custom', ...extra,
    }, null, 2));
  }

  install(vsix) {
    const out = cp.execFileSync(CODE, ['--user-data-dir', this.profile, '--extensions-dir', this.extensions, '--install-extension', vsix, '--force'], { encoding: 'utf8' });
    fs.writeFileSync(path.join(this.evidence, 'install.log'), `$ code --user-data-dir <profile> --extensions-dir <ext> --install-extension ${path.basename(vsix)} --force\n${out}\n` +
      cp.execFileSync(CODE, ['--user-data-dir', this.profile, '--extensions-dir', this.extensions, '--list-extensions', '--show-versions'], { encoding: 'utf8' }));
    this.note('installed', out.trim());
  }

  launch(folder, env = {}) {
    fs.rmSync(path.join(this.profile, 'DevToolsActivePort'), { force: true });
    this.child = cp.spawn(CODE, ['--remote-debugging-port=0', '--disable-renderer-backgrounding', '--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows',
      '--new-window', '--user-data-dir', this.profile, '--extensions-dir', this.extensions, '--skip-welcome', '--skip-release-notes', ...(env.OVERSEER_TEST_TRUST ? [] : ['--disable-workspace-trust']), ...(folder ? [folder] : [])],
    { env: { ...this.baseEnv(), ...env }, stdio: ['ignore', fs.openSync(path.join(this.root, 'code-' + Date.now() + '.log'), 'a'), fs.openSync(path.join(this.root, 'code-err-' + Date.now() + '.log'), 'a')], detached: false });
  }

  async connect() {
    this.cdp = await Cdp.connect(this.profile);
    await delay(1500);
    return this.cdp;
  }

  async screenshot(label) {
    const file = path.join(this.evidence, `${String(++this.shot).padStart(2, '0')}-${label}.png`);
    await this.cdp.screenshot(file);
    this.note('screenshot ' + path.relative(repoRoot, file));
    return file;
  }

  ctl(method, params = {}) {
    const bin = path.join(this.extensions, fs.readdirSync(this.extensions).find(d => d.startsWith('beelol.overseer')), 'bin', `overseerd-${process.platform}-${process.arch}`);
    const out = cp.execFileSync(bin, ['ctl', method, JSON.stringify(params)], { env: this.baseEnv(), encoding: 'utf8' });
    const msg = JSON.parse(out.split('\n')[0]);
    if (msg.error) throw new Error(msg.error.message);
    return msg.result;
  }

  async quit() {
    // Close VS Code the way a user does (Cmd+Q); fall back to SIGTERM.
    try { await this.cdp?.focusWorkbench(); await this.cdp?.key('q', { meta: true }); } catch {}
    for (let i = 0; i < 30; i++) {
      await delay(500);
      if (!cp.spawnSync('pgrep', ['-f', this.profile], { encoding: 'utf8' }).stdout.trim()) break;
    }
    try { this.cdp?.close(); } catch {}
    const pids = cp.spawnSync('pgrep', ['-f', this.profile], { encoding: 'utf8' }).stdout.trim().split('\n').filter(Boolean);
    if (pids.length) this.note('VS Code did not quit on Cmd+Q; sending SIGTERM', pids.length);
    for (const pid of pids) { try { process.kill(Number(pid), 'SIGTERM'); } catch {} }
    for (let i = 0; i < 40; i++) {
      await delay(500);
      if (!cp.spawnSync('pgrep', ['-f', this.profile], { encoding: 'utf8' }).stdout.trim()) break;
    }
    // Orphaned Electron helpers (network/GPU services) can outlive the app; never leave them running.
    const left = cp.spawnSync('pgrep', ['-f', this.profile], { encoding: 'utf8' }).stdout.trim().split('\n').filter(Boolean);
    if (left.length) { this.note('killing leftover VS Code helper processes', left.length); for (const pid of left) { try { process.kill(Number(pid), 'SIGKILL'); } catch {} } }
    await delay(500);
  }

  stopDaemon() {
    try { this.ctl('daemon.shutdown'); } catch {}
  }

  writeLog() {
    fs.writeFileSync(path.join(this.evidence, 'scenario.log'), this.log.join('\n') + '\n');
  }

  /** Shows the Overseer view container (clicking the activity icon only when it is not already visible). */
  async openOverseerView() {
    const visible = await this.cdp.evalWorkbench(`[...document.querySelectorAll('.pane-header')].some(h => h.offsetParent && /Agents/.test(h.textContent))`);
    if (visible) return;
    const icon = await this.cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item a, .activitybar .action-label')].find(a => /^Overseer/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000);
    await this.cdp.click(icon.x, icon.y);
    await delay(800);
  }

  /** Absolute page coordinates of an element inside a webview frame. */
  async webviewPoint(frame, selector) {
    const inner = await frame.eval(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.left + Math.min(r.width / 2, 40), y: r.top + Math.min(r.height / 2, 12), w: innerWidth, h: innerHeight }; })()`);
    if (!inner) throw new Error('element not found ' + selector);
    const frames = await this.cdp.evalWorkbench(`[...document.querySelectorAll('iframe.webview')].map(f => { const r = f.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, src: f.src }; }).filter(r => r.w > 0 && r.h > 0)`);
    const origin = frame.context.origin || '';
    const match = frames.find(f => origin && f.src.startsWith(origin)) || frames.find(f => Math.abs(f.w - inner.w) < 3 && Math.abs(f.h - inner.h) < 3) || frames[0];
    return { x: match.x + inner.x, y: match.y + inner.y };
  }
}

function startMock(root, env = {}) {
  const portFile = path.join(root, 'mock.port');
  const child = cp.spawn(process.execPath, [path.join(repoRoot, 'fixtures/mock-openai/server.js')], { env: { ...process.env, MOCK_PORT: '0', MOCK_PORT_FILE: portFile, MOCK_LOG: path.join(root, 'mock.log'), ...env }, stdio: 'ignore' });
  return { child, port: async () => { for (let i = 0; i < 50 && !fs.existsSync(portFile); i++) await delay(100); return Number(fs.readFileSync(portFile, 'utf8')); } };
}

function openCodeConfig(port) {
  return JSON.stringify({
    $schema: 'https://opencode.ai/config.json',
    provider: { mock: { npm: '@ai-sdk/openai-compatible', name: 'Mock (deterministic fixture)', options: { baseURL: `http://127.0.0.1:${port}/v1` }, models: { 'mock-coder': { name: 'Mock Coder', tool_call: true } } } },
    model: 'mock/mock-coder', small_model: 'mock/mock-coder', autoupdate: false, share: 'disabled',
    agent: { general: { permission: { task: 'allow' } } },
  }, null, 2);
}

function latestVsix() {
  const dir = path.join(repoRoot, 'extension');
  const vsix = fs.readdirSync(dir).filter(f => f.endsWith('.vsix')).sort().pop();
  if (!vsix) throw new Error('Build the VSIX first: node extension/scripts/package.js');
  return path.join(dir, vsix);
}

module.exports = { Session, makeRepo, snapshotTree, startMock, openCodeConfig, latestVsix, git, delay, repoRoot };
