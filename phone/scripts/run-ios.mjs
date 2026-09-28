#!/usr/bin/env node
// Builds the app for the iOS simulator, installs it and opens it.
//
//   node scripts/run-ios.mjs                debug build, with Metro in the foreground
//   node scripts/run-ios.mjs --no-bundler   debug build, Metro started some other way
//   node scripts/run-ios.mjs --release      Release build with its JavaScript inside
//   OVERSEER_IOS_SIMULATOR="iPhone 17" node scripts/run-ios.mjs
//
// Why not `expo run:ios`: it takes the first simulator with the given name, including one
// whose runtime is no longer installed, and it looks for the Simulator's window with
// AppleScript, which needs the Mac's Automation permission and otherwise fails after two
// minutes with the app installed but never opened. This script uses xcodebuild and simctl only.

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

import {
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

const simulatorName = process.env.OVERSEER_IOS_SIMULATOR || 'iPhone 17 Pro';

function runtimeVersion(runtime) {
  // com.apple.CoreSimulator.SimRuntime.iOS-26-5 -> [26, 5]
  const match = /iOS-(\d+)-(\d+)/.exec(runtime);
  return match ? [Number(match[1]), Number(match[2])] : [0, 0];
}

/** A simulator of that name that can boot: a booted one first, then the newest runtime. */
function findSimulator() {
  const listed = JSON.parse(read('xcrun', ['simctl', 'list', 'devices', 'available', '--json']));
  const found = Object.entries(listed.devices)
    .flatMap(([runtime, devices]) => devices.map((device) => ({ ...device, runtime })))
    .filter(
      (device) =>
        device.name === simulatorName && device.isAvailable && device.runtime.includes('iOS'),
    );
  found.sort((a, b) => {
    const booted = Number(b.state === 'Booted') - Number(a.state === 'Booted');
    if (booted !== 0) return booted;
    const [aMajor, aMinor] = runtimeVersion(a.runtime);
    const [bMajor, bMinor] = runtimeVersion(b.runtime);
    return bMajor - aMajor || bMinor - aMinor;
  });
  if (found.length === 0) {
    throw new RunError(
      `no available iOS simulator is named "${simulatorName}". See: xcrun simctl list devices available`,
    );
  }
  return found[0];
}

function boot(simulator) {
  if (simulator.state !== 'Booted') attempt('xcrun', ['simctl', 'boot', simulator.udid]);
  run('xcrun', ['simctl', 'bootstatus', simulator.udid, '-b'], { stdio: 'ignore' });
  // Shows the window. The build and the app do not depend on it.
  attempt('open', ['-a', 'Simulator', '--args', '-CurrentDeviceUDID', simulator.udid]);
}

function findWorkspace() {
  const ios = path.join(phoneRoot, 'ios');
  const workspace = fs.readdirSync(ios).find((name) => name.endsWith('.xcworkspace'));
  if (!workspace)
    throw new RunError('ios/ has no .xcworkspace. Generate it again: npm run prebuild');
  return { workspace: path.join(ios, workspace), scheme: path.basename(workspace, '.xcworkspace') };
}

main(async () => {
  const options = parseArgs(process.argv.slice(2));
  const configuration = options.release ? 'Release' : 'Debug';
  const simulator = findSimulator();
  const [major, minor] = runtimeVersion(simulator.runtime);
  console.log(`Simulator: ${simulator.name}, iOS ${major}.${minor} (${simulator.udid})`);

  ensureNativeProject('ios');
  boot(simulator);

  const { workspace, scheme } = findWorkspace();
  const derived = path.join(phoneRoot, 'ios', 'build');
  await timed(`xcodebuild ${configuration}`, () =>
    runLogged(
      'xcodebuild',
      [
        ...['-workspace', workspace],
        ...['-scheme', scheme],
        ...['-configuration', configuration],
        ...['-sdk', 'iphonesimulator'],
        ...['-destination', `id=${simulator.udid}`],
        ...['-derivedDataPath', derived],
        'build',
      ],
      path.join(derived, `xcodebuild-${configuration}.log`),
    ),
  );

  const app = path.join(
    derived,
    'Build',
    'Products',
    `${configuration}-iphonesimulator`,
    `${scheme}.app`,
  );
  const bundleId = read('plutil', [
    '-extract',
    'CFBundleIdentifier',
    'raw',
    path.join(app, 'Info.plist'),
  ]).trim();

  attempt('xcrun', ['simctl', 'terminate', simulator.udid, bundleId]);
  run('xcrun', ['simctl', 'install', simulator.udid, app]);
  const metro = options.bundler ? await startMetro() : null;
  run('xcrun', ['simctl', 'launch', simulator.udid, bundleId]);
  console.log(`${bundleId} (${configuration}) is open on ${simulator.name}`);
  await followMetro(metro);
});
