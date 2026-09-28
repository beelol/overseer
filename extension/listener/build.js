// Builds bin/Overseer Listener.app (Voice Mode, AC-163, macOS only): the Rust listener
// (`cargo build --release -p overseer-listener`) inside a bundle with its own identifier, the
// microphone's usage description and Overseer's app icon (AC-142), signed ad hoc. The daemon starts
// it with its responsibility disclaimed, so macOS asks for the microphone in Overseer's name.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

if (process.platform !== 'darwin') { console.log('listener: skipped (not macOS)'); process.exit(0); }
const here = __dirname;
const repo = path.resolve(here, '..', '..');
const app = path.join(here, '..', 'bin', 'Overseer Listener.app');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-listener-'));
const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: ['ignore', 'pipe', 'inherit'] });
try {
  execFileSync('cargo', ['build', '--release', '-p', 'overseer-listener'], { cwd: repo, stdio: 'inherit' });
  const target = process.env.CARGO_TARGET_DIR || path.join(repo, 'target');
  fs.rmSync(app, { recursive: true, force: true });
  fs.mkdirSync(path.join(app, 'Contents/MacOS'), { recursive: true });
  fs.mkdirSync(path.join(app, 'Contents/Resources'), { recursive: true });
  fs.copyFileSync(path.join(target, 'release/overseer-listener'), path.join(app, 'Contents/MacOS/overseer-listener'));
  fs.chmodSync(path.join(app, 'Contents/MacOS/overseer-listener'), 0o755);
  fs.copyFileSync(path.join(here, 'Info.plist'), path.join(app, 'Contents/Info.plist'));
  // The same icon as the notifier: Overseer's app icon on the macOS grid.
  const master = path.join(here, '..', 'notifier', 'AppIcon.png');
  const set = path.join(tmp, 'AppIcon.iconset'); fs.mkdirSync(set);
  for (const size of [16, 32, 128, 256, 512]) {
    run('sips', ['-z', String(size), String(size), master, '--out', path.join(set, `icon_${size}x${size}.png`)]);
    run('sips', ['-z', String(size * 2), String(size * 2), master, '--out', path.join(set, `icon_${size}x${size}@2x.png`)]);
  }
  run('iconutil', ['-c', 'icns', set, '-o', path.join(app, 'Contents/Resources/AppIcon.icns')]);
  run('codesign', ['--force', '--sign', '-', '--timestamp=none', app]);
  console.log('listener:', app);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
