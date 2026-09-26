// AC-13 live isolation on the owner's daemon. Never signs out A or B; never prints tokens.
const cp = require('child_process'); const fs = require('fs'); const path = require('path'); const os = require('os');
const BIN = path.join(os.homedir(), '.vscode/extensions/beelol.overseer-0.1.0/bin/overseerd-darwin-arm64');
const HOME = path.join(os.homedir(), 'Library/Application Support/Overseer');
const env = { ...process.env, OVERSEER_HOME: HOME };
const ctl = (m, p = {}) => { const o = JSON.parse(cp.execFileSync(BIN, ['ctl', m, JSON.stringify(p)], { env, encoding: 'utf8' }).split('\n')[0]); if (o.error) throw new Error(o.error.message); return o.result; };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const A = 'p-f262c1bc4958', B = 'p-52fb6421edd2';
const ident = id => { const s = ctl('profile.status', { id }); return { logged_in: s.logged_in, method: s.method, plan: s.identity?.plan, account: (s.identity?.account_fingerprint || s.identity?.fingerprint || '').slice(0, 8), api_key: s.identity?.has_api_key || false }; };
const log = []; const note = (k, v) => { log.push({ t: Date.now(), k, v }); console.log(k, JSON.stringify(v)); };
(async () => {
  note('before', { A: ident(A), B: ident(B), desktop: ident('system-codex') });
  const repo = fs.mkdtempSync('/tmp/ovs-ac13-');
  const git = (...a) => cp.execFileSync('git', a, { cwd: repo, encoding: 'utf8' }).trim();
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Overseer Test'); git('config', 'user.email', 'overseer-test@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(repo, 'README.md'), '# AC-13 disposable repo\n'); git('add', '.'); git('commit', '-qm', 'base');
  // B does minimal live work while the disposable profile is exercised.
  const tb = ctl('task.create', { repo, harness: 'codex', profile_id: B, model: 'gpt-5.6-luna', title: 'AC-13 B minimal work', prompt: 'Run the shell command: sleep 25. Then create b.txt containing exactly one line: B still works. Then reply exactly: done' });
  for (let i = 0; i < 60 && ctl('state').runs.find(r => r.id === tb.run.id).status !== 'running'; i++) await sleep(500);
  note('B running', { run: tb.run.id, status: ctl('state').runs.find(r => r.id === tb.run.id).status });
  const c = ctl('account.create', { provider: 'openai', name: 'AC-13 disposable' }).account;
  const cDir = path.join(c.home, 'codex');
  note('C created', { id: c.id, codex_dir_exists: fs.existsSync(cDir), mode: (fs.statSync(cDir).mode & 0o777).toString(8), status: ident(c.id) });
  const login = ctl('profile.login_command', { id: c.id, device: true });
  note('C login command targets only C', { program: path.basename(login.program), args: login.args, codex_home_is_c: login.env.CODEX_HOME === cDir });
  note('C logout during B work', { result: ctl('profile.logout', { id: c.id }).exit, C: ident(c.id), B_mid: ident(B), A_mid: ident(A) });
  ctl('account.remove', { id: c.id });
  note('C removed', { folder_gone: !fs.existsSync(c.home), B: ident(B), A: ident(A) });
  let run; for (let i = 0; i < 240; i++) { run = ctl('state').runs.find(r => r.id === tb.run.id); if (!['queued', 'starting', 'running'].includes(run.status)) break; await sleep(1000); }
  const bfile = path.join(tb.workspace.path, 'b.txt');
  note('B finished', { status: run.status, reason: run.exit_reason, b_txt: fs.existsSync(bfile) && fs.readFileSync(bfile, 'utf8').trim() });
  // Restart the daemon (no runs active) and check identities again.
  const active = ctl('state').runs.filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status));
  if (active.length) { note('restart skipped: active runs', active.map(r => r.id)); }
  else {
    const pid0 = ctl('hello').pid;
    ctl('daemon.shutdown');
    for (let i = 0; i < 40; i++) { await sleep(500); try { if (ctl('hello').pid !== pid0) break; } catch {} }
    let hello; try { hello = ctl('hello'); } catch { cp.spawn(BIN, ['serve'], { env, detached: true, stdio: 'ignore' }).unref(); await sleep(2000); hello = ctl('hello'); }
    note('daemon restarted', { old_pid: pid0, new_pid: hello.pid, after: { A: ident(A), B: ident(B), desktop: ident('system-codex') }, B_run_after_restart: ctl('state').runs.find(r => r.id === tb.run.id).status });
  }
  // Leakage scan: actual token values from every Codex credential home, searched in Overseer's own files. Counts only.
  const homes = [path.join(HOME, 'profiles', A, 'codex'), path.join(HOME, 'profiles', B, 'codex'), path.join(os.homedir(), '.codex')];
  const secrets = [];
  for (const h of homes) { try { const j = JSON.parse(fs.readFileSync(path.join(h, 'auth.json'), 'utf8')); for (const k of ['access_token', 'refresh_token', 'id_token']) { const v = j.tokens?.[k]; if (typeof v === 'string' && v.length > 20) secrets.push(v.slice(-40)); } if (j.OPENAI_API_KEY) secrets.push(String(j.OPENAI_API_KEY).slice(-20)); } catch {} }
  const scanned = []; let hits = 0;
  const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) { if (p.startsWith(path.join(HOME, 'profiles')) || p.startsWith(path.join(HOME, 'worktrees'))) continue; walk(p); } else if (e.isFile() && fs.statSync(p).size < 200 * 1024 * 1024) { const buf = fs.readFileSync(p); scanned.push(p); for (const sct of secrets) if (buf.includes(sct)) hits++; } } };
  walk(HOME);
  const jwtish = scanned.filter(p => /eyJhbGciOi/.test(fs.readFileSync(p).toString('latin1'))).map(p => path.relative(HOME, p));
  note('leak scan', { secrets_checked: secrets.length, files_scanned: scanned.length, token_hits: hits, jwt_like_files: jwtish, scope: 'Overseer database, WAL, logs, run launch files and raw output segments (credential homes and worktrees excluded)' });
  fs.writeFileSync(process.argv[2], JSON.stringify(log, null, 2));
})().catch(e => { console.error(e); process.exit(1); });
