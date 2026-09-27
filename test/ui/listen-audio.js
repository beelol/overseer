// Listening session for AC-145 (Audio Mode by ear). It plays real sound.
//
//     node extension/scripts/package.js && node test/ui/listen-audio.js
//
// Opens VS Code with its own profile and its own Overseer home, with the packaged VSIX installed,
// so the owner's own VS Code, daemon and agents are not touched. Agents are fixtures: no account
// is used and no tokens are spent. Keys typed in this terminal make the events to listen for, and
// they keep working after that VS Code window is closed. `m` asks for the marks and writes the
// session's record to docs/verification/evidence/ui/audio-listening/. It asks only about the steps this
// session did: a step that was not done is recorded as not tried, whatever was heard elsewhere.
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

// Records of earlier sessions are kept: a new session adds its own file.
const kept = path.join(repoRoot, 'docs/verification/evidence/ui/audio-listening');
const earlier = fs.existsSync(kept) ? fs.readdirSync(kept).filter(f => /^marks.*\.json$/.test(f)).map(f => [f, fs.readFileSync(path.join(kept, f))]) : [];
const s = new Session('audio-listening');
for (const [name, data] of earlier) fs.writeFileSync(path.join(s.evidence, name), data);
const recordFile = `marks-${new Date().toISOString().replace(/[-:]/g, '').replace('T', '-').slice(0, 15)}.json`;
const barrier = path.join(s.root, 'release-permissions');
const say = text => process.stdout.write(text + '\n');
const tryCtl = (method, params) => { try { return s.ctl(method, params); } catch (error) { say(`  ${method}: ${error.message}`); return null; } };
const state = id => s.ctl('state').runs.find(r => r.id === id);
const waitStatus = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 200) { if (re.test(state(id)?.status || '')) return true; await delay(200); } return false; };
const record = { date: new Date().toISOString().slice(0, 10), heard: [], events: [], marks: {}, steps: {} };
const note = text => { record.heard.push(`${new Date().toISOString().slice(11, 19)} ${text}`); say(`  ${text}`); };
const windowOpen = () => !!cp.spawnSync('pgrep', ['-f', s.profile], { encoding: 'utf8' }).stdout.trim();
/** What the session did, with the settings and the window at that moment. */
function did(key) {
  const audio = s.ctl('audio.get');
  const event = { key, enabled: audio.enabled, track: audio.track, commander_folder_set: audio.commander_imported, vscode: windowOpen() ? 'open' : 'closed' };
  record.events.push(event);
  return `Audio Mode ${event.enabled ? 'on' : 'off'} · ${event.track} · VS Code ${event.vscode}`;
}
let cues = [], repo, n = 0, busy = false;

const KEYS = '123456789abc';
function menu() {
  const audio = s.ctl('audio.get');
  say(`\nAudio Mode is ${audio.enabled ? 'ON' : 'off'} · track ${audio.track}${audio.voice ? ' · voice ' + audio.voice : ''}${audio.available ? '' : ' · playback unavailable'}`);
  say('  o  turn Audio Mode on            f  turn it off (then s and n must be silent)');
  cues.forEach((c, i) => say(`  ${KEYS[i]}  preview ${c.label.padEnd(20)} ${c.default_auto ? 'plays by itself' : ''}`));
  say('  s  an agent starts and completes 3 s later        start, then complete');
  say('  n  an agent needs you (asks for permission)       start, then attention');
  say('  t  two agents need you at the same moment         one attention cue, count 2 in VS Code');
  say('  v  System voice            r  Reactor            p  your Commander folder (import it in VS Code first)');
  say('  x  stop the fixture agents');
  say('  m  give your marks and write the record           q  quit\n');
}

async function generic() {
  const t = tryCtl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 3; exit 0'], prompt: '', title: `Listening ${++n}: completes` });
  if (!t) return;
  note(`an agent started; it completes in 3 s  [${did('s')}]`);
  await waitStatus(t.run.id, /completed|failed/);
  note('the agent completed');
}

async function needs(count) {
  fs.rmSync(barrier, { force: true });
  const runs = [];
  for (let i = 0; i < count; i++) {
    const t = tryCtl('task.create', { repo, harness: 'claude', title: `Listening ${++n}: needs you`, prompt: 'Write perm.txt.' });
    if (t) runs.push(t.run.id);
  }
  for (const id of runs) await waitStatus(id, /running/);
  await delay(1200); // past the start cue
  fs.writeFileSync(barrier, 'go');
  for (const id of runs) await waitStatus(id, /waiting_for_user/);
  note(`${runs.length === 1 ? 'an agent needs' : runs.length + ' agents need'} you  [${did(count === 1 ? 'n' : 't')}]`);
}

function stopAgents() {
  for (const run of s.ctl('state').runs.filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status))) tryCtl('run.interrupt', { run_id: run.id });
  note('fixture agents stopped');
}

const ask = question => new Promise(resolve => { process.stdout.write(question); process.stdin.once('data', d => { const k = String(d)[0]; say(k); resolve(k); }); });

async function marks() {
  tryCtl('audio.set', { enabled: true, track: 'reactor' });
  say('\nMarks. For each cue: r Right, n Needs work, p play again.');
  for (const cue of cues) {
    for (;;) {
      tryCtl('audio.preview', { key: cue.key });
      const k = await ask(`  ${cue.label.padEnd(20)} r / n / p ? `);
      if (k === 'r' || k === 'n') { record.marks[cue.key] = k === 'r' ? 'Right' : 'Needs work'; break; }
      await delay(600);
    }
  }
  say('\nWhat you heard in this session. y yes, n no.');
  const both = test => ['s', 'n'].every(key => record.events.some(e => e.key === key && test(e)));
  const any = test => record.events.some(e => e.key !== 't' && test(e));
  for (const [key, text, done, how] of [
    ['open', 'Start, completion and attention with VS Code open', both(e => e.enabled && e.vscode === 'open'), 'o, then s and n'],
    ['closed', 'Start, completion and attention with VS Code closed', both(e => e.enabled && e.vscode === 'closed'), 'quit the VS Code window, then s and n with Audio Mode on'],
    ['once', 'Two agents at the same moment made one attention cue', record.events.some(e => e.key === 't' && e.enabled), 't with Audio Mode on'],
    ['system', 'System voice spoke with an installed voice', any(e => e.enabled && e.track === 'system'), 'v, then s or n'],
    ['commander', 'Your own Commander folder played', any(e => e.enabled && e.track === 'commander' && e.commander_folder_set), 'import your folder in the VS Code window, p, then s or n'],
    ['off', 'With Audio Mode off, s and n were silent', both(e => !e.enabled), 'f, then s and n'],
  ]) {
    if (done) record.steps[key] = { y: 'yes', n: 'no' }[await ask(`  ${text}? y / n `)] || 'no';
    else { record.steps[key] = 'not tried'; say(`  ${text}: not done in this session (${how})`); }
  }
  const audio = s.ctl('audio.get');
  const bin = path.join(s.extensions, fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer')), 'bin', `overseerd-${process.platform}-${process.arch}`);
  Object.assign(record, {
    commit: cp.execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
    macos: cp.execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8' }).trim(), arch: os.arch(),
    daemon_sha256: require('crypto').createHash('sha256').update(fs.readFileSync(bin)).digest('hex'),
    pack: cues.map(c => ({ key: c.key, sha256: c.sha256 })),
    commander_folder_set: audio.commander_imported, // whether one was chosen; never its path or its files
  });
  fs.writeFileSync(path.join(s.evidence, recordFile), JSON.stringify(record, null, 2) + '\n');
  say(`\nWritten: ${path.relative(repoRoot, path.join(s.evidence, recordFile))}`);
}

(async () => {
  try {
    repo = makeRepo(path.join(s.root, 'listening'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      FIXTURE_MODE: 'permission', FIXTURE_PERMISSION_BARRIER: barrier, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_MODE,FIXTURE_PERMISSION_BARRIER' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await cdp.command('View: Show Overseer');
    cdp.close(); s.cdp = null; // from here on the window is the owner's
    cues = s.ctl('audio.get').manifest;
    say('\nAudio Mode listening session. Real sound: set the volume to what you work with.');
    say('A VS Code window is open with its own profile. Your own VS Code and daemon are untouched.');
    say('To listen with VS Code closed, quit that window (Cmd+Q in it) and keep using the keys here.');
    menu();
    process.stdin.setRawMode(true); process.stdin.resume();
    for (;;) {
      const k = await new Promise(resolve => process.stdin.once('data', d => resolve(String(d)[0])));
      if (busy) continue;
      busy = true;
      if (k === 'q' || k === '\u0003') break;
      else if (k === 'o') { if (tryCtl('audio.set', { enabled: true })) note('Audio Mode on'); }
      else if (k === 'f') { if (tryCtl('audio.set', { enabled: false })) note('Audio Mode off'); }
      else if (KEYS.includes(k) && cues[KEYS.indexOf(k)]) { const cue = cues[KEYS.indexOf(k)]; if (tryCtl('audio.preview', { key: cue.key })) note(`preview: ${cue.label}`); }
      else if (k === 's') await generic();
      else if (k === 'n') await needs(1);
      else if (k === 't') await needs(2);
      else if (k === 'v') { const voice = (s.ctl('audio.voices').find(v => v.name === 'Daniel') || s.ctl('audio.voices')[0])?.name || ''; if (tryCtl('audio.set', { track: 'system', voice })) note(`track: System voice (${voice})`); }
      else if (k === 'r') { if (tryCtl('audio.set', { track: 'reactor' })) note('track: Reactor'); }
      else if (k === 'p') { if (tryCtl('audio.set', { track: 'commander' })) note('track: your Commander folder'); }
      else if (k === 'x') stopAgents();
      else if (k === 'm') { await marks(); menu(); }
      else menu();
      busy = false;
    }
  } catch (error) {
    say('ERROR ' + (error.stack || error.message));
  } finally {
    try { process.stdin.setRawMode(false); } catch {}
    try { stopAgents(); } catch {}
    try { s.child?.kill('SIGTERM'); } catch {}
    await s.quit();
    s.stopDaemon();
    fs.rmSync(s.root, { recursive: true, force: true });
    if (!fs.readdirSync(s.evidence).some(f => /^marks.*\.json$/.test(f))) fs.rmSync(s.evidence, { recursive: true, force: true });
    say('Session closed: VS Code window, agents and daemon stopped.');
    process.exit(0);
  }
})();
