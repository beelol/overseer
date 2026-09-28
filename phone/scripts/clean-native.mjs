#!/usr/bin/env node
// Removes what native builds leave behind inside node_modules.
//
// Gradle builds each library where it lies, so its C++ and Kotlin output is written to
// node_modules/<library>/android/.cxx and node_modules/<library>/android/build. That output
// names other libraries' files by paths that hold a hash of their version. After a native
// dependency changes version the old output points at files that no longer exist, and the
// build fails with "ninja: error: ... missing and no known rule to make it".
// `npm run prebuild` runs this first, so a regenerated project always builds from clean.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const modules = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'node_modules');

function packages() {
  if (!fs.existsSync(modules)) return [];
  return fs.readdirSync(modules, { withFileTypes: true }).flatMap((entry) => {
    if (!entry.isDirectory() || entry.name.startsWith('.')) return [];
    const directory = path.join(modules, entry.name);
    if (!entry.name.startsWith('@')) return [directory];
    return fs.readdirSync(directory).map((name) => path.join(directory, name));
  });
}

let removed = 0;
for (const directory of packages()) {
  // Only libraries with an Android project of their own have build output to remove.
  const android = path.join(directory, 'android');
  if (
    !fs.existsSync(path.join(android, 'build.gradle')) &&
    !fs.existsSync(path.join(android, 'build.gradle.kts'))
  ) {
    continue;
  }
  for (const output of ['.cxx', 'build']) {
    const target = path.join(android, output);
    if (fs.existsSync(target)) {
      fs.rmSync(target, { recursive: true, force: true });
      removed += 1;
    }
  }
}
console.log(`clean-native: removed ${removed} build directories from node_modules`);
