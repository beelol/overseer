// Builds bin/Overseer Notifier.app (AC-52, macOS only): a universal Swift binary, Info.plist,
// an .icns rendered from AppIcon.svg, and an ad-hoc signature. Requires the Xcode command-line
// tools (swiftc, lipo, codesign) and the system qlmanage/sips/iconutil.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

if (process.platform !== 'darwin') { console.log('notifier: skipped (not macOS)'); process.exit(0); }
const here = __dirname;
const app = path.join(here, '..', 'bin', 'Overseer Notifier.app');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-notifier-'));
const run = (cmd, args) => execFileSync(cmd, args, { stdio: ['ignore', 'pipe', 'inherit'] });
try {
  // Some Command Line Tools releases ship two modulemaps that both define SwiftBridging, which
  // breaks every swiftc build; hide the duplicate from this build with a VFS overlay.
  const flags = [];
  const inc = '/Library/Developer/CommandLineTools/usr/include/swift';
  if (fs.existsSync(path.join(inc, 'module.modulemap')) && fs.existsSync(path.join(inc, 'bridging.modulemap'))) {
    fs.writeFileSync(path.join(tmp, 'empty.modulemap'), '');
    fs.writeFileSync(path.join(tmp, 'overlay.yaml'), JSON.stringify({ version: 0, 'case-sensitive': 'false', roots: [{ type: 'directory', name: inc, contents: [{ type: 'file', name: 'module.modulemap', 'external-contents': path.join(tmp, 'empty.modulemap') }] }] }));
    flags.push('-vfsoverlay', path.join(tmp, 'overlay.yaml'), '-Xcc', '-ivfsoverlay', '-Xcc', path.join(tmp, 'overlay.yaml'));
  }
  for (const arch of ['arm64', 'x86_64']) run('swiftc', ['-O', '-target', `${arch}-apple-macos12`, ...flags, path.join(here, 'main.swift'), '-o', path.join(tmp, `notifier-${arch}`)]);
  fs.rmSync(app, { recursive: true, force: true });
  fs.mkdirSync(path.join(app, 'Contents/MacOS'), { recursive: true });
  fs.mkdirSync(path.join(app, 'Contents/Resources'), { recursive: true });
  run('lipo', ['-create', path.join(tmp, 'notifier-arm64'), path.join(tmp, 'notifier-x86_64'), '-output', path.join(app, 'Contents/MacOS/notifier')]);
  fs.copyFileSync(path.join(here, 'Info.plist'), path.join(app, 'Contents/Info.plist'));
  // Icon: render the SVG once at 1024 px, then every size macOS asks for.
  // Transparent 1024 px master via AppKit; qlmanage (fallback) flattens onto white.
  const master = path.join(tmp, 'AppIcon.svg.png');
  try {
    run('swiftc', ['-O', ...flags, path.join(here, 'render-icon.swift'), '-o', path.join(tmp, 'render-icon')]);
    run(path.join(tmp, 'render-icon'), [path.join(here, 'AppIcon.svg'), master]);
  } catch {
    run('qlmanage', ['-t', '-s', '1024', '-o', tmp, path.join(here, 'AppIcon.svg')]);
  }
  const set = path.join(tmp, 'AppIcon.iconset'); fs.mkdirSync(set);
  for (const size of [16, 32, 128, 256, 512]) {
    run('sips', ['-z', String(size), String(size), master, '--out', path.join(set, `icon_${size}x${size}.png`)]);
    run('sips', ['-z', String(size * 2), String(size * 2), master, '--out', path.join(set, `icon_${size}x${size}@2x.png`)]);
  }
  run('iconutil', ['-c', 'icns', set, '-o', path.join(app, 'Contents/Resources/AppIcon.icns')]);
  run('codesign', ['--force', '--sign', '-', '--timestamp=none', app]);
  console.log('notifier:', app);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
