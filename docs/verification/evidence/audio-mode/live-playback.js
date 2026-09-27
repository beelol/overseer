#!/usr/bin/env node
// Live playback check for Audio Mode (Gate O) on macOS. It plays real sound (about 20 short cues). Run from
// the repository root after `cargo build --release -p overseerd`:
//
//     node docs/verification/evidence/audio-mode/live-playback.js
//
// A real overseerd runs with its own OVERSEER_HOME and no test sink, so cues go through
// /usr/bin/afplay and /usr/bin/say. The script watches the daemon's child processes to count
// players and how many run at once. The Commander folder is a synthetic one (three generated
// beeps) in a temporary directory; no private recording is read.
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const crypto = require('crypto');

const repo = path.resolve(__dirname, '../../../..');
const bin = process.env.OVERSEERD || path.join(repo, 'target/release/overseerd');
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-audio-live-')));
const home = path.join(root, 'overseer-home');
const env = { ...process.env, OVERSEER_HOME: home };
delete env.OVERSEER_TEST_AUDIO_LOG;
const delay = ms => new Promise(r => setTimeout(r, ms));
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const checks = [];
const check = (name, ok, detail) => { checks.push(!!ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail === undefined ? '' : '  ' + JSON.stringify(detail)}`); };

let daemon;
function spawnDaemon() {
  daemon = cp.spawn(bin, ['serve'], { env, stdio: 'ignore' });
}
function ctl(method, params = {}) {
  const out = cp.spawnSync(bin, ['ctl', method, JSON.stringify(params)], { env, encoding: 'utf8' });
  const msg = JSON.parse(out.stdout.split('\n')[0] || '{"error":{"message":"no reply"}}');
  if (msg.error) throw new Error(msg.error.message);
  return msg.result;
}
const tryCtl = (method, params) => { try { return { ok: ctl(method, params) }; } catch (error) { return { error: error.message }; } };
async function ready() {
  for (let i = 0; i < 100; i++) { if (tryCtl('hello').ok) return; await delay(100); }
  throw new Error('the daemon did not start');
}

// Players are direct children of the daemon. Each sample lists them; a watch collects every
// player seen and the most that ran at the same moment.
function players() {
  const out = cp.spawnSync('ps', ['-axo', 'pid=,ppid=,args='], { encoding: 'utf8' }).stdout;
  return out.split('\n').map(l => l.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/)).filter(m => m && Number(m[2]) === daemon.pid && /^\/usr\/bin\/(afplay|say)\b/.test(m[3])).map(m => ({ pid: m[1], args: m[3] }));
}
function watch() {
  const seen = new Map(); let most = 0, on = true;
  const loop = (async () => { while (on) { const now = players(); most = Math.max(most, now.length); for (const p of now) seen.set(p.pid, p.args); await delay(15); } })();
  return { stop: async () => { on = false; await loop; return { players: [...seen.values()], most }; } };
}
async function quiet(ms = 4000) { for (let t = 0; t < ms; t += 50) { if (!players().length) { await delay(150); if (!players().length) return; } await delay(50); } }
const rss = () => Number(cp.spawnSync('ps', ['-o', 'rss=', '-p', String(daemon.pid)], { encoding: 'utf8' }).stdout.trim());
function git(cwd, ...args) { return cp.execFileSync('git', args, { cwd, encoding: 'utf8' }).trim(); }
async function run(repoDir, script, title) {
  const created = ctl('task.create', { repo: repoDir, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title });
  for (let i = 0; i < 200; i++) {
    const status = ctl('state').runs.find(r => r.id === created.run.id)?.status;
    if (/completed|failed|interrupted/.test(status)) return status;
    await delay(100);
  }
  return 'timeout';
}
// One second of nothing much: a 440 Hz beep as 16-bit mono PCM.
function beep(file, seconds = 0.25) {
  const rate = 22050, n = Math.floor(rate * seconds), data = Buffer.alloc(n * 2);
  for (let i = 0; i < n; i++) data.writeInt16LE(Math.round(6000 * Math.sin(2 * Math.PI * 440 * i / rate) * Math.min(1, (n - i) / 2000)), i * 2);
  const head = Buffer.alloc(44);
  head.write('RIFF', 0); head.writeUInt32LE(36 + data.length, 4); head.write('WAVEfmt ', 8); head.writeUInt32LE(16, 16); head.writeUInt16LE(1, 20); head.writeUInt16LE(1, 22);
  head.writeUInt32LE(rate, 24); head.writeUInt32LE(rate * 2, 28); head.writeUInt16LE(2, 32); head.writeUInt16LE(16, 34); head.write('data', 36); head.writeUInt32LE(data.length, 40);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.concat([head, data]));
}

(async () => {
  try {
    console.log(`overseerd: ${path.relative(repo, bin)}  sha256 ${sha(bin).slice(0, 16)}…`);
    console.log(`macOS ${cp.execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8' }).trim()} ${os.arch()}; home ${home}\n`);
    const work = path.join(root, 'repo');
    fs.mkdirSync(work, { recursive: true });
    git(work, 'init', '-q', '-b', 'main'); git(work, 'config', 'user.name', 'Overseer Test'); git(work, 'config', 'user.email', 'overseer-test@example.invalid'); git(work, 'config', 'commit.gpgsign', 'false');
    fs.writeFileSync(path.join(work, 'README.md'), '# fixture\n'); git(work, 'add', '.'); git(work, 'commit', '-q', '-m', 'fixture base');
    spawnDaemon(); await ready();
    const log = () => { try { return fs.readFileSync(path.join(home, 'overseerd.log'), 'utf8'); } catch { return ''; } };
    const failures = () => log().split('\n').filter(l => /audio (playback failed|worker ended|selection failed)/.test(l));

    // Off: no player, no cache.
    let w = watch();
    const offStatus = await run(work, 'sleep 0.3; exit 0', 'Audio off');
    await delay(600);
    let seen = await w.stop();
    const first = ctl('audio.get');
    check('off: a finished agent starts no player and writes no cache', first.enabled === false && first.available === true && offStatus === 'completed' && seen.players.length === 0 && !fs.existsSync(path.join(home, 'audio')), { enabled: first.enabled, available: first.available, players: seen.players.length });

    // Reactor: start and completion, one player at a time, cache equal to the bundled cues.
    ctl('audio.set', { enabled: true });
    w = watch();
    const okStatus = await run(work, 'sleep 1.2; exit 0', 'Reactor start and complete');
    await delay(300); await quiet();
    seen = await w.stop();
    const cache = path.join(home, 'audio/reactor-v1');
    const cached = fs.existsSync(cache) ? fs.readdirSync(cache).sort() : [];
    const same = cached.every(f => sha(path.join(cache, f)) === sha(path.join(repo, 'daemon/assets/reactor', f)));
    const modes = cached.map(f => (fs.statSync(path.join(cache, f)).mode & 0o777).toString(8));
    check('Reactor: start and completion each play once through afplay, one at a time', okStatus === 'completed' && seen.players.length === 2 && seen.most === 1 && seen.players.every(a => a.startsWith('/usr/bin/afplay ' + cache)) && failures().length === 0, { ...seen, failures: failures() });
    check('Reactor: the cache holds only the cues played, identical to the bundled files and owner-only', cached.join() === 'agent_complete.mp3,agent_started.mp3' && same && modes.every(m => m === '600') && (fs.statSync(cache).mode & 0o777) === 0o700, { cached, same, modes, dir: (fs.statSync(cache).mode & 0o777).toString(8) });

    // A failed root needs the user: start, then attention.
    w = watch();
    const failStatus = await run(work, 'sleep 1.2; exit 3', 'Reactor attention');
    await delay(300); await quiet();
    seen = await w.stop();
    check('Reactor: a failed top-level agent plays the attention cue once', failStatus === 'failed' && seen.players.length === 2 && seen.most === 1 && /agent_needs_attention\.mp3$/.test(seen.players[1] || ''), seen);

    // A burst of requests: the queue refuses the excess and one player runs at a time.
    await delay(900);
    const before = rss();
    w = watch();
    let accepted = 0, refused = 0;
    for (let i = 0; i < 40; i++) { const r = tryCtl('audio.preview', { key: 'agent_progress' }); if (r.ok) accepted++; else if (/busy/.test(r.error)) refused++; }
    await delay(500); await quiet(8000);
    seen = await w.stop();
    const after = rss();
    check('a burst of 40 preview requests: at most 5 accepted (one playing, four queued), the rest refused, one player at a time', accepted >= 1 && accepted <= 5 && refused === 40 - accepted && seen.players.length === accepted && seen.most === 1, { accepted, refused, players: seen.players.length, most: seen.most });
    check('the daemon\'s memory stays level across the burst (resident size within 4 MB)', Math.abs(after - before) < 4096, { before_kb: before, after_kb: after });

    // System voice: spoken on this Mac by an installed voice.
    const voices = ctl('audio.voices');
    const voice = (voices.find(v => v.name === 'Daniel') || voices.find(v => /^en/.test(v.locale)) || voices[0]).name;
    const missing = tryCtl('audio.set', { track: 'system', voice: 'no-such-voice-12345' });
    ctl('audio.set', { track: 'system', voice });
    w = watch();
    ctl('audio.preview', { key: 'agent_started' });
    await delay(400); await quiet(8000);
    seen = await w.stop();
    check(`System voice: "${voice}" speaks through say; a voice that is not installed is refused`, seen.players.length === 1 && seen.players[0] === `/usr/bin/say -v ${voice} Agent started.` && /not installed/.test(missing.error || '') && voices.length > 0 && failures().length === 0, { installed: voices.length, player: seen.players, refused: missing.error });

    // Commander: a private folder, played where it is.
    const priv = path.join(root, 'private-commander');
    const refusedEarly = tryCtl('audio.set', { track: 'commander' });
    for (const key of first.default_keys) beep(path.join(priv, key, 'transmission/commander.wav'));
    const hashes = first.default_keys.map(key => sha(path.join(priv, key, 'transmission/commander.wav')));
    ctl('audio.import_commander', { path: priv });
    ctl('audio.set', { track: 'commander' });
    w = watch();
    ctl('audio.preview', { key: 'agent_complete' });
    await delay(400); await quiet(8000);
    seen = await w.stop();
    const copies = cp.spawnSync('find', [home, '-iname', '*.wav'], { encoding: 'utf8' }).stdout.trim();
    const inRepo = cp.spawnSync('git', ['status', '--porcelain', '--ignored'], { cwd: repo, encoding: 'utf8' }).stdout.split('\n').filter(l => /\.wav$/i.test(l));
    check('Commander: refused before a folder is chosen, then played in place from the private folder', /import the private Commander pack first/.test(refusedEarly.error || '') && seen.players.length === 1 && seen.players[0] === '/usr/bin/afplay ' + path.join(priv, 'agent_complete/transmission/commander.wav') && failures().length === 0, { refused: refusedEarly.error, player: seen.players });
    check('Commander: nothing was copied into the daemon\'s folder or the repository, and the private files are unchanged', copies === '' && inRepo.length === 0 && first.default_keys.every((key, i) => sha(path.join(priv, key, 'transmission/commander.wav')) === hashes[i]), { copies, inRepo });

    // The folder goes away: the track is unavailable, agents are not disturbed.
    fs.rmSync(priv, { recursive: true });
    w = watch();
    const goneStatus = await run(work, 'sleep 0.3; exit 0', 'Commander folder removed');
    await delay(800);
    seen = await w.stop();
    const gone = ctl('audio.get');
    const preview = tryCtl('audio.preview', { key: 'agent_started' });
    check('a missing Commander folder fails quietly: the agent completes, the failure is logged, the track reports unavailable', goneStatus === 'completed' && gone.available === false && gone.commander_imported === false && /unavailable/.test(preview.error || '') && failures().length > 0, { status: goneStatus, available: gone.available, preview: preview.error, logged: failures().length });

    // A daemon restart keeps the setting and replays nothing.
    ctl('audio.set', { track: 'reactor', voice: '' });
    daemon.kill('SIGKILL'); await delay(500);
    spawnDaemon(); await ready();
    w = watch();
    await delay(1500);
    seen = await w.stop();
    const kept = ctl('audio.get');
    check('after a daemon kill and restart Audio Mode is still on and no old cue is replayed', kept.enabled === true && kept.track === 'reactor' && seen.players.length === 0, { enabled: kept.enabled, track: kept.track, players: seen.players.length });
  } catch (error) {
    check('the script ran to the end', false, error.stack || error.message);
  } finally {
    try { ctl('daemon.shutdown'); } catch {}
    await delay(500);
    try { daemon.kill('SIGKILL'); } catch {}
    fs.rmSync(root, { recursive: true, force: true });
    const failed = checks.filter(ok => !ok).length;
    console.log(`\n${failed ? 'FAIL' : 'PASS'}  ${checks.length - failed} of ${checks.length} checks`);
    process.exit(failed ? 1 : 0);
  }
})();
