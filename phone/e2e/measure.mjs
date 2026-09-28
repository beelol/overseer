#!/usr/bin/env node
// Measures the app's speed on a simulator (AC-135, AC-136): cold starts with the door and
// without it, and the busy-logic test. The app measures itself (src/perf) and leaves each
// launch's record in its storage; this script starts the app, reads the record and sums up.
//
//   node e2e/measure.mjs --platform ios [--runs 20] [--out <dir>] [--check] [--write-baseline]
//
// The app must be installed as a release build and paired, so it opens on the agents list from
// what it has stored. `--check` compares with e2e/baselines.json and fails when a budget is
// missed; `--write-baseline` records this run as the baseline of its platform.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { device, sleep, stored } from './device.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const BASELINES = path.join(here, 'baselines.json');
/** How far a later run may be above its baseline. */
const ALLOWED = 1.1;
/**
 * One frame at 60 Hz, in milliseconds. The door and the first screen mark themselves shown in
 * the same commit, in tree order, so the door's mark may follow the screen's by a fraction of a
 * millisecond; a door that came a whole frame later was seen later.
 */
const FRAME = 17;
/** The door's opening, from the design tokens the app is built with (AC-136: within 60 ms of it). */
const DOOR_MS = JSON.parse(fs.readFileSync(path.join(here, '..', 'design', 'phone-tokens.json'), 'utf8')).motion.door.open;
/** Budgets that hold whatever the baseline says, in milliseconds (the RFC's numbers). */
const LIMITS = {
  ios: { 'agents.interactive.p95': 1000, 'door.opening.low': DOOR_MS - 60, 'door.opening.high': DOOR_MS + 60 },
  android: { 'agents.interactive.p95': 2000, 'door.opening.low': DOOR_MS - 60, 'door.opening.high': DOOR_MS + 60 },
};

function parse(argv) {
  const args = { platform: 'ios', runs: 20, out: null, check: false, writeBaseline: false, settle: 3500 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--check') args.check = true;
    else if (arg === '--write-baseline') args.writeBaseline = true;
    else if (['--platform', '--runs', '--out', '--settle'].includes(arg)) {
      args[arg.slice(2)] = ['--runs', '--settle'].includes(arg) ? Number(argv[i + 1]) : argv[i + 1];
      i += 1;
    } else throw new Error(`unknown argument: ${arg}`);
  }
  return args;
}

const round = (n) => Math.round(n * 10) / 10;

export function summary(values) {
  const sorted = values.filter((v) => typeof v === 'number' && Number.isFinite(v)).sort((a, b) => a - b);
  if (sorted.length === 0) return { count: 0, p50: null, p95: null, min: null, max: null };
  const at = (fraction) => sorted[Math.min(sorted.length, Math.max(1, Math.ceil(fraction * sorted.length))) - 1];
  return { count: sorted.length, p50: round(at(0.5)), p95: round(at(0.95)), min: round(sorted[0]), max: round(sorted[sorted.length - 1]) };
}

/** One cold start: the app is stopped, started, given time to open, and its record is read. */
async function coldStart(dev, settle, before) {
  dev.stop();
  await sleep(400);
  dev.launch();
  const end = Date.now() + settle + 6000;
  await sleep(settle);
  for (;;) {
    const record = stored(dev, 'perf.last');
    if (record && record.launch !== before && (record.marks['door.opened'] !== undefined || record.marks['door.shown'] === undefined) && record.marks['screen.agents.interactive'] !== undefined) return record;
    if (Date.now() > end) return record && record.launch !== before ? record : null;
    await sleep(250);
  }
}

/**
 * `runs` cold starts with the door and `runs` without it, taken in turn (with, without, with, ...)
 * so that whatever else the Mac does in the meantime weighs on both alike. One launch before
 * them is not counted: the first start after the scenarios pays for what later starts find ready.
 */
async function series(dev, runs, settle, log) {
  const kinds = { on: [], off: [] };
  let last = stored(dev, 'perf.last')?.launch ?? -1;
  const once = async (door, label) => {
    dev.stop();
    await sleep(300);
    dev.write('test.door', door);
    const record = await coldStart(dev, settle, last);
    if (!record) {
      log(`  ${label}: no record (the app did not reach its first screen)`);
      return null;
    }
    last = record.launch;
    const m = record.marks;
    log(`  ${label}: first screen ${round(m['screen.agents.shown'] ?? NaN)} ms, interactive ${round(m['screen.agents.interactive'] ?? NaN)} ms, door shown ${round(m['door.shown'] ?? NaN)} ms, opening ${round(record.summary['door.opening']?.max ?? NaN)} ms, dropped ${record.summary['door.dropped']?.max ?? 'n/a'}`);
    if (m['door.opening'] !== undefined) log(`    ${timeline(record)}`);
    return record;
  };
  await once('on', 'warm-up launch (not counted)');
  for (let i = 0; i < runs; i += 1) {
    kinds.on.push(await once('on', `launch ${i + 1} with the door`));
    kinds.off.push(await once('off', `launch ${i + 1} without the door`));
  }
  dev.stop();
  await sleep(300);
  dev.write('test.door', 'on');
  return kinds;
}

/**
 * One opening beside what else happened then, in milliseconds from its start: the Mac's first
 * answer, its state replacing what was stored, the list's first layout, and each late frame
 * (when, and how long the UI thread held it).
 */
export function timeline(record) {
  const m = record.marks;
  const start = m['door.opening'];
  const at = (name) => (m[name] === undefined ? 'not yet' : `${round(m[name] - start)}`);
  const stalls = [];
  for (let i = 1; record.summary[`door.stall.${i}.at`] !== undefined; i += 1) stalls.push(`${round(record.summary[`door.stall.${i}.at`].max)} (${round(record.summary[`door.stall.${i}.ms`]?.max ?? NaN)} ms)`);
  return `from the opening's start at ${round(start)} ms: Mac online ${at('session.online')}, Mac's state ${at('session.state')}, list laid out ${at('screen.agents.list')}; late frames at ${stalls.length > 0 ? stalls.join(', ') : 'none'}`;
}

function figures(records) {
  const ok = records.filter(Boolean);
  const mark = (name) => summary(ok.map((r) => r.marks[name]));
  const measure = (name) => summary(ok.map((r) => r.summary[name]?.max));
  return {
    launches: records.length,
    reached: ok.length,
    // A launch that drew the pairing screen asked the owner for something.
    pairing: ok.filter((r) => r.marks['screen.pair.shown'] !== undefined).length,
    'javascript.loaded': summary(ok.map((r) => r.startup['javascript.loaded'])),
    'door.shown': mark('door.shown'),
    'agents.shown': mark('screen.agents.shown'),
    // Per launch: how long after the first screen's mark the door's came; negative when the door came first.
    'door.late': summary(ok.map((r) => (r.marks['door.shown'] !== undefined && r.marks['screen.agents.shown'] !== undefined ? r.marks['door.shown'] - r.marks['screen.agents.shown'] : undefined))),
    'agents.interactive': mark('screen.agents.interactive'),
    'door.opening': measure('door.opening'),
    'door.frames': measure('door.frames'),
    'door.dropped': measure('door.dropped'),
    'door.longestFrame': measure('door.longestFrame'),
    // Every launch's opening together: the frames drawn and the frames dropped.
    'door.all': ok.reduce((all, r) => ({ frames: all.frames + (r.summary['door.frames']?.max ?? 0), dropped: all.dropped + (r.summary['door.dropped']?.max ?? 0) }), { frames: 0, dropped: 0 }),
    'seeded.slow': measure('seeded.slow'),
  };
}

export function verdicts(platform, withDoor, withoutDoor, baseline) {
  const limits = LIMITS[platform];
  const out = [];
  const say = (name, value, limit, ok, note = '') => out.push({ name, value, limit, ok, note });
  const p95 = withDoor['agents.interactive'].p95;
  say('every launch reaches the agents list', withDoor.reached, withDoor.launches, withDoor.reached === withDoor.launches);
  const asked = withDoor.pairing + (withoutDoor ? withoutDoor.pairing : 0);
  const launches = withDoor.launches + (withoutDoor ? withoutDoor.launches : 0);
  say(`launches that showed pairing or any other question, of ${launches}`, asked, 0, asked === 0);
  say('agents list interactive, p95 (ms)', p95, limits['agents.interactive.p95'], p95 !== null && p95 <= limits['agents.interactive.p95']);
  if (baseline) {
    const allowed = round(baseline['agents.interactive.p95'] * ALLOWED);
    say('agents list interactive against the baseline, p95 (ms)', p95, allowed, p95 !== null && p95 <= allowed, `baseline ${baseline['agents.interactive.p95']} ms + 10%`);
  }
  const late = withDoor['door.late'].max;
  say('the door is on screen with the first screen or before it, worst launch (ms after the first screen)', late, FRAME, late !== null && late <= FRAME, `${withDoor['door.late'].count} of ${withDoor.launches} launches marked both`);
  const opening = withDoor['door.opening'];
  say(`the door opens in ${DOOR_MS} ms within 60 ms, shortest (ms)`, opening.min, limits['door.opening.low'], opening.min !== null && opening.min >= limits['door.opening.low']);
  say(`the door opens in ${DOOR_MS} ms within 60 ms, longest (ms)`, opening.max, limits['door.opening.high'], opening.max !== null && opening.max <= limits['door.opening.high']);
  const frames = withDoor['door.frames'].min ?? 0;
  const dropped = withDoor['door.dropped'].max ?? 0;
  const share = frames + dropped === 0 ? 100 : round((dropped / (frames + dropped)) * 100);
  if (platform === 'ios') {
    say('frames dropped while the door opens, worst launch (%)', share, 1, share <= 1, `${dropped} of ${frames + dropped} frames`);
  } else {
    // AC-135: the 1% display budget is measured on the owner's iPhone; the emulator records its own
    // baseline for each budget and a later run stays within 10% of it. Its drops are still shown.
    const all = withDoor['door.all'];
    const allShare = all.frames + all.dropped === 0 ? 100 : round((all.dropped / (all.frames + all.dropped)) * 100);
    const worst = `worst launch ${share}% (${dropped} of ${frames + dropped} frames)`;
    if (baseline && typeof baseline['door.dropped.share'] === 'number') {
      const allowed = round(baseline['door.dropped.share'] * ALLOWED);
      say('frames dropped while the door opens, all launches, against the baseline (%)', allShare, allowed, allShare <= allowed, `${all.dropped} of ${all.frames + all.dropped} frames; baseline ${baseline['door.dropped.share']}% + 10%; ${worst}`);
    } else {
      say("frames dropped while the door opens, all launches (%): the emulator's own figure, its baseline", allShare, '—', true, `${all.dropped} of ${all.frames + all.dropped} frames; ${worst}; the 1% display budget is the owner's iPhone's`);
    }
  }
  if (withoutDoor) {
    const without = withoutDoor['agents.interactive'].p50;
    const withIt = withDoor['agents.interactive'].p50;
    const allowed = round(without * ALLOWED + 20);
    say('the door makes the first screen no later, median (ms)', withIt, allowed, withIt !== null && without !== null && withIt <= allowed, `without the door ${without} ms`);
  }
  return out;
}

async function main() {
  const args = parse(process.argv.slice(2));
  const dev = device(args.platform);
  const lines = [];
  const log = (line) => {
    lines.push(line);
    console.log(line);
  };
  if (!dev.installed()) throw new Error('the app is not installed on this device');
  log(`Measuring on ${args.platform} (${dev.id}), ${args.runs} cold starts, ${new Date().toISOString()}`);
  // The machine's load beside the figures: a simulator shares the Mac with everything else on it.
  const load = () => os.loadavg().map((n) => n.toFixed(1)).join(' ');
  log(`Load average (1, 5, 15 min) at the start: ${load()}`);
  const startLoad = os.loadavg();

  log(`Cold starts with the door and without it (turned off by the test setting), in turn; the door opens in ${DOOR_MS} ms:`);
  const kinds = await series(dev, args.runs, args.settle, log);
  const withDoor = figures(kinds.on);
  const withoutDoor = figures(kinds.off);

  const baselines = fs.existsSync(BASELINES) ? JSON.parse(fs.readFileSync(BASELINES, 'utf8')) : {};
  const result = { platform: args.platform, device: dev.id, at: new Date().toISOString(), runs: args.runs, withDoor, withoutDoor };
  const checks = verdicts(args.platform, withDoor, withoutDoor, args.writeBaseline ? null : baselines[args.platform]);
  log(`Load average (1, 5, 15 min) at the end: ${load()}`);
  result.load = { start: startLoad, end: os.loadavg() };
  log('');
  for (const c of checks) log(`${c.ok ? 'ok  ' : 'FAIL'} ${c.name}: ${c.value} (limit ${c.limit})${c.note ? ` — ${c.note}` : ''}`);
  result.checks = checks;

  const failed = checks.filter((c) => !c.ok);
  if (args.writeBaseline && failed.length === 0) {
    const all = withDoor['door.all'];
    baselines[args.platform] = { at: result.at, 'agents.interactive.p95': withDoor['agents.interactive'].p95, 'agents.interactive.p50': withDoor['agents.interactive'].p50, 'door.opening.p50': withDoor['door.opening'].p50, 'javascript.loaded.p50': withDoor['javascript.loaded'].p50, 'door.dropped.share': all.frames + all.dropped === 0 ? 0 : round((all.dropped / (all.frames + all.dropped)) * 100) };
    fs.writeFileSync(BASELINES, `${JSON.stringify(baselines, null, 2)}\n`);
    log(`Baseline of ${args.platform} written to e2e/baselines.json`);
  } else if (args.writeBaseline) {
    // A run that missed a budget is no baseline: the last good one stays.
    log(`Baseline of ${args.platform} not written: a budget was missed`);
  }
  if (args.out) {
    fs.mkdirSync(args.out, { recursive: true });
    fs.writeFileSync(path.join(args.out, `measure-${args.platform}.json`), `${JSON.stringify(result, null, 2)}\n`);
    fs.writeFileSync(path.join(args.out, `measure-${args.platform}.log`), `${lines.join('\n')}\n`);
  }
  // A missed budget fails the measurement whether it checks a baseline or writes one.
  if (failed.length > 0) {
    console.error(`${failed.length} budget${failed.length === 1 ? '' : 's'} missed`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(2);
  });
}
