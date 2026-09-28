// The phone's scenarios, in the order they run. Each drives the app with a flow (e2e/flows) and
// then asks the daemon what happened, as the Mac sees it: a scenario passes only when both agree.

import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

import { PIN } from './device.mjs';

/**
 * The storage namespaces that hold things of the Mac: the cached state and what the screens
 * learned (src/session/learned.ts). The connection library's pairing and the typed address are
 * checked by name. All gone after a revoke.
 */
const OF_THE_MAC = ['cache', 'agents', 'new', 'review', 'drafts', 'held'];

const ALLOWED_FIELDS = ['aps.alert.title', 'aps.alert.body', 'aps.category', 'aps.thread-id', 'aps.sound', 'aps.interruption-level', 'overseer.v', 'overseer.kind', 'overseer.run_id', 'overseer.task_id', 'overseer.request_id', 'overseer.device', 'Simulator Target Bundle'];
const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];
/** How long a flow takes to start watching the screen. */
const WATCH_FIRST_MS = 12_000;

const me = (c) => c.lab.call('gateway.devices').devices.find((d) => d.id === c.deviceId);
const run = (c, id) => c.lab.call('state').runs.find((r) => r.id === id);
const events = (c, runId) => c.lab.call('events.list', { run_id: runId, after: 0, limit: 5000 }).events;
/** A port nothing on this Mac listens on now: another lab or daemon may hold any fixed one. */
const freePort = () =>
  new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '0.0.0.0', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
const expect = (ok, what) => {
  if (!ok) throw new Error(what);
};


/** A regular expression that matches `text` exactly, as Maestro reads it. */
const exactly = (text) => text.replace(/[\\^$.|?*+()[\]{}]/g, '\\$&');

/** The lines `git diff` removes and adds for `file` in `dir` against `base`, in order. */
function gitDiff(dir, base, file) {
  const tracked = (() => {
    try {
      execFileSync('git', ['-C', dir, 'cat-file', '-e', `${base}:${file}`], { stdio: 'ignore' });
      return true;
    } catch {
      return false;
    }
  })();
  let text;
  try {
    text = tracked
      ? execFileSync('git', ['-C', dir, 'diff', '--no-color', '--no-ext-diff', '-U0', base, '--', file], { encoding: 'utf8' })
      : execFileSync('git', ['-C', dir, 'diff', '--no-color', '--no-index', '-U0', '/dev/null', file], { encoding: 'utf8' });
  } catch (error) {
    // `git diff --no-index` answers 1 when the files differ.
    text = error.stdout ?? '';
  }
  const removed = [];
  const added = [];
  for (const line of text.split('\n')) {
    if (/^(diff |index |--- |\+\+\+ |@@ |new file|deleted file|similarity|rename |old mode|new mode|\\ )/.test(line)) continue;
    if (line.startsWith('-')) removed.push(line.slice(1));
    else if (line.startsWith('+')) added.push(line.slice(1));
  }
  return { removed, added };
}


/** The X25519 public key of a 32-byte private key. */
function publicOf(secret) {
  const prefix = Buffer.from('302e020100300506032b656e04220420', 'hex');
  const key = crypto.createPrivateKey({ key: Buffer.concat([prefix, secret]), format: 'der', type: 'pkcs8' });
  return crypto.createPublicKey(key).export({ format: 'der', type: 'spki' }).subarray(-32);
}

/**
 * Every file under `dir` searched for a private key whose public key is `publicKey`: any 32 bytes
 * written as hex, base64 or base64url. Returns the files where one was found.
 */
function keyIn(dir, publicKey) {
  const found = [];
  const tried = new Map();
  const matches = (bytes) => {
    if (bytes.length !== 32) return false;
    const id = bytes.toString('hex');
    if (!tried.has(id)) {
      let same = false;
      try {
        same = publicOf(bytes).equals(publicKey);
      } catch {
        same = false;
      }
      tried.set(id, same);
    }
    return tried.get(id);
  };
  for (const entry of fs.readdirSync(dir, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const file = path.join(entry.parentPath ?? entry.path, entry.name);
    const text = fs.readFileSync(file).toString('latin1');
    let hit = false;
    for (const run of text.match(/[0-9a-fA-F]{64,}/g) ?? []) {
      for (let at = 0; !hit && at + 64 <= run.length; at += 2) hit = matches(Buffer.from(run.slice(at, at + 64), 'hex'));
    }
    // Base64 and base64url: every 43 characters of a run (a longer run is tried at each offset up
    // to 2,048 characters, beyond that at the 4-character steps a key inside it would keep).
    for (const run of text.match(/[A-Za-z0-9+/_-]{43,}/g) ?? []) {
      const step = run.length > 2048 ? 4 : 1;
      for (let at = 0; !hit && at + 43 <= run.length; at += step) hit = matches(Buffer.from(`${run.slice(at, at + 43).replace(/-/g, '+').replace(/_/g, '/')}=`, 'base64'));
    }
    if (hit) found.push(path.relative(dir, file));
  }
  return found;
}

export const scenarios = [
  {
    name: 'pair',
    criteria: ['AC-117', 'AC-118', 'AC-141'],
    says: 'a code typed on the phone, confirmed on the Mac, and the agents list',
    needed: true,
    always: true,
    async run(c) {
      const already = c.lab.call('gateway.devices').devices.find((d) => d.platform === c.platform && !d.revoked_ms);
      c.name = already ? already.name : `Lab ${c.platform}`;
      // A phone that is paired already (working on scenarios) is not paired again: it never is.
      if (already) await c.flow('opened');
      else await c.flow('pair', { CODE: c.lab.code(), NAME: c.name, NOTIFICATIONS: 'pair.notifications.allow' });
      const device = await c.until('the phone on the list of the Mac, connected', () => c.lab.call('gateway.devices').devices.find((d) => d.name === c.name && d.connected));
      c.deviceId = device.id;
      expect(device.scope === 'full', `a new phone has full control, this one has ${device.scope}`);
      expect(device.platform === c.platform, `the Mac knows the phone as ${device.platform}`);
      c.runs = c.lab.info().runs;
      c.shot('paired');
    },
  },
  {
    name: 'agents',
    criteria: ['AC-124'],
    says: 'every agent, those that need the owner first; every agent of the Mac has its row on the phone',
    async run(c) {
      await c.flow('agents', { SHOWCASE: c.runs.showcase, PERMISSION: c.runs.permission });
      c.shot('agents');
      // Nine agents or more, a nested child among them: the lab starts with fewer, so echo fixtures are added.
      const { agents: model, store } = await import('../model/src/index.ts');
      const listed = () => model.agentRows(store.load(c.lab.call('state')), { now: Date.now() }).filter((r) => r.runId && r.kind !== 'needs');
      const titles = ['Label the shelves', 'Check the invoices', 'Tag the photos', 'Merge the carts', 'Trim the logs', 'Name the branches'];
      for (let i = 0; new Set(listed().map((r) => r.runId)).size < 9 && i < titles.length; i += 1) c.lab.agent('echo', titles[i], 'hello');
      await c.until('the Mac with nine agents or more, their first turns over', () => {
        const state = c.lab.call('state');
        return new Set(listed().map((r) => r.runId)).size >= 9 && !state.runs.some((r) => ['queued', 'starting'].includes(r.status));
      }, 60_000);
      // The rows the phone must show: its own view model (parity-tested against VS Code's side
      // bar) over the daemon's state. Each is looked for on the screen, above or below.
      const rows = listed();
      const ids = [...new Set(rows.map((r) => r.runId))];
      expect(ids.length >= 9, `the Mac has only ${ids.length} agents; the check needs nine or more`);
      const steps = ids.flatMap((id) => [
        `- scrollUntilVisible:\n    element:\n      id: ${JSON.stringify(exactly(`agents.row.${id}`))}\n    direction: DOWN\n    timeout: 8000\n    optional: true`,
        `- scrollUntilVisible:\n    element:\n      id: ${JSON.stringify(exactly(`agents.row.${id}`))}\n    direction: UP\n    timeout: 8000`,
      ]);
      const file = path.join(path.dirname(c.out), 'maestro', c.platform, 'generated', 'agents-every-row.yaml');
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, `# Every agent of the Mac, as the phone's view model lists it from the daemon's state.\nappId: \${APP}\n---\n- extendedWaitUntil:\n    visible:\n      id: "agents.screen"\n    timeout: 30000\n${steps.join('\n')}\n`);
      await c.flow(file);
      c.log.say(`  ${ids.length} agents of the Mac (nested children among them: ${rows.filter((r) => r.kind === 'child').length}), each found on the phone`);
      return { agents: ids.length, children: rows.filter((r) => r.kind === 'child').length };
    },
  },
  {
    name: 'delay',
    criteria: ['AC-124'],
    says: "a line an agent prints reaches the phone's screen: the delay from the Mac's event to the frame that shows it",
    async run(c) {
      c.dev.remove('perf.delay');
      const created = c.lab.call('task.create', {
        repo: c.lab.info().repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Timed lines',
        args: ['-c', 'i=0; while [ $i -lt 400 ]; do echo line$i; i=$((i+1)); sleep 0.1; done'],
      });
      const id = created.run.id;
      await c.flow('watch-stream', { RUN: id });
      await c.sleep(20_000);
      const delay = await c.until('the delays the app recorded', () => {
        const raw = c.dev.read('perf.delay');
        const summary = raw ? JSON.parse(JSON.parse(raw)) : null;
        return summary && summary.count >= 50 ? summary : null;
      }, 30_000);
      const round = (n) => Math.round(n * 10) / 10;
      const clock = c.dev.clockOffset();
      const load = os.loadavg()[0].toFixed(1);
      c.log.say(`  from the Mac's event to the frame that shows it: ${delay.count} lines, p50 ${round(delay.p50)} ms, p95 ${round(delay.p95)} ms, longest ${round(delay.max)} ms (AC-58 holds a VS Code tile to 250 ms); the device's clock ${clock} ms from the Mac's; load average ${load}`);
      if (ACTIVE.includes(run(c, id).status)) c.lab.call('run.interrupt', { run_id: id });
      return { delay: { count: delay.count, p50: round(delay.p50), p95: round(delay.p95), max: round(delay.max), clockOffsetMs: clock, load: Number(load) } };
    },
  },
  {
    name: 'conversation',
    criteria: ['AC-124'],
    says: "an agent's conversation opens with its composer",
    async run(c) {
      await c.flow('conversation', { SHOWCASE: c.runs.showcase });
    },
  },
  {
    name: 'permission',
    criteria: ['AC-125'],
    says: 'a permission request allowed from the phone unblocks the agent',
    async run(c) {
      const id = c.runs.permission;
      expect(run(c, id).status === 'waiting_for_user', 'the fixture agent is not waiting for permission');
      await c.flow('permission', { PERMISSION: id });
      await c.until('the agent going on', () => run(c, id).status !== 'waiting_for_user');
      const answer = events(c, id).find((e) => e.kind === 'permission_answered');
      expect(answer, 'the daemon has no answer to the request');
      expect(answer.source === `phone:${c.name}`, `the answer's source is ${answer.source}, not the phone`);
      expect(answer.payload.allow === true, 'the answer is not Allow');
    },
  },
  {
    name: 'send',
    criteria: ['AC-122', 'AC-125'],
    says: 'a message typed on the phone reaches the agent exactly once',
    async run(c) {
      const id = c.runs.showcase;
      await c.until('the agent being idle', () => !ACTIVE.includes(run(c, id).status));
      c.lab.mode('echo');
      const message = `tidy the totals again ${Date.now() % 100000}`;
      await c.flow('send', { SHOWCASE: id, MESSAGE: message });
      const sent = () => c.lab.call('run.turns', { run_id: id }).filter((t) => t.prompt === message).length;
      await c.until('the message reaching the agent', () => sent() >= 1);
      await c.sleep(3000);
      expect(sent() === 1, `the agent received the message ${sent()} times`);
      const command = events(c, id).filter((e) => e.kind === 'remote_command' && e.source === `phone:${c.name}`);
      expect(command.length >= 1, 'the daemon did not record the message as coming from the phone');
    },
  },
  {
    name: 'image',
    criteria: ['AC-125'],
    says: "an image chosen in the system's photo picker reaches the Claude fixture as an image block",
    async run(c) {
      const id = c.runs.showcase;
      await c.until('the agent being idle', () => !ACTIVE.includes(run(c, id).status));
      c.lab.mode('echo');
      c.dev.addPhoto(path.join(path.dirname(new URL(import.meta.url).pathname), '..', '..', 'docs', 'design', 'brand', 'overseer-app-icon.png'));
      const message = `what is in this picture ${Date.now() % 100000}`;
      await c.flow('send-image', { RUN: id, MESSAGE: message });
      await c.until('the message reaching the agent', () => c.lab.call('run.turns', { run_id: id }).some((t) => t.prompt === message), 60_000);
      // What the daemon wrote to the fixture's input for that turn: the image as a content block.
      const workspace = c.lab.call('state').workspaces.find((w) => w.id === run(c, id).workspace_id);
      const log = path.join(c.lab.info().home, 'stdin', `${path.basename(workspace.path)}.log`);
      const block = await c.until('the image block in what the fixture read', () => {
        if (!fs.existsSync(log)) return null;
        for (const line of fs.readFileSync(log, 'utf8').split('\n').filter(Boolean)) {
          let parsed;
          try {
            parsed = JSON.parse(line);
          } catch {
            continue;
          }
          const content = parsed?.message?.content;
          if (!Array.isArray(content) || !content.some((b) => b?.type === 'text' && String(b.text).includes(message))) continue;
          const image = content.find((b) => b?.type === 'image');
          if (image) return image;
        }
        return null;
      }, 30_000);
      const bytes = Buffer.from(block.source?.data ?? '', 'base64');
      expect(block.source?.type === 'base64' && block.source?.media_type === 'image/jpeg', `the image block is ${JSON.stringify({ ...block, source: { ...block.source, data: undefined } })}`);
      expect(bytes[0] === 0xff && bytes[1] === 0xd8, 'the image block does not hold a JPEG');
      c.log.say(`  the fixture read an image block: image/jpeg, ${bytes.length} bytes, with the message's text`);
      return { mime: block.source.media_type, bytes: bytes.length };
    },
  },
  {
    name: 'review',
    criteria: ['AC-126'],
    says: "an agent's changed files, a file's hunks, and a hunk marked reviewed",
    async run(c) {
      const state = c.lab.call('state');
      let found = null;
      for (const r of state.runs.filter((x) => !x.parent_run_id)) {
        const changes = c.lab.call('workspace.changes', { workspace_id: r.workspace_id });
        if (changes.files > 0) {
          found = { run: r.id, file: changes.names[0] };
          break;
        }
      }
      expect(found, 'no fixture agent has changed a file');
      const before = c.lab.call('review.marks', { run_id: found.run }).keys.length;
      await c.flow('review', { RUN: found.run, FILE: found.file });
      await c.until('the mark on the Mac', () => c.lab.call('review.marks', { run_id: found.run }).keys.length > before);
      const mark = c.lab.call('review.marks', { run_id: found.run }).marks.at(-1);
      expect(String(mark.by).includes(c.name), `the mark was made by ${mark.by}`);
      c.review = found;
    },
  },
  {
    name: 'diff',
    criteria: ['AC-126'],
    says: "a file's diff on the phone is git diff for the comparison, line for line; a mark made on the Mac shows on the phone",
    async run(c) {
      expect(c.review, 'the review scenario found no changed file');
      const { run: runId, file } = c.review;
      const agent = run(c, runId);
      const base = c.lab.call('comparison.options', { run_id: runId }).options.find((o) => o.mode === 'task_start' && o.available)?.base;
      expect(base, 'the Mac offers no comparison since the task started');
      const workspace = c.lab.call('state').workspaces.find((w) => w.id === agent.workspace_id);
      const answered = c.lab.call('workspace.hunks', { workspace_id: agent.workspace_id, path: file, base, run_id: runId });
      // The rows the phone draws, from the phone's own review model.
      const { review } = await import('../model/src/index.ts');
      const drawn = review.fileDiff(answered);
      const rows = drawn.hunks.flatMap((h) => h.rows);
      const git = gitDiff(workspace.path, base, file);
      const same = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
      expect(same(rows.filter((r) => r.kind === 'removed').map((r) => r.text), git.removed), `the removed lines differ from git diff: ${JSON.stringify(git.removed).slice(0, 200)}`);
      expect(same(rows.filter((r) => r.kind === 'added').map((r) => r.text), git.added), `the added lines differ from git diff: ${JSON.stringify(git.added).slice(0, 200)}`);
      // A mark made on the Mac: the second hunk marked reviewed, or with one hunk, the phone's mark taken away.
      const hunks = answered.hunks;
      const second = hunks[1];
      if (second) c.lab.call('review.accept', { run_id: runId, ...review.acceptParams(file, second) });
      else c.lab.call('review.unaccept', { run_id: runId, key: hunks[0].key });
      // The hunk's button says what it would do: take the mark away from a reviewed hunk, or accept it.
      const expectations = second
        ? [[hunks[0].key, 'Unmark reviewed hunk 1'], [second.key, 'Unmark reviewed hunk 2']]
        : [[hunks[0].key, 'Accept hunk 1']];
      // Every line on the screen, as the phone says it: its number and its text.
      const shown = (line) => line.replace(/\t/g, '  ').slice(0, 2000);
      const steps = rows.flatMap((row) => {
        const label = row.kind === 'removed' ? `Removed, line ${row.baseLine ?? ''}: ${shown(row.text)}` : `Added, line ${row.modifiedLine ?? ''}: ${shown(row.text)}`;
        return [
          `- scrollUntilVisible:\n    element:\n      id: ${JSON.stringify(exactly(`file.line.${row.key}`))}\n    direction: DOWN\n    timeout: 15000`,
          `- assertVisible:\n    id: ${JSON.stringify(exactly(`file.line.${row.key}`))}\n    text: ${JSON.stringify(exactly(label))}`,
        ];
      });
      const marks = expectations.map(([key, words]) => `- scrollUntilVisible:\n    element:\n      id: "file\\\\.hunk\\\\.accept"\n      text: "${words}"\n      childOf:\n        id: ${JSON.stringify(exactly(`file.hunk.${key}`))}\n    direction: UP\n    timeout: 15000`);
      const flows = path.join(path.dirname(new URL(import.meta.url).pathname), 'flows');
      // Written beside Maestro's record, the flow names its subflows by their full path.
      const open = fs.readFileSync(path.join(flows, 'open-file.yaml'), 'utf8').replace(/(runFlow:\s*(?:\n\s*file:\s*)?)([\w.-]+\.yaml)/g, (_, lead, file) => `${lead}${path.join(flows, file)}`);
      const file_ = path.join(path.dirname(c.out), 'maestro', c.platform, 'generated', 'diff-lines.yaml');
      fs.mkdirSync(path.dirname(file_), { recursive: true });
      fs.writeFileSync(file_, `${open.trimEnd()}\n${steps.join('\n')}\n${marks.join('\n')}\n- takeScreenshot: diff-lines\n`);
      await c.flow(file_, { RUN: runId, FILE: file });
      c.log.say(`  ${rows.length} lines in ${hunks.length} hunks equal git diff and are on the screen; the Mac's mark shows on the phone`);
      return { lines: rows.length, hunks: hunks.length };
    },
  },
  {
    name: 'reject',
    criteria: ['AC-126', 'AC-130'],
    says: 'a hunk rejected from the phone is asked about once and then gone from the worktree',
    async run(c) {
      expect(c.review, 'the review scenario found no changed file');
      const workspace = run(c, c.review.run).workspace_id;
      const names = c.lab.call('workspace.changes', { workspace_id: workspace }).names;
      const file = names[1] ?? names[0];
      const base = c.lab.call('comparison.options', { run_id: c.review.run }).options.find((o) => o.mode === 'task_start' && o.available)?.base;
      expect(base, 'the Mac offers no comparison for this agent');
      const hunks = () => c.lab.call('workspace.hunks', { workspace_id: workspace, path: file, base }).hunks?.length ?? 0;
      const before = hunks();
      expect(before > 0, `${file} has no hunk to reject`);
      await c.flow('reject', { RUN: c.review.run, FILE: file });
      await c.until('the lines being back in the worktree', () => hunks() === before - 1);
      const rejected = events(c, c.review.run).find((e) => e.kind === 'review_reject');
      expect(rejected && rejected.source === `phone:${c.name}`, 'the daemon did not record the phone as the one who rejected');
    },
  },
  {
    name: 'big-repo',
    criteria: ['AC-126', 'AC-135'],
    says: 'an agent in a repository of 10,000 files changes 500: the phone lists them, and the list scrolls without dropped frames',
    async run(c) {
      const dir = path.join(path.dirname(c.lab.info().repo), 'warehouse');
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
        const git = (...args) => execFileSync('git', args, { cwd: dir, stdio: 'ignore' });
        git('init', '-q', '-b', 'main');
        git('config', 'user.name', 'Lab');
        git('config', 'user.email', 'lab@example.invalid');
        git('config', 'commit.gpgsign', 'false');
        for (let d = 0; d < 100; d += 1) {
          const shelf = path.join(dir, `shelf${String(d).padStart(2, '0')}`);
          fs.mkdirSync(shelf);
          for (let f = 0; f < 100; f += 1) fs.writeFileSync(path.join(shelf, `item${String(f).padStart(2, '0')}.txt`), `shelf ${d} item ${f}\n`);
        }
        git('add', '.');
        git('commit', '-q', '-m', '10,000 files');
      }
      const created = c.lab.call('task.create', {
        repo: dir, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Restock the warehouse',
        args: ['-c', 'for f in shelf0[0-4]/item*.txt; do echo restocked >> "$f"; done'],
      });
      const id = created.run.id;
      await c.until('the agent done', () => !ACTIVE.includes(run(c, id)?.status ?? 'queued'), 120_000);
      const changes = c.lab.call('workspace.changes', { workspace_id: run(c, id).workspace_id });
      expect(changes.files === 500, `the Mac counts ${changes.files} changed files, not 500`);
      c.dev.remove('perf.scroll');
      await c.flow('big-repo', { RUN: id, FIRST: 'shelf00/item00.txt' });
      const raw = c.dev.read('perf.scroll');
      const scroll = raw ? JSON.parse(JSON.parse(raw)).changes : null;
      expect(scroll && scroll.scrolls >= 5, `the app timed ${scroll?.scrolls ?? 0} scrolls of the list`);
      c.log.say(`  10,000 files, 500 changed: ${scroll.scrolls} scrolls of the list, ${scroll.frames} frames, ${scroll.dropped} dropped (${scroll.droppedPercent}%), longest ${scroll.longest} ms; load average ${os.loadavg()[0].toFixed(1)}`);
      // The display budget (AC-135: at most 1% of frames dropped) is the simulator's to meet here and
      // the iPhone's in the end; the emulator draws through the Mac's GPU and keeps its own figure.
      if (c.platform === 'ios') expect(scroll.droppedPercent <= 1, `${scroll.dropped} of ${scroll.frames + scroll.dropped} frames dropped while the list scrolled (${scroll.droppedPercent}%, the budget is 1%)`);
      else c.log.say(`  the emulator's own figure, recorded as its baseline: ${scroll.droppedPercent}% dropped`);
      return { files: changes.files, scroll };
    },
  },
  {
    name: 'cleanup',
    criteria: ['AC-130', 'AC-127'],
    says: 'clean up a worktree with uncommitted files: the phone names them and asks once; the worktree is gone',
    async run(c) {
      const created = c.lab.call('task.create', {
        repo: c.lab.info().repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Draft the returns note',
        args: ['-c', "printf 'returns within 30 days\\n' > returns-draft.txt; echo more >> README.md"],
      });
      const id = created.run.id;
      await c.until('the agent done', () => !ACTIVE.includes(run(c, id)?.status ?? 'queued'), 60_000);
      const workspace = run(c, id).workspace_id;
      const where = c.lab.call('state').workspaces.find((w) => w.id === workspace).path;
      expect(fs.existsSync(path.join(where, 'returns-draft.txt')), 'the agent left no uncommitted file');
      await c.flow('cleanup', { RUN: id, LOST: 'returns-draft\\.txt' });
      const gone = await c.until('the worktree removed', () => c.lab.call('state').workspaces.find((w) => w.id === workspace)?.removed_ms ?? null, 30_000);
      expect(!fs.existsSync(where), 'the worktree folder is still there');
      return { removed_ms: gone };
    },
  },
  {
    name: 'merge',
    criteria: ['AC-130', 'AC-127'],
    says: 'merge back from the phone: aborted after a conflict, and completed, each asked once',
    async run(c) {
      const repo = c.lab.info().repo;
      const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim();
      fs.writeFileSync(path.join(repo, 'prices.txt'), 'price list\n');
      git('add', 'prices.txt');
      git('commit', '-q', '-m', 'prices');
      const agentOf = (title, script) => c.lab.call('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title, args: ['-c', script] }).run.id;
      // Abort: the agent and the main branch change the same line.
      const clash = agentOf('Reprice the list', "echo 'agent prices' > prices.txt && git commit -qam 'agent prices'");
      const plain = agentOf('Write the stock note', "echo 'stock is fine' > stock-note.txt && git add stock-note.txt && git commit -qm 'stock note'");
      await c.until('both agents done', () => [clash, plain].every((id) => !ACTIVE.includes(run(c, id)?.status ?? 'queued')), 60_000);
      fs.writeFileSync(path.join(repo, 'prices.txt'), 'main prices\n');
      git('commit', '-qam', 'main prices');
      await c.flow('merge-abort', { RUN: clash });
      const clashWorkspace = run(c, clash).workspace_id;
      const after = c.lab.call('workspace.merge_plan', { workspace_id: clashWorkspace });
      expect(after.ok && after.state === 'idle', `after the abort the merge is ${after.state}`);
      expect(!git('log', '--oneline', 'main').includes('agent prices'), 'the aborted merge reached main');
      // Complete: nothing in the way.
      await c.flow('merge-complete', { RUN: plain });
      expect(git('ls-tree', '--name-only', 'main').split('\n').includes('stock-note.txt'), "the agent's commit did not land on main");
      return { aborted: clash, completed: plain };
    },
  },
  {
    name: 'safety',
    criteria: ['AC-130'],
    says: "both safety settings turned on from the phone, each with the device's own unlock: the app is locked when opened, and Stop all asks for the unlock",
    async run(c) {
      const matched = async (times = 4) => {
        // Face ID is matched from the Mac once the prompt is up; a match with no prompt does nothing.
        for (let i = 0; i < times; i += 1) {
          await c.sleep(1500);
          c.dev.unlockMatch();
        }
      };
      const stored = (key) => c.dev.read(`settings.${key}`) === 'true';
      c.dev.unlockSetUp();
      try {
        // The unlock before changes first: once the app lock is on, every launch meets the lock.
        await c.flow('safety-toggle', { SWITCH: 'settings.safety.unlock', PIN });
        await matched();
        await c.flow('safety-is-on', { SWITCH: 'settings.safety.unlock', SHOT: 'safety-unlock-on' });
        await c.flow('safety-toggle', { SWITCH: 'settings.safety.lock', PIN });
        await matched();
        await c.flow('safety-is-on', { SWITCH: 'settings.safety.lock', SHOT: 'safety-lock-on' });
        expect(stored('appLock') && stored('unlockBeforeChanges'), 'the phone did not keep both settings on');
        // Opened again: covered until the device's unlock.
        await c.flow('locked-open', { PIN });
        await matched();
        await c.flow('unlocked');
        // A change that cannot be undone: Stop all asks once, then for the unlock.
        const waiting = c.lab.agent('showcase-permission', 'Weigh the crates', 'weigh the crates');
        await c.until('the agent waiting for the owner', () => run(c, waiting)?.status === 'waiting_for_user', 60_000);
        await c.flow('stop-all-unlock', { PIN });
        await matched();
        await c.until('the agent stopped after the unlock', () => !ACTIVE.includes(run(c, waiting).status), 30_000);
        return { appLock: true, unlockBeforeChanges: true };
      } finally {
        // Back to the defaults for the scenarios that follow, and the device as it was.
        c.dev.stop();
        c.dev.write('settings.appLock', false);
        c.dev.write('settings.unlockBeforeChanges', false);
        c.dev.unlockTearDown();
      }
    },
  },
  {
    name: 'backup',
    criteria: ['AC-130'],
    says: "an attempt to read the phone's key from everything the app keeps on disk (more than a backup carries) fails",
    async run(c) {
      const db = path.join(c.lab.info().home, 'overseer.sqlite');
      const hex = execFileSync('sqlite3', [db, `select public_key from devices where id = '${c.deviceId}'`], { encoding: 'utf8' }).trim();
      expect(/^[0-9a-f]{64}$/.test(hex), "the Mac has no public key for this phone");
      const publicKey = Buffer.from(hex, 'hex');
      const copy = fs.mkdtempSync(path.join(os.tmpdir(), 'overseer-backup-'));
      try {
        c.dev.appFiles(copy);
        const files = fs.readdirSync(copy, { recursive: true }).length;
        // The search is proven first: a key planted in the copy, as the app would write it, is found.
        const planted = crypto.randomBytes(32);
        fs.writeFileSync(path.join(copy, 'planted.json'), JSON.stringify({ devicePrivateKey: planted.toString('hex') }));
        expect(keyIn(copy, publicOf(planted)).includes('planted.json'), 'the search did not find a planted key');
        fs.rmSync(path.join(copy, 'planted.json'));
        const found = keyIn(copy, publicKey);
        expect(found.length === 0, `the phone's private key was read from ${found.join(', ')}`);
        const items = c.dev.keychainItems();
        // On iOS the key is in the keychain, marked for this device only: a backup never carries it.
        if (c.platform === 'ios') expect(items.length > 0 && items.every((i) => i.accessible === 'cku'), `the app's keychain items: ${JSON.stringify(items)}`);
        c.log.say(`  ${files} files of the app searched: no private key of this phone; a planted key was found; keychain items ${JSON.stringify(items)}`);
        return { files, keychain: items };
      } finally {
        fs.rmSync(copy, { recursive: true, force: true });
      }
    },
  },
  {
    name: 'new',
    criteria: ['AC-125'],
    says: 'a new agent started from the phone',
    async run(c) {
      c.lab.mode('echo');
      const task = `count the products ${Date.now() % 100000}`;
      const before = c.lab.call('state').runs.length;
      await c.flow('new', { TASK: task });
      const made = await c.until('the new agent on the Mac', () => {
        const state = c.lab.call('state');
        return state.runs.length > before && state.runs.find((r) => c.lab.call('run.turns', { run_id: r.id }).some((t) => t.prompt === task));
      });
      expect(made.harness === 'claude', `the agent runs ${made.harness}`);
      const all = c.lab.call('events.list', { after: 0, limit: 5000 }).events;
      const created = all.find((e) => e.kind === 'task_created' && e.run_id === made.id);
      const asked = all.filter((e) => e.kind === 'remote_command' && e.payload.method === 'task.create' && e.seq < created.seq).at(-1);
      expect(asked && asked.source === `phone:${c.name}` && created.seq - asked.seq <= 3, 'the daemon did not record the phone as the one who started it');
    },
  },
  {
    name: 'accounts',
    criteria: ['AC-127'],
    says: 'the accounts of the Mac',
    async run(c) {
      await c.flow('accounts');
    },
  },
  {
    name: 'notifications',
    criteria: ['AC-129'],
    says: 'notifications switched off on the phone are off on the Mac, and on again',
    async run(c) {
      expect(me(c).notifications === true, 'notifications are not on after the owner allowed them');
      await c.flow('notifications-off');
      await c.until('the switch reaching the Mac', () => me(c).notifications === false);
      const quiet = c.lab.agent('showcase-permission', 'Ask while notifications are off', 'ask quietly');
      const push = await c.until('the send log', () => events(c, quiet).find((e) => e.kind === 'push'));
      expect(push.payload.outcome === 'not_sent' && /off on this phone/.test(push.payload.why), `with the switch off the daemon logged: ${push.payload.outcome}, ${push.payload.why}`);
      c.lab.call('run.interrupt', { run_id: quiet });
      await c.flow('notifications-on');
      await c.until('the switch reaching the Mac', () => me(c).notifications === true);
    },
  },
  {
    name: 'banner',
    criteria: ['AC-129'],
    says: 'with the app open, an agent that needs the owner shows as a banner; a tap opens the agent',
    platforms: ['android'],
    skipped: 'iOS shows the system\'s own notification (the scenario push)',
    async run(c) {
      await c.flow('opened');
      // The flow watches first; the agent asks while it watches (a banner stays a few seconds).
      const watching = c.flow('banner');
      await c.sleep(WATCH_FIRST_MS);
      const asking = c.lab.agent('showcase-permission', 'Weigh the parcels', 'weigh the parcels');
      await watching;
      const logged = await c.until('the send log', () => events(c, asking).find((e) => e.kind === 'push' && e.payload.device === c.deviceId));
      expect(logged.payload.route === 'in_app', `the daemon's route for this phone is ${logged.payload.route}`);
      c.lab.call('run.interrupt', { run_id: asking });
    },
  },
  {
    name: 'push',
    criteria: ['AC-129'],
    says: "the daemon's own notification on the simulator: a tap opens the agent, Allow unblocks it",
    platforms: ['ios'],
    skipped: 'Android shows its notifications itself while the app is open, in this gate',
    async run(c) {
      await c.flow('home');
      // The flow watches first; the agent asks while it watches (a banner stays a few seconds).
      const opening = c.flow('notification-open');
      await c.sleep(WATCH_FIRST_MS);
      const first = c.lab.agent('showcase-permission', 'Round the prices', 'round the prices');
      const sent = await c.until('the daemon sending', () => events(c, first).find((e) => e.kind === 'push' && e.payload.device === c.deviceId));
      expect(sent.payload.outcome === 'sent' && sent.payload.route === 'simulator', `the daemon logged: ${sent.payload.route}, ${sent.payload.outcome}, ${sent.payload.why}`);
      const extra = sent.payload.fields.filter((f) => !ALLOWED_FIELDS.includes(f));
      expect(extra.length === 0, `the notification holds fields that are not allowed: ${extra.join(', ')}`);
      const seconds = (sent.ts - events(c, first).find((e) => e.kind === 'permission').ts) / 1000;
      expect(seconds <= 5, `the notification was sent ${seconds} s after the request`);
      await opening;
      c.shot('notification-opened');
      await c.flow('home');
      const allowing = c.flow('notification-allow');
      await c.sleep(WATCH_FIRST_MS);
      const second = c.lab.agent('showcase-permission', 'Sort the receipts', 'sort the receipts');
      await c.until('the daemon sending', () => events(c, second).find((e) => e.kind === 'push' && e.payload.outcome === 'sent'));
      await allowing;
      await c.until('the agent going on', () => run(c, second).status !== 'waiting_for_user');
      const answer = events(c, second).find((e) => e.kind === 'permission_answered');
      expect(answer && answer.source === `phone:${c.name}` && answer.payload.allow === true, 'Allow on the notification did not reach the agent as the phone\'s answer');
      c.lab.call('run.interrupt', { run_id: first });
    },
  },
  {
    name: 'stop-all',
    criteria: ['AC-125', 'AC-130'],
    says: 'Stop all agents names how many will stop, asks once, and stops them',
    async run(c) {
      // Two agents that stay going until they are stopped: each waits for the owner's answer.
      // (The lab's slow agent is done in 400 ms, often before it was seen going.)
      const one = c.lab.agent('showcase-permission', 'Count the stock', 'count');
      const two = c.lab.agent('showcase-permission', 'Price the returns', 'price');
      await c.until('two agents going', () => ACTIVE.includes(run(c, one).status) && ACTIVE.includes(run(c, two).status));
      // Only this scenario's Stop all counts: another scenario (safety) sent one of its own.
      const since = c.lab.call('state').cursor;
      await c.flow('stop-all');
      await c.until('every agent stopped', () => c.lab.call('state').runs.every((r) => !ACTIVE.includes(r.status)), 60_000);
      const asked = c.lab.call('events.list', { after: since, limit: 5000 }).events.filter((e) => e.kind === 'remote_command' && e.payload.method === 'runs.stop_all');
      expect(asked.length === 1 && asked[0].source === `phone:${c.name}`, 'the daemon did not record one Stop all from the phone');
    },
  },
  {
    name: 'update',
    criteria: ['AC-141'],
    says: 'a new build installed over the old one keeps the pairing: the app opens on the agents list',
    async run(c) {
      const paired = me(c).paired_ms;
      c.dev.stop();
      c.dev.install(c.app);
      await c.flow('opened');
      await c.until('the phone connected again', () => me(c)?.connected, 60_000);
      expect(me(c).paired_ms === paired, 'the phone was paired again after the update');
    },
  },
  {
    name: 'reopen',
    criteria: ['AC-141', 'AC-130'],
    says: 'opened five times, the app never asks to pair, sign in or unlock',
    async run(c) {
      const paired = me(c).paired_ms;
      for (let i = 0; i < 5; i += 1) await c.flow('opened');
      await c.until('the phone connected again', () => me(c)?.connected);
      expect(me(c).paired_ms === paired, 'the phone was paired again');
      expect(c.lab.call('gateway.devices').devices.filter((d) => !d.revoked_ms).length === 1, 'the Mac lists more than one phone');
    },
  },
  {
    name: 'restart',
    criteria: ['AC-141'],
    says: 'the phone restarted: the app opens on the agents list with no prompt and connects by itself',
    async run(c) {
      const paired = me(c).paired_ms;
      c.dev.reboot();
      await c.flow('opened');
      await c.until('the phone connected again', () => me(c)?.connected, 90_000);
      expect(me(c).paired_ms === paired, 'the phone was paired again');
      c.shot('restarted');
    },
  },
  {
    name: 'off-and-on',
    criteria: ['AC-116', 'AC-121'],
    says: 'phone access turned off on the Mac and on again: the app says so and comes back by itself',
    async run(c) {
      const port = c.port;
      c.lab.call('gateway.disable');
      try {
        await c.flow('off', { SHOWCASE: c.runs.showcase });
        c.shot('phone-access-off');
        expect(me(c).connected === false, 'the phone is still connected while phone access is off');
      } finally {
        // On again whatever the flow found: the scenarios after this one need it.
        c.lab.call('gateway.enable', { port });
      }
      await c.flow('on', { SHOWCASE: c.runs.showcase });
      await c.until('the phone connected again', () => me(c).connected);
    },
  },
  {
    name: 'queued',
    criteria: ['AC-122', 'AC-121'],
    says: 'a message typed while phone access is off is kept, shown, and sent once when it is on again',
    async run(c) {
      const id = c.runs.showcase;
      await c.until('the agent being idle', () => !ACTIVE.includes(run(c, id).status));
      c.lab.mode('echo');
      const message = `count the receipts ${Date.now() % 100000}`;
      const sent = () => c.lab.call('run.turns', { run_id: id }).filter((t) => t.prompt === message).length;
      c.lab.call('gateway.disable');
      try {
        await c.flow('send-offline', { SHOWCASE: id, MESSAGE: message });
        expect(sent() === 0, 'the message reached the agent while phone access was off');
      } finally {
        c.lab.call('gateway.enable', { port: c.port });
      }
      await c.until('the message reaching the agent', () => sent() >= 1, 90_000);
      await c.sleep(4000);
      expect(sent() === 1, `the agent received the message ${sent()} times`);
    },
  },
  {
    name: 'unreachable',
    criteria: ['AC-123', 'AC-116', 'AC-121'],
    says: 'the Mac gone without a word: the app says unreachable with the last contact, and comes back by itself',
    async run(c) {
      c.lab.down();
      try {
        await c.flow('unreachable', { SHOWCASE: c.runs.showcase });
      } finally {
        // The Mac comes back whatever the flow found: the scenarios after this one need it.
        c.lab.up();
      }
      await c.flow('reachable');
      await c.until('the phone connected again', () => me(c).connected, 60_000);
      expect(c.lab.call('gateway.status').enabled === true, 'phone access did not stay on across the restart of the daemon');
    },
  },
  {
    name: 'manual-address',
    criteria: ['AC-120'],
    says: 'the Mac answers somewhere else: an address typed by the owner connects, with no pairing again',
    async run(c) {
      const paired = me(c).paired_ms;
      const moved = await freePort();
      c.lab.call('gateway.disable');
      try {
        c.lab.call('gateway.enable', { port: moved });
      } catch (error) {
        // The scenarios after this one need phone access on.
        c.lab.call('gateway.enable', { port: c.port });
        throw error;
      }
      c.port = moved;
      await c.sleep(1500);
      expect(me(c).connected === false, 'the phone is connected although the Mac moved');
      await c.flow('manual-address', { ADDRESS: `${c.dev.host}:${moved}` });
      await c.until('the phone connected through the typed address', () => me(c).connected, 90_000);
      expect(me(c).paired_ms === paired, 'the phone was paired again');
    },
  },
  {
    name: 'away',
    criteria: ['AC-121'],
    says: 'five minutes in the background while an agent writes numbered lines, the daemon restarted in the middle: every event once, in order',
    async run(c) {
      const minutes = Number(process.env.OVERSEER_AWAY_MINUTES || 5);
      const lines = Math.round((minutes * 60 + 90) * 8);
      const created = c.lab.call('task.create', {
        repo: c.lab.info().repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', prompt: '', title: 'Numbered lines',
        args: ['-c', `i=0; while [ $i -lt ${lines} ]; do echo line$i; i=$((i+1)); sleep 0.125; done`],
      });
      const id = created.run.id;
      await c.flow('away-start', { RUN: id });
      const half = (minutes * 60_000) / 2;
      c.log.say(`  the app is in the background for ${minutes} minutes; the daemon restarts half way`);
      await c.sleep(half);
      c.lab.down();
      await c.sleep(3000);
      c.lab.up();
      await c.sleep(half);
      await c.flow('away-return');
      const newest = () => c.lab.call('state').cursor;
      const stream = await c.until('the phone catching up with the Mac', () => {
        const raw = c.dev.read('perf.stream');
        const stats = raw ? JSON.parse(JSON.parse(raw)) : null;
        return stats && stats.last >= newest() - 40 ? stats : null;
      }, 120_000);
      c.lab.call('run.interrupt', { run_id: id });
      const final = await c.until('the phone at the end of the stream', () => {
        const stats = JSON.parse(JSON.parse(c.dev.read('perf.stream')));
        return !ACTIVE.includes(run(c, id).status) && stats.last >= newest() ? stats : null;
      }, 60_000);
      c.log.say(`  the Mac's newest event is ${newest()}; the phone received ${final.count} events up to ${final.last}, gaps ${final.gaps}, duplicates ${final.duplicates}, reloads ${final.truncated}`);
      expect(stream.gaps === 0 && final.gaps === 0, `the phone missed events: ${final.gaps} gaps in the sequence`);
      expect(final.duplicates === 0, `the phone received ${final.duplicates} events twice`);
      const written = events(c, id).filter((e) => e.kind === 'output').length;
      expect(written > minutes * 60 * 4, `the agent wrote only ${written} lines while the phone was away`);
      c.shot('stream-caught-up');
    },
  },
  {
    name: 'tour',
    criteria: ['AC-131'],
    says: 'every screen in both themes, at the smallest and the largest text size; the theme changed with the app open',
    async run(c) {
      const state = c.lab.call('state');
      let shown = null;
      for (const r of state.runs.filter((x) => !x.parent_run_id)) {
        const changes = c.lab.call('workspace.changes', { workspace_id: r.workspace_id });
        if (changes.files > 0) {
          shown = { run: r.id, file: changes.names[0] };
          break;
        }
      }
      expect(shown, 'no agent has a changed file to show');
      const file = shown.file;
      c.runs = { ...c.runs, tour: shown.run };
      for (const theme of ['dark', 'light']) {
        for (const size of ['small', 'large']) {
          c.dev.appearance(theme);
          c.dev.textSize(size);
          await c.sleep(1500);
          await c.flow('tour', { SHOWCASE: c.runs.tour, FILE: file, SHOTS: `${c.out}/screens/${theme}-${size}` });
        }
      }
      // The system's setting, changed while the app is open: the app follows at once.
      c.dev.textSize('standard');
      c.dev.appearance('dark');
      await c.flow('opened');
      c.shot('theme-dark-before-the-switch');
      c.dev.appearance('light');
      await c.sleep(2500);
      c.shot('theme-light-after-the-switch-with-the-app-open');
      c.dev.appearance('dark');
      await c.sleep(2500);
      c.shot('theme-dark-again');
    },
  },
  {
    name: 'busy',
    criteria: ['AC-135', 'AC-137'],
    says: 'an animation drops no frame while the logic is held for 500 ms',
    async run(c) {
      c.dev.remove('perf.busy');
      await c.flow('busy');
      const frames = await c.until('the frames counted', () => {
        const raw = c.dev.read('perf.busy');
        return raw ? JSON.parse(JSON.parse(raw)) : null;
      });
      c.log.say(`  frames ${frames.frames}, dropped ${frames.dropped} (${frames.droppedPercent}%), longest ${frames.longest} ms, frame time ${frames.period} ms`);
      c.busy = frames;
      // A display of 60 Hz or more must show its frames; an emulator that draws slower records
      // its own count and is held to no dropped frame at its own pace.
      if (frames.period <= 25) expect(frames.frames >= 30, `only ${frames.frames} frames were drawn in ${frames.seconds} s`);
      else c.log.say(`  this display draws a frame every ${frames.period} ms: its own pace is the baseline`);
      expect(frames.dropped === 0, `${frames.dropped} frames were dropped while the logic was busy (longest ${frames.longest} ms)`);
    },
  },
  {
    name: 'watch-only',
    criteria: ['AC-119'],
    says: 'made watch only on the Mac, the phone shows everything and no control that changes something',
    async run(c) {
      c.lab.call('gateway.device_scope', { id: c.deviceId, scope: 'watch' });
      await c.flow('watch', { SHOWCASE: c.runs.showcase });
      c.shot('watch-only');
      c.lab.call('gateway.device_scope', { id: c.deviceId, scope: 'full' });
      await c.flow('full');
    },
  },
  {
    name: 'revoke',
    criteria: ['AC-119'],
    // The run measures the budgets before this one: they need the app paired.
    unpairs: true,
    says: 'removed on the Mac, the phone is back on pairing and holds nothing of the Mac',
    async run(c) {
      // The scenarios before this one opened agents, looked at the New agent form and typed an
      // address: the phone holds things of the Mac beyond its pairing and the cached state.
      expect(c.dev.read('agents.seen'), 'the phone holds nothing of the Mac yet: the check would prove nothing');
      c.lab.call('gateway.device_revoke', { id: c.deviceId });
      await c.flow('revoked');
      c.shot('revoked');
      expect(!c.dev.read('cache.state'), 'the phone still holds what it knew of the Mac');
      expect(!c.dev.read('overseer.pairing'), 'the phone still holds its pairing');
      const kept = c.dev.keys().filter((key) => OF_THE_MAC.some((namespace) => key.startsWith(`${namespace}.`)) || key === 'discovery.manual');
      expect(kept.length === 0, `the phone still holds ${kept.join(', ')}`);
      c.log.say(`storage after the revoke: ${c.dev.keys().join(', ') || 'nothing'}`);
    },
  },
  {
    // An experiment, run only when named (`--only pair,tap-timing`): Maestro's first tap on a hunk
    // was lost now and then on the Android emulator. Taps sent by adb itself, at set moments after
    // the file screen shows its hunks, say whether the app drops a tap or only Maestro's.
    name: 'tap-timing',
    optIn: true,
    criteria: ['AC-126'],
    says: "experiment: a tap on a hunk's Accept, sent by adb the moment the file screen shows it, 1.2 s later and 3 s later: is any lost?",
    platforms: ['android'],
    skipped: 'an experiment for the Android emulator',
    async run(c) {
      const state = c.lab.call('state');
      let found = null;
      for (const r of state.runs.filter((x) => !x.parent_run_id)) {
        const changes = c.lab.call('workspace.changes', { workspace_id: r.workspace_id });
        if (changes.files > 0) {
          found = { run: r.id, file: changes.names[0] };
          break;
        }
      }
      expect(found, 'no fixture agent has changed a file');
      const adb = path.join(process.env.ANDROID_HOME || '/opt/homebrew/share/android-commandlinetools', 'platform-tools', 'adb');
      const marked = () => c.lab.call('review.marks', { run_id: found.run }).keys.length;
      const clear = () => {
        for (const key of c.lab.call('review.marks', { run_id: found.run }).keys) c.lab.call('review.unaccept', { run_id: found.run, key });
      };
      // Where Accept is, read from the settled screen by Maestro's own view of it.
      clear();
      await c.flow('open-file', { RUN: found.run, FILE: found.file });
      await c.sleep(3000);
      const tree = execFileSync('maestro', ['--udid', c.dev.id, 'hierarchy'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: { ...process.env, MAESTRO_CLI_NO_ANALYTICS: '1' } });
      const bounds = tree.match(/"resource-id"\s*:\s*"file\.hunk\.accept"[\s\S]*?"bounds"\s*:\s*"\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
      expect(bounds, 'Accept was not found on the screen');
      const x = Math.round((Number(bounds[1]) + Number(bounds[3])) / 2);
      const y = Math.round((Number(bounds[2]) + Number(bounds[4])) / 2);
      c.log.say(`  Accept is at ${x}, ${y}`);
      const tally = {};
      const tries = Number(process.env.OVERSEER_TAP_TRIES) || 12;
      for (const wait of [0, 1200, 3000]) {
        tally[wait] = { marked: 0, lost: 0 };
        for (let i = 1; i <= tries; i += 1) {
          clear();
          await c.flow('open-file', { RUN: found.run, FILE: found.file });
          if (wait > 0) await c.sleep(wait);
          const cursor = c.lab.call('state').cursor;
          execFileSync(adb, ['-s', c.dev.id, 'shell', 'input', 'tap', String(x), String(y)]);
          let landed = false;
          const until = Date.now() + 5000;
          while (Date.now() < until) {
            if (marked() > 0) {
              landed = true;
              break;
            }
            await c.sleep(200);
          }
          // What the phone asked of the Mac: accept, unaccept (the button showed a remembered mark) or nothing.
          const sent = c.lab.call('events.list', { after: cursor, limit: 500 }).events.filter((e) => e.kind === 'remote_command' && String(e.payload.method).startsWith('review.')).map((e) => e.payload.method);
          tally[wait][landed ? 'marked' : 'lost'] += 1;
          tally[wait].sent = [...(tally[wait].sent ?? []), sent.join('+') || 'nothing'];
          c.log.say(`  a tap ${wait} ms after the hunks showed, try ${i}: ${landed ? 'marked' : 'LOST'}; the phone sent ${sent.join(', ') || 'nothing'}`);
        }
      }
      clear();
      c.log.say(`  taps by adb: ${Object.entries(tally).map(([w, t]) => `${w} ms after: ${t.marked} marked, ${t.lost} lost`).join('; ')}`);
      return tally;
    },
  },
  {
    // The same question with no Maestro between the opening and the tap: adb opens the file from the
    // changes list and taps Accept a set time later. Run only when named (`--only pair,tap-open`).
    name: 'tap-open',
    optIn: true,
    criteria: ['AC-126'],
    says: 'experiment: the file opened by adb from the changes list, Accept tapped by adb 300, 600, 1000 and 2000 ms later: is any tap lost?',
    platforms: ['android'],
    skipped: 'an experiment for the Android emulator',
    async run(c) {
      const state = c.lab.call('state');
      let found = null;
      for (const r of state.runs.filter((x) => !x.parent_run_id)) {
        const changes = c.lab.call('workspace.changes', { workspace_id: r.workspace_id });
        if (changes.files > 0) {
          found = { run: r.id, file: changes.names[0] };
          break;
        }
      }
      expect(found, 'no fixture agent has changed a file');
      const adb = (...args) => execFileSync(path.join(process.env.ANDROID_HOME || '/opt/homebrew/share/android-commandlinetools', 'platform-tools', 'adb'), ['-s', c.dev.id, ...args]);
      const tap = ([x, y]) => adb('shell', 'input', 'tap', String(x), String(y));
      const where = (id) => {
        const tree = execFileSync('maestro', ['--udid', c.dev.id, 'hierarchy'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: { ...process.env, MAESTRO_CLI_NO_ANALYTICS: '1' } });
        const escaped = id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const m = tree.match(new RegExp(`"resource-id"\\s*:\\s*"${escaped}"[\\s\\S]*?"bounds"\\s*:\\s*"\\[(\\d+),(\\d+)\\]\\[(\\d+),(\\d+)\\]"`));
        expect(m, `${id} was not found on the screen`);
        return [Math.round((Number(m[1]) + Number(m[3])) / 2), Math.round((Number(m[2]) + Number(m[4])) / 2)];
      };
      const marked = () => c.lab.call('review.marks', { run_id: found.run }).keys.length;
      const clear = () => {
        for (const key of c.lab.call('review.marks', { run_id: found.run }).keys) c.lab.call('review.unaccept', { run_id: found.run, key });
      };
      clear();
      await c.flow('open-changes', { RUN: found.run, FILE: found.file });
      await c.sleep(3000);
      const row = where(`changes.row.${found.file}`);
      tap(row);
      await c.sleep(3000);
      const accept = where('file.hunk.accept');
      adb('shell', 'input', 'keyevent', '4');
      await c.sleep(2000);
      c.log.say(`  the row is at ${row.join(', ')}, Accept at ${accept.join(', ')}`);
      const shots = path.join(c.out, c.platform, 'tap-open');
      fs.mkdirSync(shots, { recursive: true });
      const tally = {};
      for (const wait of [300, 600, 1000, 2000]) {
        tally[wait] = { marked: 0, lost: 0 };
        for (let i = 1; i <= 10; i += 1) {
          clear();
          await c.sleep(1500);
          const cursor = c.lab.call('state').cursor;
          tap(row);
          await c.sleep(wait);
          tap(accept);
          // What the screen showed just after the tap, for a tap that is lost.
          const shot = adb('exec-out', 'screencap', '-p');
          let landed = false;
          const until = Date.now() + 5000;
          while (Date.now() < until) {
            if (marked() > 0) {
              landed = true;
              break;
            }
            await c.sleep(200);
          }
          // What the phone asked of the Mac after the tap: accept, unaccept (the button still showed a
          // remembered mark) or nothing (the tap never reached the button).
          const sent = c.lab.call('events.list', { after: cursor, limit: 500 }).events.filter((e) => e.kind === 'remote_command' && String(e.payload.method).startsWith('review.')).map((e) => e.payload.method);
          if (!landed) fs.writeFileSync(path.join(shots, `lost-${wait}ms-${i}.png`), shot);
          tally[wait][landed ? 'marked' : 'lost'] += 1;
          tally[wait].sent = [...(tally[wait].sent ?? []), sent.join('+') || 'nothing'];
          c.log.say(`  opened by adb, Accept tapped ${wait} ms later, try ${i}: ${landed ? 'marked' : 'LOST'}; the phone sent ${sent.join(', ') || 'nothing'}`);
          adb('shell', 'input', 'keyevent', '4');
          await c.sleep(1500);
        }
      }
      clear();
      c.log.say(`  opened and tapped by adb: ${Object.entries(tally).map(([w, t]) => `${w} ms after: ${t.marked} marked, ${t.lost} lost`).join('; ')}`);
      return tally;
    },
  },
];
