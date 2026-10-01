// Builds bin/Overseer Menu.app (AC-262, macOS only): Overseer's item in the menu bar. A universal
// Swift binary, Info.plist, the status icon made from the owner's single-colour silhouette
// (docs/design/brand/overseer-icon-flat.png, drawn as a template image, AC-179), the app icon of
// the notifier, and an ad-hoc signature. Requires the Xcode command-line tools (swiftc, lipo,
// codesign) and the system sips/iconutil. OVERSEER_MENUBAR_ARCH=host builds this Mac's
// architecture only (scripts/dev, where speed matters more than a universal binary).
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

if (process.platform !== 'darwin') { console.log('menubar: skipped (not macOS)'); process.exit(0); }
const here = __dirname;
const repo = path.resolve(here, '..', '..');
const app = path.join(here, '..', 'bin', 'Overseer Menu.app');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-menubar-'));
const run = (cmd, args) => execFileSync(cmd, args, { stdio: ['ignore', 'pipe', 'inherit'] });
try {
  // The same workaround as the notifier: some Command Line Tools ship two SwiftBridging modulemaps.
  const flags = [];
  const inc = '/Library/Developer/CommandLineTools/usr/include/swift';
  if (fs.existsSync(path.join(inc, 'module.modulemap')) && fs.existsSync(path.join(inc, 'bridging.modulemap'))) {
    fs.writeFileSync(path.join(tmp, 'empty.modulemap'), '');
    fs.writeFileSync(path.join(tmp, 'overlay.yaml'), JSON.stringify({ version: 0, 'case-sensitive': 'false', roots: [{ type: 'directory', name: inc, contents: [{ type: 'file', name: 'module.modulemap', 'external-contents': path.join(tmp, 'empty.modulemap') }] }] }));
    flags.push('-vfsoverlay', path.join(tmp, 'overlay.yaml'), '-Xcc', '-ivfsoverlay', '-Xcc', path.join(tmp, 'overlay.yaml'));
  }
  const archs = process.env.OVERSEER_MENUBAR_ARCH === 'host' ? [process.arch === 'arm64' ? 'arm64' : 'x86_64'] : ['arm64', 'x86_64'];
  for (const arch of archs) run('swiftc', ['-O', '-swift-version', '5', '-target', `${arch}-apple-macos13`, ...flags, path.join(here, 'main.swift'), '-o', path.join(tmp, `menu-${arch}`)]);
  fs.rmSync(app, { recursive: true, force: true });
  fs.mkdirSync(path.join(app, 'Contents/MacOS'), { recursive: true });
  fs.mkdirSync(path.join(app, 'Contents/Resources'), { recursive: true });
  run('lipo', ['-create', ...archs.map(a => path.join(tmp, `menu-${a}`)), '-output', path.join(app, 'Contents/MacOS/overseer-menu')]);
  const flat = path.join(repo, 'docs/design/brand/overseer-icon-flat.png');
  const master = path.join(here, '..', 'notifier', 'AppIcon.png');
  // The build number follows the code and the icons, so macOS never shows a stale one (as the notifier).
  const digest = require('crypto').createHash('sha256').update(fs.readFileSync(path.join(here, 'main.swift'))).update(fs.readFileSync(flat)).update(fs.readFileSync(master)).digest();
  fs.writeFileSync(path.join(app, 'Contents/Info.plist'), fs.readFileSync(path.join(here, 'Info.plist'), 'utf8').replace('<key>CFBundleVersion</key><string>1</string>', `<key>CFBundleVersion</key><string>1.${digest.readUInt32BE(0)}</string>`));
  // The menu-bar mark: 18 pt, at 1x and 2x, from the owner's flat silhouette (never redrawn).
  run('sips', ['-z', '18', '18', flat, '--out', path.join(app, 'Contents/Resources/StatusIcon.png')]);
  run('sips', ['-z', '36', '36', flat, '--out', path.join(app, 'Contents/Resources/StatusIcon@2x.png')]);
  const set = path.join(tmp, 'AppIcon.iconset'); fs.mkdirSync(set);
  for (const size of [16, 32, 128, 256, 512]) {
    run('sips', ['-z', String(size), String(size), master, '--out', path.join(set, `icon_${size}x${size}.png`)]);
    run('sips', ['-z', String(size * 2), String(size * 2), master, '--out', path.join(set, `icon_${size}x${size}@2x.png`)]);
  }
  run('iconutil', ['-c', 'icns', set, '-o', path.join(app, 'Contents/Resources/AppIcon.icns')]);
  run('codesign', ['--force', '--sign', '-', '--timestamp=none', app]);
  console.log('menubar:', app);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
