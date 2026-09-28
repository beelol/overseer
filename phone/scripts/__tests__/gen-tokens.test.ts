import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { motion, palettes, scale } from '@/theme/tokens.generated';

const phoneRoot = path.join(__dirname, '..', '..');
const script = path.join(phoneRoot, 'scripts', 'gen-tokens.mjs');
const designSource = path.join(phoneRoot, '..', 'extension', 'design');

interface DesignSource {
  scale: { motion: Record<string, number> } & Record<string, unknown>;
  palettes: { dark: Record<string, unknown>; light: Record<string, unknown> };
}

function run(...args: string[]) {
  const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
  return { status: result.status, output: `${result.stdout}${result.stderr}` };
}

describe('the token generator', () => {
  let work: string;
  let source: string;
  let out: string;

  beforeEach(() => {
    work = fs.mkdtempSync(path.join(os.tmpdir(), 'overseer-tokens-'));
    source = path.join(work, 'design');
    out = path.join(work, 'tokens.generated.ts');
    fs.mkdirSync(source);
    for (const file of ['tokens.js', 'build-themes.js']) {
      fs.copyFileSync(path.join(designSource, file), path.join(source, file));
    }
  });

  afterEach(() => {
    fs.rmSync(work, { recursive: true, force: true });
  });

  test('the file in the repository matches the VS Code source', () => {
    const result = run('--check');
    expect(result.output).toContain('matches its source');
    expect(result.status).toBe(0);
  });

  test('what it generates is what the VS Code themes are built from', () => {
    const design = jest.requireActual<DesignSource>(path.join(designSource, 'tokens.js'));
    const { motion: durations, ...rest } = design.scale;
    expect(palettes).toEqual({ dark: design.palettes.dark, light: design.palettes.light });
    expect(scale).toEqual(rest);
    expect(motion.duration).toEqual(durations);
    // The easing curve of the webviews: --ov-ease in extension/media/tokens.css.
    const css = fs.readFileSync(path.join(designSource, '..', 'media', 'tokens.css'), 'utf8');
    expect(css).toContain(
      `--ov-ease: cubic-bezier(${motion.ease.map((n) => String(n).replace(/^0\./, '.')).join(', ')})`,
    );
  });

  test('check passes on a file it has just written', () => {
    expect(run('--source', source, '--out', out).status).toBe(0);
    expect(run('--check', '--source', source, '--out', out).status).toBe(0);
  });

  test('writing twice gives the same bytes', () => {
    run('--source', source, '--out', out);
    const first = fs.readFileSync(out, 'utf8');
    run('--source', source, '--out', out);
    expect(fs.readFileSync(out, 'utf8')).toBe(first);
  });

  test('check fails when the file is missing', () => {
    const result = run('--check', '--source', source, '--out', out);
    expect(result.status).toBe(1);
    expect(result.output).toContain('is missing');
  });

  test('check fails when the file was edited by hand', () => {
    run('--source', source, '--out', out);
    const edited = fs.readFileSync(out, 'utf8').replace(palettes.dark.accent, palettes.dark.red);
    expect(edited).not.toBe(fs.readFileSync(out, 'utf8'));
    fs.writeFileSync(out, edited);

    const result = run('--check', '--source', source, '--out', out);
    expect(result.status).toBe(1);
    expect(result.output).toContain('differs from extension/design');
  });

  test('check fails on a change as small as a space', () => {
    run('--source', source, '--out', out);
    fs.appendFileSync(out, ' ');
    expect(run('--check', '--source', source, '--out', out).status).toBe(1);
  });

  test('check fails when the source has changed since the file was written', () => {
    run('--source', source, '--out', out);
    const tokens = path.join(source, 'tokens.js');
    const changed = fs.readFileSync(tokens, 'utf8').replace('fast: 120', 'fast: 90');
    expect(changed).not.toBe(fs.readFileSync(tokens, 'utf8'));
    fs.writeFileSync(tokens, changed);

    const result = run('--check', '--source', source, '--out', out);
    expect(result.status).toBe(1);

    // Generating again takes the change, and the check passes once more.
    expect(run('--source', source, '--out', out).status).toBe(0);
    expect(fs.readFileSync(out, 'utf8')).toContain('fast: 90');
    expect(run('--check', '--source', source, '--out', out).status).toBe(0);
  });

  test('a source without a palette is an error, not an empty theme', () => {
    const tokens = path.join(source, 'tokens.js');
    fs.writeFileSync(
      tokens,
      fs
        .readFileSync(tokens, 'utf8')
        .replace('module.exports = { scale, palettes }', 'module.exports = { scale }'),
    );
    const result = run('--source', source, '--out', out);
    expect(result.status).toBe(2);
    expect(fs.existsSync(out)).toBe(false);
  });
});
