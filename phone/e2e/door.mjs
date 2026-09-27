#!/usr/bin/env node
// Records the door (AC-136) on a simulator: a cold start in dark and in light, a cold start with
// Reduce Motion, and a return from the background (no door). Each recording is kept with a
// contact sheet of its frames, so the opening can be read frame by frame.
//
//   node e2e/door.mjs --platform ios|android [--out <dir>]
//   node e2e/door.mjs --platform ios|android --sheets    the sheets again, from the recordings kept
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

/** The recording starts this long before the app is launched. */
const LEAD_S = 1.5;

/** Frames at 30 a second from the launch, as the sheets and the opening frame read them. */
const FROM_LAUNCH = (file) => ['-ss', String(LEAD_S), '-i', file];

/**
 * The index, in frames at 30 a second from the launch, of the door mid-opening (mid-fade with
 * Reduce Motion), or null when the recorder kept no frame of it. The recorder repeats a frame
 * until the screen changes: the closed door is the longest still stretch, the list the longest
 * still stretch after it, and the opening the frames between them. The pick differs most from both.
 */
function midOpening(file) {
  const W = 40;
  const H = 87;
  const raw = execFileSync('ffmpeg', ['-v', 'fatal', ...FROM_LAUNCH(file), '-vf', `fps=30,scale=${W}:${H},format=gray`, '-f', 'rawvideo', '-'], { maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'ignore'] });
  const size = W * H;
  const frames = [];
  for (let i = 0; i + size <= raw.length; i += size) frames.push(raw.subarray(i, i + size));
  const differ = (a, b) => {
    let d = 0;
    for (let i = 0; i < a.length; i += 1) d += Math.abs(a[i] - b[i]);
    return d / a.length;
  };
  const stretches = [];
  for (let i = 0; i < frames.length; i += 1) {
    const current = stretches[stretches.length - 1];
    if (current && differ(frames[i], frames[current.start]) < 0.5) current.end = i;
    else stretches.push({ start: i, end: i });
  }
  const longest = (list) => [...list].sort((a, b) => b.end - b.start - (a.end - a.start))[0];
  const door = longest(stretches);
  const list = door && longest(stretches.filter((t) => t.start > door.end));
  if (!door || !list || list.start - door.end < 2) return null;
  let best = { index: null, score: 0 };
  for (let i = door.end + 1; i < list.start; i += 1) {
    const score = Math.min(differ(frames[i], frames[door.start]), differ(frames[i], frames[list.start]));
    if (score > best.score) best = { index: i, score };
  }
  return best.index;
}

/**
 * A recording read for people: every frame at 30 a second from the launch to the end, as one
 * sheet, and the frame of the opening with the most colour at full width. A recorder under load
 * keeps fewer frames than the screen drew; the app's own count is in the timings.
 */
function sheets(name) {
  const file = path.join(out, `${platform}-${name}.mp4`);
  if (!fs.existsSync(file)) return;
  try {
    execFileSync('ffmpeg', ['-v', 'fatal', '-y', ...FROM_LAUNCH(file), '-vf', 'fps=30,scale=150:-1,tile=12x10', '-frames:v', '1', path.join(out, `${platform}-${name}-frames.png`)]);
    const opening = path.join(out, `${platform}-${name}-opening.png`);
    fs.rmSync(opening, { force: true });
    const index = name === 'return-from-background' ? null : midOpening(file);
    if (index !== null) execFileSync('ffmpeg', ['-v', 'fatal', '-y', ...FROM_LAUNCH(file), '-vf', `fps=30,select=eq(n\\,${index}),scale=600:-1`, '-frames:v', '1', opening]);
  } catch {
    /* ffmpeg is a help for reading, not a need */
  }
}

const NAMES = ['cold-start-dark', 'cold-start-light', 'return-from-background', 'cold-start-reduce-motion'];

async function record(name, act) {
  const file = path.join(out, `${platform}-${name}.mp4`);
  const before = stored(dev, 'perf.last')?.launch ?? null;
  const stop = dev.record(file);
  await sleep(LEAD_S * 1000);
  await act();
  await sleep(4000);
  await stop();
  sheets(name);
  const fresh = stored(dev, 'perf.last');
  // Back from the background the app is not launched again: there is no new record of a launch.
  const record = fresh && fresh.launch !== before ? fresh : null;
  const lines = [`${platform} ${name}: ${path.basename(file)}${record ? '' : ' (no new launch: the app was not started again)'}`, `  marks: ${JSON.stringify(record?.marks ?? {})}`, `  door: ${JSON.stringify({ opening: record?.summary?.['door.opening'], frames: record?.summary?.['door.frames'], dropped: record?.summary?.['door.dropped'], longestFrame: record?.summary?.['door.longestFrame'] })}`];
  fs.appendFileSync(path.join(out, `${platform}-timings.txt`), `${lines.join('\n')}\n`);
  console.log(lines.join('\n'));
}

async function main() {
  if (args.includes('--sheets')) {
    for (const name of NAMES) sheets(name);
    return;
  }
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
