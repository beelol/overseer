// The processes a test leaves behind, found and stopped (audit 2026-09-28: about 80 leftovers of old
// runs on the owner's Mac: `overseerd shim` under /private/tmp/ovs-ui-*, `claude-fixture.js`, and
// `opencode run … mock/mock-coder` reparented to launchd).
//
// A test's processes are found by its temporary folder: the daemon, its shims and VS Code name it on
// their command line, and a harness (Claude or Codex fixture, OpenCode) works in a worktree or repo
// inside it. The children of any of those, and the process group each harness leads (the shim starts
// the harness as a group leader), go with them. Nothing outside a test folder is ever touched.
//
// Used by the UI harness (on exit, on a signal, after a failure) and by scripts/test-all (the check
// that a run left nothing running). Synchronous, so it works inside process.on('exit').
const cp = require('child_process');
const fs = require('fs');
const path = require('path');

/** Every process of this user: pid, parent, group, start time (ms) and command line. */
function list() {
  const out = cp.spawnSync('ps', ['-Ao', 'pid=,ppid=,pgid=,uid=,lstart=,command=', '-ww'], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }).stdout || '';
  const uid = process.getuid();
  const procs = [];
  for (const line of out.split('\n')) {
    const m = line.match(/^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\w{3} \w{3}\s+\d+ [\d:]+ \d{4})\s+(.*)$/);
    if (m && Number(m[4]) === uid) procs.push({ pid: +m[1], ppid: +m[2], pgid: +m[3], start: Date.parse(m[5]), cmd: m[6] });
  }
  return procs;
}

/** Working directory of each of this user's processes (lsof shows a deleted folder's old path too). */
function cwds() {
  const out = cp.spawnSync('lsof', ['-a', '-d', 'cwd', '-u', String(process.getuid()), '-Fpn'], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }).stdout || '';
  const map = new Map();
  let pid = 0;
  for (const line of out.split('\n')) {
    if (line[0] === 'p') pid = Number(line.slice(1));
    else if (line[0] === 'n' && pid) map.set(pid, line.slice(1));
  }
  return map;
}

/** /tmp and /private/tmp are one folder on macOS. */
function variants(root) {
  const r = root.replace(/\/+$/, '');
  if (r.startsWith('/private/tmp/')) return [r, r.slice('/private'.length)];
  if (r.startsWith('/tmp/')) return [r, '/private' + r];
  return [r];
}

/** This process and its ancestors: never stopped. */
function ancestors(procs) {
  const byPid = new Map(procs.map(p => [p.pid, p]));
  const out = new Set();
  for (let pid = process.pid; pid > 1 && !out.has(pid); pid = byPid.get(pid)?.ppid || 0) out.add(pid);
  return out;
}

/** Adds the children of `hit` and the members of the groups `hit` leads, until nothing changes. */
function spread(hit, procs, keep) {
  let grew = true;
  while (grew) {
    grew = false;
    for (const p of procs) {
      if (hit.has(p.pid) || keep.has(p.pid)) continue;
      if (hit.has(p.ppid) || (hit.has(p.pgid) && !keep.has(p.pgid))) { hit.add(p.pid); grew = true; }
    }
  }
  return hit;
}

/** The processes that belong to these test folders (command line or working directory inside one). */
function underRoots(roots, procs = list(), dirs = cwds()) {
  const keep = ancestors(procs);
  const all = roots.flatMap(variants);
  const inside = p => !!p && all.some(r => p === r || p.startsWith(r + '/'));
  const hit = new Set();
  for (const p of procs) {
    if (keep.has(p.pid)) continue;
    if (all.some(r => p.cmd.includes(r + '/') || p.cmd.endsWith(r)) || inside(dirs.get(p.pid))) hit.add(p.pid);
  }
  spread(hit, procs, keep);
  return procs.filter(p => hit.has(p.pid));
}

function alive(pid) { try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; } }
function pause(ms) { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); }

/** Stops these processes: SIGTERM, then SIGKILL for what is still there after `graceMs`. */
function stop(procs, graceMs = 1500) {
  const pids = procs.map(p => p.pid);
  for (const pid of pids) { try { process.kill(pid, 'SIGTERM'); } catch {} }
  const end = Date.now() + graceMs;
  while (Date.now() < end && pids.some(alive)) pause(100);
  for (const pid of pids.filter(alive)) { try { process.kill(pid, 'SIGKILL'); } catch {} }
  return pids;
}

/** Stops everything that belongs to these test folders, in rounds (a dying daemon may start nothing new, but check). */
function reap(roots, { graceMs = 1500 } = {}) {
  const stopped = [];
  for (let round = 0; round < 3; round++) {
    const found = underRoots(roots);
    if (!found.length) break;
    stop(found, graceMs);
    stopped.push(...found);
  }
  return stopped;
}

// ---------------------------------------------------------------- the check in scripts/test-all

/** Temporary folders the tests make: /tmp/ovs-… (Rust homes and repos, UI roots, dev and deploy roots). */
const TEST_PATH = /(?:\/private)?\/tmp\/(ovs-[A-Za-z0-9._-]+)/;
/** Fixture programs whose command line names no folder. */
const FIXTURE = /claude-fixture\.js|codex-fixture|mock-openai\/server\.js|mock\/mock-coder/;

/** The marker a test writes in its folder: which test-all run made it, and the process that owns it. */
function markFolder(root) {
  try { fs.writeFileSync(path.join(root, '.ovs-test-run'), JSON.stringify({ run: process.env.OVERSEER_TEST_RUN || null, pid: process.pid })); } catch {}
}
function folderMark(root) {
  for (const r of variants(root)) { try { return JSON.parse(fs.readFileSync(path.join(r, '.ovs-test-run'), 'utf8')); } catch {} }
  return null;
}

/**
 * What a test run left behind: processes started since `since` (ms) that were not alive before it
 * (`baseline`), that name a test folder or a fixture, and that nothing alive owns (their parent
 * chain reaches launchd through leftovers only). Returns { ours, others }:
 *   ours    in a folder this run marked (`run`), in a test folder that is gone (its test finished
 *           and deleted it), or a fixture working in this checkout;
 *   others  in a folder with no marker or another run's marker (a test of another checkout that
 *           may still be running): reported, never stopped.
 */
function leftovers({ baseline, since, run, checkout }) {
  const procs = list();
  const dirs = cwds();
  const keep = ancestors(procs);
  const byPid = new Map(procs.map(p => [p.pid, p]));
  const rootOf = p => { const m = p.cmd.match(TEST_PATH) || (dirs.get(p.pid) || '').match(TEST_PATH); return m ? '/private/tmp/' + m[1] : null; };
  const inCheckout = p => !!checkout && (dirs.get(p.pid) || '').startsWith(checkout + '/') || (dirs.get(p.pid) || '') === checkout;
  const hit = new Set();
  for (const p of procs) {
    if (keep.has(p.pid) || baseline.has(p.pid) || !(p.start >= since - 2000)) continue;
    if (rootOf(p) || FIXTURE.test(p.cmd)) hit.add(p.pid);
  }
  spread(hit, procs, new Set([...keep, ...baseline]));
  const ownedByLive = p => {
    for (let q = byPid.get(p.ppid); q && q.pid > 1; q = byPid.get(q.ppid)) if (!hit.has(q.pid)) return true;
    return false;
  };
  const ours = [], others = [];
  for (const p of procs) {
    if (!hit.has(p.pid) || ownedByLive(p)) continue;
    const root = rootOf(p);
    const item = { ...p, root, cwd: dirs.get(p.pid) || '' };
    if (root) {
      const mark = folderMark(root);
      if (!fs.existsSync(root) || (mark && mark.run === run)) ours.push(item);
      else if (mark && mark.pid && alive(mark.pid)) continue; // another test, still running
      else others.push(item);
    } else if (inCheckout(p)) ours.push(item);
    else others.push(item);
  }
  return { ours, others };
}

module.exports = { list, cwds, underRoots, reap, stop, leftovers, markFolder, folderMark };
