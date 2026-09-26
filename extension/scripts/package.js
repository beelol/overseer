// Development-only packager: builds the review bundle and the release daemon, copies the
// daemon binary for this platform into bin/, and packages the VSIX with pinned vsce.
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const repo = path.resolve(root, '..');
const run = (cmd, args, cwd) => {
  const r = spawnSync(cmd, args, { cwd, stdio: 'inherit', shell: false });
  if (r.error || r.status !== 0) { console.error(`${cmd} ${args.join(' ')} failed`); process.exit(r.status || 1); }
};
const toolRoot = path.join(root, 'tooling', 'vsce');
const tool = path.join(toolRoot, 'node_modules', '@vscode', 'vsce');
const installed = JSON.parse(fs.readFileSync(path.join(tool, 'package.json'), 'utf8'));
const required = JSON.parse(fs.readFileSync(path.join(toolRoot, 'package.json'), 'utf8')).devDependencies['@vscode/vsce'];
if (installed.version !== required) { console.error(`Expected vsce ${required}; run npm ci --prefix extension/tooling/vsce --ignore-scripts.`); process.exit(1); }
run(process.execPath, [path.join(root, 'branch-diff/scripts/build-review.js')], root);
run(process.execPath, [path.join(root, 'notifier/build.js')], root);
run('cargo', ['build', '--release', '-p', 'overseerd'], repo);
fs.mkdirSync(path.join(root, 'bin'), { recursive: true });
const target = path.join(root, 'bin', `overseerd-${process.platform}-${process.arch}`);
fs.copyFileSync(path.join(repo, 'target/release/overseerd'), target);
fs.chmodSync(target, 0o755);
const out = path.join(root, `overseer-${JSON.parse(fs.readFileSync(path.join(root, 'package.json'))).version}.vsix`);
run(process.execPath, [path.resolve(tool, installed.bin.vsce), 'package', '--no-dependencies', '--allow-missing-repository', '-o', out], root);
console.log(out);
