// The phone's scenarios, in the order they run. Each drives the app with a flow (e2e/flows) and
// then asks the daemon what happened, as the Mac sees it: a scenario passes only when both agree.

import net from 'node:net';

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
    says: 'every agent, those that need the owner first',
    async run(c) {
      await c.flow('agents', { SHOWCASE: c.runs.showcase, PERMISSION: c.runs.permission });
      c.shot('agents');
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
      c.lab.mode('slow');
      const one = c.lab.agent('slow', 'Count the stock', 'count');
      const two = c.lab.agent('showcase-permission', 'Price the returns', 'price');
      await c.until('two agents going', () => ACTIVE.includes(run(c, one).status) && ACTIVE.includes(run(c, two).status));
      await c.flow('stop-all');
      await c.until('every agent stopped', () => c.lab.call('state').runs.every((r) => !ACTIVE.includes(r.status)), 60_000);
      const asked = c.lab.call('events.list', { after: 0, limit: 5000 }).events.filter((e) => e.kind === 'remote_command' && e.payload.method === 'runs.stop_all');
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
    name: 'off-and-on',
    criteria: ['AC-116', 'AC-121'],
    says: 'phone access turned off on the Mac and on again: the app says so and comes back by itself',
    async run(c) {
      const port = c.port;
      c.lab.call('gateway.disable');
      await c.flow('off', { SHOWCASE: c.runs.showcase });
      c.shot('phone-access-off');
      expect(me(c).connected === false, 'the phone is still connected while phone access is off');
      c.lab.call('gateway.enable', { port });
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
      await c.flow('send-offline', { SHOWCASE: id, MESSAGE: message });
      expect(sent() === 0, 'the message reached the agent while phone access was off');
      c.lab.call('gateway.enable', { port: c.port });
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
      await c.flow('unreachable', { SHOWCASE: c.runs.showcase });
      c.lab.up();
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
      let file = null;
      for (const r of state.runs.filter((x) => x.id === c.runs.showcase)) file = c.lab.call('workspace.changes', { workspace_id: r.workspace_id }).names[0];
      expect(file, 'the showcase agent has no changed file to show');
      for (const theme of ['dark', 'light']) {
        for (const size of ['small', 'large']) {
          c.dev.appearance(theme);
          c.dev.textSize(size);
          await c.sleep(1500);
          await c.flow('tour', { SHOWCASE: c.runs.showcase, FILE: file, SHOTS: `${c.out}/screens/${theme}-${size}` });
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
    says: 'removed on the Mac, the phone is back on pairing and holds nothing of the Mac',
    async run(c) {
      c.lab.call('gateway.device_revoke', { id: c.deviceId });
      await c.flow('revoked');
      c.shot('revoked');
      expect(!c.dev.read('cache.state'), 'the phone still holds what it knew of the Mac');
      expect(!c.dev.read('overseer.pairing'), 'the phone still holds its pairing');
    },
  },
];
