// Listening session for AC-145 (Audio Mode by ear). It plays real sound.
//
//     node extension/scripts/package.js && node test/ui/listen-audio.js
//
// Opens VS Code with its own profile and its own Overseer home, with the packaged VSIX installed,
// so the owner's own VS Code, daemon and agents are not touched. Agents are fixtures: no account
// is used and no tokens are spent. Keys typed in this terminal make the events to listen for, and
// they keep working after that VS Code window is closed. `m` asks for the marks and writes the
// record to docs/verification/evidence/ui/audio-listening/.
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const s = new Session('audio-listening');
const barrier = path.join(s.root, 'release-permissions');
const say = text => process.stdout.write(text + '\n');
const tryCtl = (method, params) => { try { return s.ctl(method, params); } catch (error) { say(`  ${method}: ${error.message}`); return null; } };
const state = id => s.ctl('state').runs.find(r => r.id === id);
const waitStatus = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 200) { if (re.test(state(id)?.status || '')) return true; await delay(200); } return false; };
const record = { date: new Date().toISOString().slice(0, 10), heard: [], marks: {}, steps: {} };
const note = text => { record.heard.push(`${new Date().toISOString().slice(11, 19)} ${text}`); say(`  ${text}`); };
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
  note('an agent started; it completes in 3 s');
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
  note(`${runs.length === 1 ? 'an agent needs' : runs.length + ' agents need'} you`);
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
  say('\nWhat you heard. y yes, n no, - not tried.');
  for (const [key, text] of [
    ['open', 'Start, completion and attention with VS Code open'],
    ['closed', 'Start, completion and attention with VS Code closed'],
    ['once', 'Two agents at the same moment made one attention cue'],
    ['system', 'System voice spoke with an installed voice'],
    ['commander', 'Your own Commander folder played'],
    ['off', 'With Audio Mode off, s and n were silent'],
  ]) record.steps[key] = { y: 'yes', n: 'no' }[await ask(`  ${text}? y / n / - `)] || 'not tried';
  const audio = s.ctl('audio.get');
  const bin = path.join(s.extensions, fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer')), 'bin', `overseerd-${process.platform}-${process.arch}`);
  Object.assign(record, {
    commit: cp.execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
    macos: cp.execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8' }).trim(), arch: os.arch(),
    daemon_sha256: require('crypto').createHash('sha256').update(fs.readFileSync(bin)).digest('hex'),
    pack: cues.map(c => ({ key: c.key, sha256: c.sha256 })),
    commander_folder_set: audio.commander_imported, // whether one was chosen; never its path or its files
  });
  fs.writeFileSync(path.join(s.evidence, 'marks.json'), JSON.stringify(record, null, 2) + '\n');
  say(`\nWritten: ${path.relative(repoRoot, path.join(s.evidence, 'marks.json'))}`);
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
    if (!fs.existsSync(path.join(s.evidence, 'marks.json'))) fs.rmSync(s.evidence, { recursive: true, force: true });
    say('Session closed: VS Code window, agents and daemon stopped.');
    process.exit(0);
  }
})();
