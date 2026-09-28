#!/usr/bin/env node
// Proves the token rule (AC-131, AC-137) instead of trusting it: a colour, a size, a radius, a
// font weight or a duration written by hand in a screen fails the lint.
//
//   1. writes temporary files with seeded violations in src/,
//   2. runs the lint and asserts that it FAILS, with every violation reported by the rule,
//   3. removes the files,
//   4. runs the lint again and asserts that it PASSES.
//
// It also prints the motion tokens, the whole list every transition is built from.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const phoneRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const PROBE = path.join(phoneRoot, 'src', '__token_rule_probe__');

const violations = [
  { file: 'duration.ts', code: 'export const opening = { duration: 600 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'delay.ts', code: 'export const waiting = { delay: 120 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'colour.ts', code: "export const accent = '#A48BFF';\n", says: 'A colour written by hand' },
  { file: 'colour-function.ts', code: "export const shade = 'rgba(0, 0, 0, 0.4)';\n", says: 'A colour written by hand' },
  { file: 'named-colour.ts', code: "export const style = { backgroundColor: 'red' };\n", says: 'A colour written by hand' },
  { file: 'font-size.ts', code: 'export const style = { fontSize: 15 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'space.ts', code: 'export const style = { paddingHorizontal: 18 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'negative-space.ts', code: 'export const style = { marginTop: -4 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'radius.ts', code: 'export const style = { borderRadius: 10 };\n', says: 'A size, space, radius or duration written by hand' },
  { file: 'weight.ts', code: "export const style = { fontWeight: '700' };\n", says: 'A font weight written by hand' },
];

function lint() {
  const result = spawnSync('npm', ['run', 'lint', '--silent', '--', '--format', 'json'], { cwd: phoneRoot, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.error) throw result.error;
  try {
    return { status: result.status, files: JSON.parse(result.stdout) };
  } catch {
    throw new Error(`the lint did not report in JSON:\n${result.stdout}\n${result.stderr}`);
  }
}

const remove = () => fs.rmSync(PROBE, { recursive: true, force: true });

function flat(value, prefix, out) {
  for (const [key, item] of Object.entries(value)) {
    const name = prefix ? `${prefix}.${key}` : key;
    if (item !== null && typeof item === 'object' && !Array.isArray(item)) flat(item, name, out);
    else out.push([name, Array.isArray(item) ? `[${item.join(', ')}]` : String(item)]);
  }
  return out;
}

async function motionTokens() {
  const source = fs.readFileSync(path.join(phoneRoot, 'src', 'theme', 'tokens.generated.ts'), 'utf8');
  const read = (name) => {
    const match = new RegExp(`export const ${name} = (\\{[\\s\\S]*?\\n\\}) as const;`).exec(source);
    if (!match) throw new Error(`tokens.generated.ts has no ${name}`);
    return Function(`return (${match[1]})`)();
  };
  return [...flat(read('motion'), 'motion', []), ...flat(read('phone').motion, 'phone.motion', [])];
}

async function main() {
  if (fs.existsSync(PROBE)) throw new Error('a probe directory from an earlier run exists; remove it and run again');
  const problems = [];
  try {
    fs.mkdirSync(PROBE);
    for (const { file, code } of violations) fs.writeFileSync(path.join(PROBE, file), code);
    const seeded = lint();
    if (seeded.status === 0) problems.push('the lint passed with the violations in place');
    for (const { file, says } of violations) {
      const entry = seeded.files.find((candidate) => path.resolve(candidate.filePath) === path.join(PROBE, file));
      const found = (entry?.messages ?? []).some((m) => m.ruleId === 'no-restricted-syntax' && m.severity === 2 && m.message.includes(says));
      console.log(`  ${found ? 'caught ' : 'MISSED '} src/__token_rule_probe__/${file}  (${says})`);
      if (!found) problems.push(`${file} was not reported`);
    }
  } finally {
    remove();
  }
  const clean = lint();
  const left = clean.files.filter((entry) => entry.errorCount + entry.warningCount > 0);
  if (clean.status !== 0 || left.length > 0) problems.push(`the lint does not pass without the violations (${left.length} files with findings)`);
  console.log(`  without the violations the lint ${clean.status === 0 ? 'passes' : 'FAILS'}`);

  const tokens = await motionTokens();
  console.log(`\nThe motion tokens (${tokens.length}), in milliseconds unless the name says otherwise:`);
  for (const [name, value] of tokens) console.log(`  ${name.padEnd(36)} ${value}`);

  if (problems.length > 0) {
    console.error(`token rule: FAILED. ${problems.join('; ')}`);
    process.exitCode = 1;
  } else console.log(`\ntoken rule: ok. ${violations.length} seeded violations failed the lint; the clean tree passes.`);
}

process.on('SIGINT', () => {
  remove();
  process.exit(130);
});

main().catch((error) => {
  remove();
  console.error(`token rule: FAILED. ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});
