#!/usr/bin/env node
// Records the door (AC-136) on a simulator: a cold start in dark and in light, a cold start with
// Reduce Motion, and a return from the background (no door). Each recording is kept with a
// contact sheet of its frames, so the opening can be read frame by frame.
//
//   node e2e/door.mjs --platform ios|android [--out <dir>]
//
// The app must be installed (a release build) and paired, so it opens on the agents list.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { device, sleep, stored } from './device.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const platform = args[args.indexOf('--platform') + 1] || 'ios';
const out = args.includes('--out') ? path.resolve(args[args.indexOf('--out') + 1]) : path.join(here, '..', '..', 'docs', 'verification', 'evidence', 'phone', 'door');
fs.mkdirSync(out, { recursive: true });

const dev = device(platform);
const udid = dev.id;
const ADB = '/opt/homebrew/share/android-commandlinetools/platform-tools/adb';
const shell = (command) => execFileSync(ADB, ['-s', udid, 'shell', command], { encoding: 'utf8' }).trim();

function reduceMotion(on) {
  if (platform === 'ios') {
    execFileSync('xcrun', ['simctl', 'spawn', udid, 'defaults', 'write', 'com.apple.Accessibility', 'ReduceMotionEnabled', '-bool', on ? 'true' : 'false']);
  } else {
    shell(`settings put global animator_duration_scale ${on ? '0' : '1'}`);
    shell(`settings put global transition_animation_scale ${on ? '0' : '1'}`);
    shell(`settings put global window_animation_scale ${on ? '0' : '1'}`);
  }
}

async function record(name, act) {
  const file = path.join(out, `${platform}-${name}.mp4`);
  const stop = dev.record(file);
  await sleep(1500);
  await act();
  await sleep(4000);
  await stop();
  // Every frame at 30 a second from the moment the app appears, as one sheet, and the timings.
  const sheet = path.join(out, `${platform}-${name}-frames.png`);
  try {
    execFileSync('ffmpeg', ['-v', 'error', '-y', '-i', file, '-vf', 'fps=30,scale=180:-1,tile=10x9', '-frames:v', '1', sheet]);
  } catch {
    /* ffmpeg is a help for reading, not a need */
  }
  const record = stored(dev, 'perf.last');
  const lines = [`${platform} ${name}: ${path.basename(file)}`, `  marks: ${JSON.stringify(record?.marks ?? {})}`, `  door: ${JSON.stringify({ opening: record?.summary?.['door.opening'], frames: record?.summary?.['door.frames'], dropped: record?.summary?.['door.dropped'], longestFrame: record?.summary?.['door.longestFrame'] })}`];
  fs.appendFileSync(path.join(out, `${platform}-timings.txt`), `${lines.join('\n')}\n`);
  console.log(lines.join('\n'));
}

async function main() {
  fs.rmSync(path.join(out, `${platform}-timings.txt`), { force: true });
  dev.stop();
  dev.write('test.door', 'on');
  reduceMotion(false);
  for (const theme of ['dark', 'light']) {
    dev.appearance(theme);
    await sleep(1000);
    dev.stop();
    await sleep(500);
    await record(`cold-start-${theme}`, async () => dev.launch());
  }
  // Back from the background: no door.
  if (platform === 'ios') execFileSync('xcrun', ['simctl', 'launch', udid, 'com.apple.Preferences'], { stdio: 'ignore' });
  else shell('input keyevent KEYCODE_HOME');
  await sleep(3000);
  await record('return-from-background', async () => dev.launch());
  // Reduce Motion: the door fades instead.
  reduceMotion(true);
  dev.stop();
  await sleep(500);
  await record('cold-start-reduce-motion', async () => dev.launch());
  reduceMotion(false);
  dev.appearance('dark');
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
