#!/usr/bin/env node
// Builds the app for the Android emulator, installs it and opens it.
//
//   node scripts/run-android.mjs                debug build, with Metro in the foreground
//   node scripts/run-android.mjs --no-bundler   debug build, Metro started some other way
//   node scripts/run-android.mjs --release      release build with its JavaScript inside,
//                                               signed with the debug keystore
//   OVERSEER_ANDROID_AVD=Other_AVD node scripts/run-android.mjs
//
// Needs ANDROID_HOME, and a JDK 17 in JAVA_HOME and first on the PATH (see README.md).
// It uses Overseer's own virtual device and no other: it starts it when it is not running and
// talks only to the emulator that reports that name.
//
// Why not `expo run:android`: it brings the emulator's window forward with AppleScript, which
// needs the Mac's Automation permission and otherwise waits two minutes before the app opens.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

import {
  METRO_PORT,
  RunError,
  attempt,
  ensureNativeProject,
  followMetro,
  main,
  parseArgs,
  phoneRoot,
  read,
  run,
  runLogged,
  startMetro,
  timed,
} from './lib/run-shared.mjs';

const avd = process.env.OVERSEER_ANDROID_AVD || 'Overseer_API_35';
const sdk = process.env.ANDROID_HOME;
const adb = sdk ? path.join(sdk, 'platform-tools', 'adb') : 'adb';

function checkEnvironment() {
  if (!sdk || !fs.existsSync(path.join(sdk, 'platform-tools'))) {
    throw new RunError('ANDROID_HOME is not set to an Android SDK. See README.md.');
  }
  const java = attempt('java', ['-version']);
  const major = /version "(\d+)/.exec(java.output)?.[1];
  if (!java.ok || major !== '17') {
    throw new RunError(
      `Gradle needs JDK 17 first on the PATH; found ${major ? `JDK ${major}` : 'no java'}. See README.md.`,
    );
  }
}

/** The serial of the running emulator that reports the name of our virtual device. */
function findEmulator() {
  const serials = read(adb, ['devices'])
    .split('\n')
    .map((line) => /^(emulator-\d+)\s+device$/.exec(line.trim())?.[1])
    .filter(Boolean);
  return serials.find((serial) => {
    const name = attempt(adb, ['-s', serial, 'emu', 'avd', 'name']);
    return name.ok && name.output.split('\n')[0].trim() === avd;
  });
}

async function ensureEmulator() {
  let serial = findEmulator();
  if (!serial) {
    const known = read(path.join(sdk, 'emulator', 'emulator'), ['-list-avds']).split('\n');
    if (!known.some((name) => name.trim() === avd)) {
      throw new RunError(
        `the virtual device ${avd} does not exist. See README.md for how it is created.`,
      );
    }
    console.log(`Starting the emulator ${avd}`);
    const emulator = spawn(
      path.join(sdk, 'emulator', 'emulator'),
      // Without a window when asked (the scenario run asks): a windowed emulator waits for the
      // Mac's display, and hangs while it is asleep.
      ['-avd', avd, '-gpu', 'host', '-no-boot-anim', ...(process.env.OVERSEER_EMULATOR_HEADLESS === '1' ? ['-no-window'] : [])],
      {
        detached: true,
        stdio: 'ignore',
      },
    );
    emulator.unref();
  }
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    serial = serial ?? findEmulator();
    if (serial) {
      const booted = attempt(adb, ['-s', serial, 'shell', 'getprop', 'sys.boot_completed']);
      if (booted.ok && booted.output.trim() === '1') return serial;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new RunError(`the emulator ${avd} did not finish starting within three minutes`);
}

main(async () => {
  const options = parseArgs(process.argv.slice(2));
  const variant = options.release ? 'release' : 'debug';
  checkEnvironment();
  const serial = await ensureEmulator();
  const abi = read(adb, ['-s', serial, 'shell', 'getprop', 'ro.product.cpu.abi']).trim();
  console.log(`Emulator: ${avd} (${serial}, ${abi})`);

  ensureNativeProject('android');

  const task = options.release ? ':app:assembleRelease' : ':app:assembleDebug';
  await timed(`gradle ${task}`, () =>
    runLogged(
      './gradlew',
      [task, `-PreactNativeArchitectures=${abi}`, '--console=plain'],
      path.join(phoneRoot, 'android', 'build', `gradle-${variant}.log`),
      { cwd: path.join(phoneRoot, 'android') },
    ),
  );

  const apk = path.join(
    phoneRoot,
    'android',
    'app',
    'build',
    'outputs',
    'apk',
    variant,
    `app-${variant}.apk`,
  );
  const aapt = fs
    .readdirSync(path.join(sdk, 'build-tools'))
    .sort()
    .map((version) => path.join(sdk, 'build-tools', version, 'aapt2'))
    .findLast((tool) => fs.existsSync(tool));
  if (!aapt) throw new RunError('no aapt2 in the build tools of ANDROID_HOME');
  const badging = read(aapt, ['dump', 'badging', apk]);
  const appId = /package: name='([^']+)'/.exec(badging)?.[1];
  const activity = /launchable-activity: name='([^']+)'/.exec(badging)?.[1];
  if (!appId || !activity) throw new RunError(`could not read the package and activity of ${apk}`);

  attempt(adb, ['-s', serial, 'shell', 'am', 'force-stop', appId]);
  run(adb, ['-s', serial, 'install', '-r', apk]);
  let metro = null;
  if (options.bundler) {
    metro = await startMetro();
    // The debug app asks for Metro on its own localhost; this sends that to the Mac.
    run(adb, ['-s', serial, 'reverse', `tcp:${METRO_PORT}`, `tcp:${METRO_PORT}`]);
  }
  run(adb, ['-s', serial, 'shell', 'am', 'start', '-n', `${appId}/${activity}`], {
    stdio: 'ignore',
  });
  console.log(`${appId} (${variant}) is open on ${avd}`);
  await followMetro(metro);
});
