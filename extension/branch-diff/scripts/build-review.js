// Build-time only. No installs or network requests; dependencies come from the reviewed lockfile.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '..');
const toolRoot = path.join(root, 'tooling/review');
const expected = JSON.parse(fs.readFileSync(path.join(toolRoot, 'package.json')));
for (const name of ['monaco-editor', 'esbuild']) {
  const installed = require(path.join(toolRoot, 'node_modules', name, 'package.json'));
  if (installed.version !== expected.devDependencies[name]) throw new Error(`Install locked review tools before packaging: ${name}`);
}
const esbuild = require(path.join(toolRoot, 'node_modules/esbuild'));
async function build() {
  const outdir = path.join(root, 'dist');
  fs.mkdirSync(outdir, { recursive: true });
  for (const file of fs.readdirSync(outdir)) fs.rmSync(path.join(outdir, file), { recursive: true });
  const common = { absWorkingDir: root, bundle: true, minify: true, target: 'chrome130', logLevel: 'warning',
    nodePaths: [path.join(toolRoot, 'node_modules')], loader: { '.ttf': 'file' }, legalComments: 'eof', metafile: true };
  const worker = await esbuild.build({ ...common, entryPoints: [path.join(toolRoot, 'node_modules/monaco-editor/esm/vs/editor/editor.worker.js')],
    outfile: path.join(outdir, 'editor.worker.js'), format: 'iife', write: false, footer: { js: 'globalThis.postMessage({type: "branch-diff-worker-ready"});' } });
  const statistics = await esbuild.build({ ...common, entryPoints: ['review/statistics-worker.js'], outfile: path.join(outdir, 'statistics.worker.js'), format: 'iife', write: false });
  const editor = await esbuild.build({ ...common, entryPoints: { monaco: 'review/monaco.js' }, outdir, format: 'esm', splitting: true,
    define: { __BRANCH_DIFF_WORKER_SOURCE__: JSON.stringify(worker.outputFiles[0].text) } });
  const result = await esbuild.build({ ...common, entryPoints: { review: 'review/browser.js' }, outdir, format: 'esm',
    define: { __BRANCH_DIFF_STATISTICS_SOURCE__: JSON.stringify(statistics.outputFiles[0].text) } });
  // Dynamic language chunks repeat core CSS already included in the main stylesheet.
  // Drop only byte-identical subsets; keep any future distinct required styles.
  const mainCss = fs.readFileSync(path.join(outdir, 'monaco.css'), 'utf8');
  for (const file of fs.readdirSync(outdir)) {
    if (file !== 'monaco.css' && file.endsWith('.css') && mainCss.includes(fs.readFileSync(path.join(outdir, file), 'utf8').trim())) fs.unlinkSync(path.join(outdir, file));
  }
  fs.writeFileSync(path.join(toolRoot, 'bundle-inputs.json'), JSON.stringify([...new Set([...Object.keys(result.metafile.inputs), ...Object.keys(editor.metafile.inputs), ...Object.keys(worker.metafile.inputs), ...Object.keys(statistics.metafile.inputs)])].sort(), null, 2) + '\n');
  const notices = ['monaco-editor/LICENSE', 'monaco-editor/ThirdPartyNotices.txt', 'dompurify/LICENSE', 'marked/LICENSE.md'];
  fs.writeFileSync(path.join(outdir, 'THIRD-PARTY-NOTICES.txt'), notices.map(file => {
    const full = path.join(toolRoot, 'node_modules', file);
    return file + '\n\n' + fs.readFileSync(full, 'utf8');
  }).join('\n\n========================================\n\n'));
}
module.exports = { build };
if (require.main === module) build().catch(error => { console.error(error); process.exitCode = 1; });
