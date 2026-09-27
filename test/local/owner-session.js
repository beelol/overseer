#!/usr/bin/env node
// The owner's own offline session (AC-97): an isolated VS Code (its own profile, extensions folder
// and OVERSEER_HOME, so the owner's VS Code, daemon, logins and settings are untouched) with this
// branch's VSIX, and afterwards the daemon's own record of what happened.
//
//   node test/local/owner-session.js check            what is in place (VSIX, VS Code, Ollama, the model)
//   node test/local/owner-session.js start [folder]   installs the VSIX into the isolated profile and opens
//                                                     VS Code on the folder (default: a scratch repo); prints the steps
//   node test/local/owner-session.js report           afterwards: writes the daemon's record of the session to
//                                                     docs/verification/evidence/ac-97/ (connection changes, the
//                                                     handoff, the run tree, the review), redacted
//
// The isolated folders live under the temporary directory and are kept between the two commands.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const http = require('http');
const cp = require('child_process');

const root = path.resolve(__dirname, '../..');
const CODE = process.env.OVERSEER_CODE || '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code';
const base = path.join(os.tmpdir(), 'overseer-owner-session');
const profile = path.join(base, 'profile'), extensions = path.join(base, 'extensions'), home = path.join(base, 'overseer-home');
const MODEL = 'qwen3-coder:30b';
const clock = ms => new Date(ms).toISOString().slice(11, 19);
const redact = s => s.split(base).join('/OWNER_SESSION').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/').replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, 'EMAIL');
const latestVsix = () => { const dir = path.join(root, 'extension'); const v = fs.readdirSync(dir).filter(f => f.endsWith('.vsix')).sort().pop(); return v ? path.join(dir, v) : null; };
const get = url => new Promise(resolve => { const r = http.get(url, { timeout: 3000 }, res => { let b = ''; res.on('data', d => b += d); res.on('end', () => { try { resolve(JSON.parse(b)); } catch { resolve(null); } }); }); r.on('error', () => resolve(null)); r.on('timeout', () => { r.destroy(); resolve(null); }); });

async function check() {
  const vsix = latestVsix();
  const lines = [];
  lines.push(vsix ? `VSIX: ${path.relative(root, vsix)} (built ${fs.statSync(vsix).mtime.toISOString().slice(0, 16).replace('T', ' ')})` : 'VSIX: missing — run node extension/scripts/package.js');
  lines.push(fs.existsSync(CODE) ? `VS Code: ${CODE}` : `VS Code: not found at ${CODE} (set OVERSEER_CODE)`);
  const tags = await get('http://127.0.0.1:11434/api/tags');
  const installed = tags ? (tags.models || []).map(m => m.name) : [];
  lines.push(tags ? `Ollama: running; ${installed.length} models` : 'Ollama: not answering on 127.0.0.1:11434 — start it, or let Overseer start its own (continuity.allowOllamaInstall)');
  lines.push(installed.some(n => n.startsWith(MODEL)) ? `${MODEL}: installed (the verified model; ${installed.filter(n => n.startsWith('overseer/')).length} Overseer context variants)` : `${MODEL}: not installed — the local transition needs it (\`ollama pull ${MODEL}\`, 18 GB, or allow downloads in Overseer)`);
  lines.push(`memory: ${(os.totalmem() / 2 ** 30).toFixed(0)} GiB total; the 30b model at a 64k context takes about 24 GiB, and the daemon's own guard measures what is free when it picks`);
  lines.push(`session folders: ${base}${fs.existsSync(home) ? ' (present)' : ' (not created yet)'}`);
  for (const l of lines) console.log(l);
  return !!vsix && fs.existsSync(CODE) && !!tags && installed.some(n => n.startsWith(MODEL));
}

function steps(folder) {
  return `
The steps (about ten minutes; Wi-Fi is turned off and on by you):
  1. In the new window, open Overseer (the activity bar) → Agents → New Agent. Choose Codex or Claude Code
     and a small task, for example: "Create notes/offline.md with three short lines about this repository,
     then reply done." Start it.
  2. As soon as it is running, turn Wi-Fi off. Within about two minutes the agent's chat shows
     "Transitioning to ${MODEL} (local, Ollama) because you've disconnected. Work continues in the same
     worktree." and a new agent appears under the task. Screenshot the chat and the Agents view (⇧⌘4).
  3. Let the local agent finish (its status reads completed). Open the task's review (Changes) and
     screenshot it.
  4. Turn Wi-Fi on. The local agent's chat offers "Switch back to …": click it. The original harness
     continues in the same worktree; screenshot that announcement too.
  5. Quit that VS Code window (⌘Q), then run:  node test/local/owner-session.js report
     Copy the screenshots to /private/tmp/ac-97/ (the session's tools cannot read ~/Desktop or ~/Downloads)
     and say in the chat that it is done, with the date.
Folder: ${folder}
Nothing here touches your own VS Code, daemon, logins or settings.`;
}

async function start(folder) {
  if (!(await check())) { console.log('\nFix the lines above first.'); process.exit(2); }
  fs.mkdirSync(path.join(profile, 'User'), { recursive: true }); fs.mkdirSync(extensions, { recursive: true }); fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(path.join(profile, 'User/settings.json'), JSON.stringify({
    'telemetry.telemetryLevel': 'off', 'extensions.autoUpdate': false, 'extensions.autoCheckUpdates': false, 'update.mode': 'none',
    'workbench.startupEditor': 'none', 'security.workspace.trust.enabled': false, 'window.restoreWindows': 'none', 'git.openRepositoryInParentFolders': 'always',
  }, null, 2));
  if (!folder) {
    folder = path.join(base, 'repo');
    if (!fs.existsSync(path.join(folder, '.git'))) {
      fs.mkdirSync(folder, { recursive: true });
      const git = (...a) => cp.execFileSync('git', a, { cwd: folder, encoding: 'utf8' });
      git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Owner'); git('config', 'user.email', 'owner@example.invalid'); git('config', 'commit.gpgsign', 'false');
      fs.writeFileSync(path.join(folder, 'README.md'), '# Offline session\n\nA scratch repository for the owner\'s offline session (AC-97).\n');
      git('add', '.'); git('commit', '-q', '-m', 'base');
    }
  }
  const vsix = latestVsix();
  console.log('\n' + cp.execFileSync(CODE, ['--user-data-dir', profile, '--extensions-dir', extensions, '--install-extension', vsix, '--force'], { encoding: 'utf8' }).trim());
  const child = cp.spawn(CODE, ['--new-window', '--user-data-dir', profile, '--extensions-dir', extensions, '--skip-welcome', '--skip-release-notes', '--disable-workspace-trust', folder],
    { env: { ...process.env, OVERSEER_HOME: home }, stdio: 'ignore', detached: true });
  child.unref();
  console.log(steps(folder));
}

function daemonBin() {
  const ext = fs.existsSync(extensions) && fs.readdirSync(extensions).find(d => d.startsWith('beelol.overseer'));
  if (!ext) return null;
  return path.join(extensions, ext, 'bin', `overseerd-${process.platform}-${process.arch}`);
}

async function report() {
  const bin = daemonBin();
  if (!bin || !fs.existsSync(home)) { console.log(`No session yet: run  node test/local/owner-session.js start  first (looked in ${base}).`); process.exit(2); }
  const env = { ...process.env, OVERSEER_HOME: home };
  const ctl = (method, params) => {
    const r = cp.spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 20000, maxBuffer: 64 * 1024 * 1024 });
    const msg = JSON.parse((r.stdout || '').split('\n')[0] || '{"error":{"message":"no answer"}}');
    return msg.error ? { error: msg.error.message } : msg.result;
  };
  let started = null;
  if (ctl('hello').error) {
    started = cp.spawn(bin, ['serve'], { env, stdio: 'ignore' });
    for (let i = 0; i < 100 && ctl('hello').error; i++) await new Promise(r => setTimeout(r, 100));
  }
  const lines = [], transcript = [];
  const say = l => { console.log(l); lines.push(l); };
  try {
    say(`overseerd ${ctl('hello').version}; the isolated session at ${base}; read on ${new Date().toISOString().slice(0, 16).replace('T', ' ')}`);
    const all = (ctl('events.list', { limit: 5000 }) || {}).events || [];
    say('\n== Connection');
    for (const e of all.filter(e => e.kind === 'connection')) { say(`${clock(e.ts)}  ${e.payload.status.state} (${e.payload.status.reason}); the system said: ${e.payload.status.system.detail}`); transcript.push(e); }
    const st = ctl('state') || {};
    const runs = st.runs || [], tasks = st.tasks || [], workspaces = st.workspaces || [];
    const handoffs = (ctl('continuity.handoffs') || {}).handoffs || [];
    if (!runs.length) { say('\nNo agent has run in this session yet; nothing written.'); return; }
    say('\n== Runs');
    for (const task of tasks) {
      say(`task "${task.title}"`);
      for (const r of runs.filter(r => r.task_id === task.id && !r.parent_run_id).sort((a, b) => a.created_ms - b.created_ms)) {
        say(`  ${clock(r.created_ms)}  ${r.id}  ${r.harness}${r.model ? ' ' + r.model : ''}  ${r.status}${r.exit_reason ? '  — ' + r.exit_reason : ''}`);
        const ev = (ctl('events.list', { run_id: r.id, limit: 5000 }) || {}).events || [];
        for (const e of ev) {
          const keep = ['status', 'handoff', 'local_model', 'local_load', 'stall', 'retry', 'turn_started', 'turn_done', 'error'].includes(e.kind) || (e.kind === 'output' && e.payload.role === 'system');
          if (keep) transcript.push(e);
          if (e.kind === 'output' && e.payload.role === 'system') say(`      ${clock(e.ts)}  said: ${String(e.payload.text).replace(/\s+/g, ' ').slice(0, 300)}`);
          else if (['handoff', 'local_model', 'stall', 'retry'].includes(e.kind)) say(`      ${clock(e.ts)}  ${e.kind}: ${JSON.stringify(e.payload).slice(0, 300)}`);
        }
        const ws = workspaces.find(w => w.id === r.workspace_id);
        if (ws) { const ch = ctl('workspace.changes', { workspace_id: ws.id }); say(`      review: ${ch && !ch.error ? (ch.names || []).join(', ') || 'no changes' : 'unavailable'} (${ws.path})`); }
      }
    }
    say('\n== Handoffs');
    if (!handoffs.length) say('none');
    for (const h of handoffs) say(`${h.predecessor} → ${h.successor}${h.reason ? ' (' + h.reason + ')' : ''}${h.kind ? ' ' + h.kind : ''}`);
    const out = path.join(root, 'docs/verification/evidence/ac-97');
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, 'session.txt'), redact(lines.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, 'session-events.jsonl'), transcript.map(e => redact(JSON.stringify({ run: e.run_id, seq: e.seq, ts: e.ts, kind: e.kind, source: e.source, payload: e.payload }))).join('\n') + '\n');
    console.log(`\nwritten: docs/verification/evidence/ac-97/session.txt and session-events.jsonl (${transcript.length} events)`);
  } finally {
    if (started) { ctl('daemon.shutdown'); started.kill(); }
  }
}

const cmd = process.argv[2] || 'check';
(cmd === 'start' ? start(process.argv[3] && path.resolve(process.argv[3])) : cmd === 'report' ? report() : check().then(ok => process.exit(ok ? 0 : 2))).catch(e => { console.error(e.message); process.exit(2); });
