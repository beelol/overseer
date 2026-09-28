#!/usr/bin/env node
// Proves the platform rule (AC-134) instead of trusting it:
//
//   1. writes temporary files with seeded violations outside src/platform/,
//   2. runs the lint and asserts that it FAILS, with every violation reported by the rule,
//   3. removes the files,
//   4. runs the lint again and asserts that it PASSES.
//
// The same code inside src/platform/ is seeded too and must NOT be reported by the rule:
// the platform layer is the one place allowed to ask. And it is seeded in a package of its own
// beside the app, as core/ and model/ are, where it must be reported as well.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const phoneRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUTSIDE = path.join(phoneRoot, 'src', '__platform_rule_probe__');
const INSIDE = path.join(phoneRoot, 'src', 'platform', '__platform_rule_probe__');
const PACKAGE = path.join(phoneRoot, '__platform_rule_probe_package__', 'src');
const RULES = ['no-restricted-imports', 'no-restricted-syntax'];

/** Each seeded violation: the file, its code, and what the lint must say about it. */
const violations = [
  {
    file: 'import-platform.ts',
    code: "import { Platform } from 'react-native';\n\nexport const platform = Platform;\n",
    rule: 'no-restricted-imports',
    says: "'Platform' import from 'react-native' is restricted",
  },
  {
    file: 'platform-os.ts',
    code: "declare const Platform: { OS: string };\n\nexport const isPhone = Platform.OS === 'ios';\n",
    rule: 'no-restricted-syntax',
    says: 'Platform.OS',
  },
  {
    file: 'platform-select.ts',
    code: 'declare const Platform: { select<T>(choices: { ios: T; android: T }): T };\n\nexport const gap = Platform.select({ ios: 1, android: 2 });\n',
    rule: 'no-restricted-syntax',
    says: 'Platform.select',
  },
  {
    file: 'import-ios-file.ts',
    code: "import { createLaunch } from '../platform/native/launch.ios';\n\nexport const launch = createLaunch;\n",
    rule: 'no-restricted-imports',
    says: "A platform's file is chosen by the bundler",
  },
  {
    file: 'import-android-file.ts',
    code: "import { createLaunch } from '../platform/native/launch.android';\n\nexport const launch = createLaunch;\n",
    rule: 'no-restricted-imports',
    says: "A platform's file is chosen by the bundler",
  },
  {
    file: 'compare-platform-name.ts',
    code: "export function isApple(platform: string): boolean {\n  return platform === 'ios';\n}\n",
    rule: 'no-restricted-syntax',
    says: "A comparison with a platform's name",
  },
];

function lint() {
  const result = spawnSync('npm', ['run', 'lint', '--silent', '--', '--format', 'json'], {
    cwd: phoneRoot,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  let files;
  try {
    files = JSON.parse(result.stdout);
  } catch {
    throw new Error(`the lint did not report in JSON:\n${result.stdout}\n${result.stderr}`);
  }
  return { status: result.status, files };
}

function messagesFor(files, file) {
  const entry = files.find((candidate) => path.resolve(candidate.filePath) === path.resolve(file));
  return entry ? entry.messages : [];
}

function remove() {
  fs.rmSync(OUTSIDE, { recursive: true, force: true });
  fs.rmSync(INSIDE, { recursive: true, force: true });
  fs.rmSync(path.dirname(PACKAGE), { recursive: true, force: true });
}

function fail(message) {
  console.error(`platform rule: FAILED. ${message}`);
  process.exitCode = 1;
}

function main() {
  if ([OUTSIDE, INSIDE, path.dirname(PACKAGE)].some((probe) => fs.existsSync(probe))) {
    throw new Error('a probe directory from an earlier run exists; remove it and run again');
  }
  const problems = [];
  try {
    fs.mkdirSync(OUTSIDE);
    fs.mkdirSync(INSIDE);
    fs.mkdirSync(PACKAGE, { recursive: true });
    for (const { file, code } of violations) {
      fs.writeFileSync(path.join(OUTSIDE, file), code);
      fs.writeFileSync(
        path.join(PACKAGE, file),
        code.replaceAll('../platform/native/', '../../src/platform/native/'),
      );
      // The same code, one directory deeper, so its relative imports still point at the layer.
      fs.writeFileSync(
        path.join(INSIDE, file),
        code.replaceAll('../platform/native/', '../native/'),
      );
    }

    const seeded = lint();
    if (seeded.status === 0) problems.push('the lint passed with the violations in place');
    for (const { file, rule, says } of violations) {
      const found = messagesFor(seeded.files, path.join(OUTSIDE, file)).some(
        (message) =>
          message.ruleId === rule && message.severity === 2 && message.message.includes(says),
      );
      console.log(
        `  ${found ? 'caught ' : 'MISSED '} src/__platform_rule_probe__/${file}  (${rule}: ${says})`,
      );
      if (!found) problems.push(`${file} was not reported by ${rule}`);

      const inPackage = messagesFor(seeded.files, path.join(PACKAGE, file)).some(
        (message) =>
          message.ruleId === rule && message.severity === 2 && message.message.includes(says),
      );
      if (!inPackage) problems.push(`${file} was not reported in a package beside the app`);

      const inside = messagesFor(seeded.files, path.join(INSIDE, file)).filter(
        (message) => RULES.includes(message.ruleId) && /platform|Platform/.test(message.message),
      );
      if (inside.length > 0)
        problems.push(`${file} was reported inside src/platform/, where it is allowed`);
    }
    console.log(
      `  the same ${violations.length} files in a package beside the app were ${problems.some((p) => p.includes('beside')) ? 'MISSED' : 'caught'}`,
    );
    console.log(
      `  the same ${violations.length} files inside src/platform/ were ${problems.some((p) => p.includes('inside')) ? 'REPORTED' : 'allowed'}`,
    );
  } finally {
    remove();
  }

  const clean = lint();
  const left = clean.files.filter((entry) => entry.errorCount + entry.warningCount > 0);
  if (clean.status !== 0 || left.length > 0) {
    problems.push(
      `the lint does not pass without the violations (${left.length} files with findings)`,
    );
  }
  console.log(`  without the violations the lint ${clean.status === 0 ? 'passes' : 'FAILS'}`);

  if (problems.length > 0) fail(problems.join('; '));
  else
    console.log(
      `platform rule: ok. ${violations.length} seeded violations failed the lint; the clean tree passes.`,
    );
}

process.on('SIGINT', () => {
  remove();
  process.exit(130);
});

try {
  main();
} catch (error) {
  remove();
  fail(error instanceof Error ? error.message : String(error));
}
