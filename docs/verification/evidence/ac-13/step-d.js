// AC-11/13 step d on the owner's daemon. Counts only; never prints tokens.
const cp = require('child_process'); const os = require('os');
const { ctl, ident, HOME, fs, path } = require('./ctl-helpers.js');
const BIN = path.join(os.homedir(), '.vscode/extensions/beelol.overseer-0.1.0/bin/overseerd-darwin-arm64');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const A = 'p-f262c1bc4958', B = 'p-52fb6421edd2', C = 'p-e4a877587734';
const out = {};
(async () => {
  const s = ctl('state'); const b = s.runs.find(r => r.id === 'r-c180b4e20206'); const ws = s.workspaces.find(w => w.id === b.workspace_id);
  out.B_run = { status: b.status, reason: b.exit_reason, b3_txt: fs.existsSync(ws.path + '/b3.txt') && fs.readFileSync(ws.path + '/b3.txt', 'utf8').trim() };
  const active = s.runs.filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status));
  if (active.length) { out.restart = { skipped: 'active runs', ids: active.map(r => r.id) }; }
  else {
    const pid0 = ctl('hello').pid; ctl('daemon.shutdown');
    let hello; for (let i = 0; i < 40; i++) { await sleep(500); try { hello = ctl('hello'); if (hello.pid !== pid0) break; } catch {} }
    if (!hello || hello.pid === pid0) { cp.spawn(BIN, ['serve'], { env: { ...process.env, OVERSEER_HOME: HOME }, detached: true, stdio: 'ignore' }).unref(); await sleep(2000); hello = ctl('hello'); }
    out.restart = { old_pid: pid0, new_pid: hello.pid };
  }
  out.after_restart = { A: ident(A), B: ident(B), desktop: ident('system-codex'), throwaway: ident(C) };
  // Leak scan: tails of real token values from every Codex credential home (incl. the throwaway), searched in Overseer's own files.
  const homes = [A, B, C].map(id => path.join(HOME, 'profiles', id, 'codex')).concat(path.join(os.homedir(), '.codex'));
  const secrets = [];
  for (const h of homes) { try { const j = JSON.parse(fs.readFileSync(path.join(h, 'auth.json'), 'utf8')); for (const k of ['access_token', 'refresh_token', 'id_token']) { const v = j.tokens?.[k]; if (typeof v === 'string' && v.length > 20) secrets.push(v.slice(-40)); } if (j.OPENAI_API_KEY) secrets.push(String(j.OPENAI_API_KEY).slice(-20)); } catch {} }
  const scanned = []; let hits = 0;
  const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) { if (p.startsWith(path.join(HOME, 'profiles')) || p.startsWith(path.join(HOME, 'worktrees'))) continue; walk(p); } else if (e.isFile() && fs.statSync(p).size < 200 * 1024 * 1024) { const buf = fs.readFileSync(p); scanned.push(p); for (const sct of secrets) if (buf.includes(sct)) hits++; } } };
  walk(HOME);
  const vsLogs = path.join(os.homedir(), 'Library/Application Support/Code/logs');
  let vsHits = 0, vsFiles = 0;
  for (const d of fs.readdirSync(vsLogs)) { const f = path.join(vsLogs, d); const stack = [f]; while (stack.length) { const x = stack.pop(); for (const e of fs.readdirSync(x, { withFileTypes: true })) { const p = path.join(x, e.name); if (e.isDirectory()) stack.push(p); else if (/beelol\.overseer/.test(p)) { vsFiles++; const buf = fs.readFileSync(p); for (const sct of secrets) if (buf.includes(sct)) vsHits++; } } } }
  const jwt = scanned.filter(p => /eyJhbGciOi/.test(fs.readFileSync(p).toString('latin1'))).length;
  out.leak_scan = { secrets_checked: secrets.length, overseer_files_scanned: scanned.length, token_hits: hits, jwt_like_files: jwt, vscode_overseer_log_files: vsFiles, vscode_log_hits: vsHits };
  // Remove the throwaway.
  const cHome = path.join(HOME, 'profiles', C);
  ctl('account.remove', { id: C });
  out.throwaway_removed = { folder_gone: !fs.existsSync(cHome), listed: ctl('account.list').accounts.some(a => a.id === C) };
  out.final = { A: ident(A), B: ident(B), desktop: ident('system-codex') };
  out.at = new Date().toISOString();
  console.log(JSON.stringify(out, null, 2));
})().catch(e => { console.error(e.message); process.exit(1); });
