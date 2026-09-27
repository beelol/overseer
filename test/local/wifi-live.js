#!/usr/bin/env node
// LIVE check of AC-83 that only the owner can run: Wi-Fi off and on again while a real `overseerd`
// (its own OVERSEER_HOME, the real network, the real system answer and probes) watches. The
// script never changes the network itself; it reads Wi-Fi power with `networksetup
// -getairportpower` and the daemon's connection events, and measures the gap between them.
//
//   node test/local/wifi-live.js            the owner turns Wi-Fi off when asked, then on again
//   node test/local/wifi-live.js watch      the same, waiting quietly for hours (WIFI_PATIENCE_MIN,
//                                           default 720): started for the owner, whose whole step is
//                                           to switch Wi-Fi off for about 15 seconds and on again
//   node test/local/wifi-live.js rehearse   the same script with only the system's answer simulated
//                                           (the script flips it; the probes are real, Wi-Fi is not
//                                           touched): checks the script
//
// Writes docs/verification/evidence/ac-83/wifi.txt and wifi-events.jsonl (the rehearsal writes to
// its temporary folder). Nothing of the user's is written; the daemon is stopped at the end.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const rehearse = process.argv.includes('rehearse');
const watch = process.argv.includes('watch');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-wifi-'));
const out = rehearse ? path.join(home, 'evidence') : path.join(root, 'docs/verification/evidence/ac-83');
const PATIENCE = Number(process.env.WIFI_PATIENCE_MIN || (watch ? 720 : 10)) * 60 * 1000;
// Wi-Fi power is read once a second while waiting, so a measured gap may be up to this much short.
const POLL = 1000;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const clock = ms => new Date(ms).toISOString().slice(11, 23);
const redact = s => s.split(home).join('/OVERSEER_HOME').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/').replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, 'EMAIL');

// The rehearsal replaces only the system's answer (OVERSEER_TEST_SYSTEM_NET); the probes stay real.
const CONNECTED = 'connected', NO_NETWORK = 'none';
const sysFile = path.join(home, 'system-answer');
let fixture = CONNECTED;
const flip = v => { fixture = v; fs.writeFileSync(sysFile + '.tmp', v); fs.renameSync(sysFile + '.tmp', sysFile); };
const env = { ...process.env, OVERSEER_HOME: home };
if (rehearse) { flip(CONNECTED); Object.assign(env, { OVERSEER_TEST_SYSTEM_NET: sysFile }); }

const log = [], results = [];
const say = line => { console.log(line); log.push(`${clock(Date.now())}  ${line}`); };
const check = (what, ok, detail) => { results.push(!!ok); say(`${ok ? 'PASS' : 'FAIL'}  ${what}${detail ? '  — ' + detail : ''}`); };
const ctl = (method, params) => {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 20000, maxBuffer: 64 * 1024 * 1024 });
  const msg = JSON.parse((r.stdout || '').split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
};
const status = () => (ctl('connection.status') || {}).status;
const connections = () => ((ctl('events.list', { limit: 5000 }) || {}).events || []).filter(e => e.kind === 'connection');

// The Wi-Fi device, read once; its power, read every time (a read, never a write).
const device = (() => {
  const r = spawnSync('networksetup', ['-listallhardwareports'], { encoding: 'utf8' });
  const m = /Hardware Port: Wi-Fi\nDevice: (\S+)/.exec(r.stdout || '');
  return m ? m[1] : 'en0';
})();
const wifi = () => {
  if (rehearse) return fixture === CONNECTED ? 'On' : 'Off';
  const r = spawnSync('networksetup', ['-getairportpower', device], { encoding: 'utf8' });
  return /: On/.test(r.stdout || '') ? 'On' : /: Off/.test(r.stdout || '') ? 'Off' : 'unknown';
};

async function until(what, pred, limit) {
  const started = Date.now();
  for (;;) {
    const v = pred();
    if (v) return v;
    if (Date.now() - started > limit) throw new Error(`${what} did not happen within ${Math.round(limit / 1000)} s`);
    await sleep(250);
  }
}

async function main() {
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  const t = {};
  try {
    await until('the daemon', () => !ctl('hello').error, 15000);
    say(`overseerd ${ctl('hello').version}; Wi-Fi device ${device} (power ${wifi()}); ${rehearse ? 'REHEARSAL: only the system\'s answer is simulated (the script flips it); the probes are real; Wi-Fi is not touched' : 'real network, real system answer, real probes; no fixtures'}`);
    const online = await until('online', () => { const s = status(); return s && s.state === 'online' ? s : null; }, 90000);
    say(`connection: online (${online.reason}); the system says: ${online.system.detail}`);
    say('\n== Turn Wi-Fi off now (the menu bar, or System Settings). Leave it off until this script says Overseer is offline; then turn it on again.');
    const started = Date.now();
    let told = false;
    while (Date.now() - started < PATIENCE) {
      const now = Date.now();
      if (rehearse && !t.wifiOff && now - started > 2000) flip(NO_NETWORK);
      const power = wifi();
      if (!t.wifiOff && power === 'Off') { t.wifiOff = now; say(`${clock(now)}  Wi-Fi is off`); }
      if (!t.wifiOff) { await sleep(POLL); continue; } // quiet until the switch: one read a second
      const s = status();
      if (s && t.wifiOff && !t.systemNoNetwork && s.system.state === 'no_network') t.systemNoNetwork = now;
      const ev = connections();
      const off = t.wifiOff && ev.find(e => e.ts >= t.wifiOff - 1000 && e.payload.status.state === 'offline');
      if (off && !t.offline) {
        t.offline = off.ts; t.offlineReason = off.payload.status.reason; t.offlineSystem = off.payload.status.system.detail;
        say(`${clock(off.ts)}  Overseer: offline (${off.payload.status.reason}), ${((off.ts - t.wifiOff) / 1000).toFixed(1)} s after Wi-Fi went off; the system said: ${off.payload.status.system.detail}`);
      }
      if (t.offline && !told) { told = true; say('\n== Turn Wi-Fi on again now.'); }
      if (rehearse && t.offline && !t.wifiOn && now - t.offline > 2000) flip(CONNECTED);
      if (t.offline && !t.wifiOn && power === 'On' && now > t.offline) { t.wifiOn = now; say(`${clock(now)}  Wi-Fi is on`); }
      const on = t.wifiOn && ev.find(e => e.ts >= t.wifiOn - 1000 && e.payload.status.state === 'online');
      if (on) {
        t.online = on.ts; t.onlineReason = on.payload.status.reason;
        say(`${clock(on.ts)}  Overseer: online (${on.payload.status.reason}), ${((on.ts - t.wifiOn) / 1000).toFixed(1)} s after Wi-Fi came back`);
        break;
      }
      await sleep(250);
    }
    const final = status() || {};
    const events = connections();
    const between = events.filter(e => t.offline && e.ts > t.offline && (!t.online || e.ts < t.online)).map(e => `${clock(e.ts)} ${e.payload.status.state} (${e.payload.status.reason})`);
    say('');
    check('Wi-Fi was turned off and Overseer went offline', t.wifiOff && t.offline, t.wifiOff ? (t.offline ? '' : 'Overseer never went offline') : 'Wi-Fi was not turned off within 10 minutes');
    check('offline within 10 s of the system signal', t.offline && t.offline - t.wifiOff + (rehearse ? 0 : POLL) <= 10000, t.offline ? `${((t.offline - t.wifiOff) / 1000).toFixed(1)} s after Wi-Fi power was seen off${rehearse ? '' : ' (read once a second, so at most 1 s more)'}; the daemon's own reading of the system's no-network answer ${t.systemNoNetwork ? `${((t.systemNoNetwork - t.wifiOff) / 1000).toFixed(1)} s` : 'was not seen'} after` : 'Overseer never went offline');
    check("the reason is the system's own answer", t.offlineReason === 'no network (system)', t.offlineReason);
    check('online again after the checks agree', t.online && final.state === 'online' && final.baseline && final.baseline.by_name.ok && final.baseline.by_ip.ok && Object.values(final.providers || {}).every(h => h.reachable !== false),
      t.online ? `${((t.online - t.wifiOn) / 1000).toFixed(1)} s after Wi-Fi came back; baseline by name ${final.baseline && final.baseline.by_name.reason}, by IP ${final.baseline && final.baseline.by_ip.reason}; providers ${Object.entries(final.providers || {}).map(([k, h]) => `${k} ${h.reachable === false ? 'unreachable' : 'reachable'}`).join(', ')}` : 'Overseer never came back online');
    say(`states in between: ${between.length ? between.join('; ') : 'none'}`);
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, 'wifi.txt'), redact(log.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, 'wifi-events.jsonl'), [JSON.stringify({ kind: 'timeline', device, rehearsal: rehearse, ...t }), ...events.map(e => JSON.stringify({ seq: e.seq, ts: e.ts, kind: e.kind, payload: e.payload }))].map(redact).join('\n') + '\n');
    say(`\nwritten: ${path.relative(root, out)}/wifi.txt and wifi-events.jsonl`);
  } catch (e) {
    check('the check ran to its end', false, e.message);
  } finally {
    ctl('daemon.shutdown');
    daemon.kill();
    await sleep(300);
  }
  const failed = results.filter(ok => !ok).length;
  say(`\n${results.length - failed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
}
main().catch(e => { console.error(e); process.exit(2); });
