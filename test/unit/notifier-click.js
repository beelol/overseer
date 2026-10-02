// You hear about it outside VS Code (AC-240): where a click on an agent's notification goes, from
// the notifier's own routing (extension/notifier/main.swift, built here with swiftc and run with
// --simulate-click, which opens nothing). VS Code running: that agent in VS Code. VS Code closed:
// the TUI on that agent in Terminal, through a .command file (Terminal's own file type); a stand-in
// TUI runs that file here, so no terminal opens. No TUI found: VS Code's URL, which starts VS Code.
// Run: node test/unit/notifier-click.js (macOS with the Xcode command-line tools; skipped otherwise)
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

if (process.platform !== 'darwin') { console.log('skip: the notifier is macOS only'); process.exit(0); }
try { execFileSync('xcrun', ['--find', 'swiftc'], { stdio: 'ignore' }); } catch { console.log('skip: no swiftc'); process.exit(0); }

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-notifier-click-'));
let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };
try {
  // The same overlay as extension/notifier/build.js for Command Line Tools with two SwiftBridging maps.
  const flags = [];
  const inc = '/Library/Developer/CommandLineTools/usr/include/swift';
  if (fs.existsSync(path.join(inc, 'module.modulemap')) && fs.existsSync(path.join(inc, 'bridging.modulemap'))) {
    fs.writeFileSync(path.join(tmp, 'empty.modulemap'), '');
    fs.writeFileSync(path.join(tmp, 'overlay.yaml'), JSON.stringify({ version: 0, 'case-sensitive': 'false', roots: [{ type: 'directory', name: inc, contents: [{ type: 'file', name: 'module.modulemap', 'external-contents': path.join(tmp, 'empty.modulemap') }] }] }));
    flags.push('-vfsoverlay', path.join(tmp, 'overlay.yaml'), '-Xcc', '-ivfsoverlay', '-Xcc', path.join(tmp, 'overlay.yaml'));
  }
  const bin = path.join(tmp, 'notifier');
  execFileSync('swiftc', ['-Onone', ...flags, path.resolve(__dirname, '../../extension/notifier/main.swift'), '-o', bin], { stdio: ['ignore', 'ignore', 'inherit'] });
  const files = path.join(tmp, 'T'); fs.mkdirSync(files);
  const click = args => execFileSync(bin, ['--simulate-click', ...args], { encoding: 'utf8', env: { ...process.env, TMPDIR: files + '/' } }).trim();
  const url = 'vscode://beelol.overseer/open-agent?run=r-abc';
  // A stand-in TUI in a folder with a space (the daemon quotes the command for the shell).
  const tuiDir = path.join(tmp, 'bin dir'); fs.mkdirSync(tuiDir);
  const tui = path.join(tuiDir, 'overseer-tui'), argsLog = path.join(tmp, 'tui-args');
  fs.writeFileSync(tui, `#!/bin/sh\nprintf '%s\\n' "$@" > '${argsLog}'\n`); fs.chmodSync(tui, 0o755);
  const command = `'${tui}' --focus 'r-abc' --home '/tmp/some home'`;

  test('VS Code running: the click opens that agent in VS Code', () => {
    assert.strictEqual(click(['--open', url, '--tui', command, '--vscode', 'running']), `open ${url}`);
    assert.deepStrictEqual(fs.readdirSync(files), [], 'no Terminal file');
  });
  test('VS Code closed: the click opens the TUI on that agent in Terminal (a .command file that runs it and removes itself)', () => {
    const out = click(['--open', url, '--tui', command, '--vscode', 'closed']);
    const m = /^terminal (.+\.command)$/.exec(out);
    assert.ok(m, out);
    const file = m[1];
    assert.ok(fs.statSync(file).mode & 0o100, 'executable');
    const text = fs.readFileSync(file, 'utf8');
    assert.ok(text.startsWith('#!/bin/sh\n') && text.trimEnd().endsWith(`exec ${command}`), text);
    // What Terminal does with it: run it.
    execFileSync('/bin/sh', [file]);
    assert.deepStrictEqual(fs.readFileSync(argsLog, 'utf8').trim().split('\n'), ['--focus', 'r-abc', '--home', '/tmp/some home']);
    assert.ok(!fs.existsSync(file), 'the file removes itself');
  });
  test('VS Code closed and no TUI found: the click opens VS Code (its URL starts it)', () => {
    assert.strictEqual(click(['--open', url, '--vscode', 'closed']), `open ${url}`);
  });
  test('only VS Code URLs are opened', () => {
    assert.strictEqual(click(['--open', 'https://example.com', '--vscode', 'running']), 'nothing');
  });
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
console.log(`${passed} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
