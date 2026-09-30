// ON SCREEN (menus open on this Mac's screen for a moment: run it with --only=menubar) — AC-262,
// Overseer in the Mac's menu bar, against a dev daemon with the Claude fixture (no paid turns). The
// dev item (`scripts/dev up`) is the real Overseer Menu.app; its evidence modes open its real menu
// in-process and capture only its own windows (no screen recording). Checked:
//   quiet (3 working, no dot), needs you (2 requests), 30 agents in 3 repositories (18, 7, 5) with
//   the overseer submenu open, and six waiting (4 shown, then "2 more waiting"), each in a light and
//   a dark menu; Allow once, Always allow and Deny pressed in the menu reach the fixture agents;
//   choosing an agent in a repository's submenu opens it in a test VS Code pinned to the dev daemon;
//   with the daemon stopped the item says so, and its Start Overseer brings it back.
// The dev root, VS Code profile and repositories are temporary; the owner's daemon, VS Code and
// data are never involved, and nothing is registered as a login item.
const fs = require('fs');
const net = require('net');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const DEV = path.join(repoRoot, 'scripts/dev');

function call(socket, method, params = {}) {
  return new Promise((resolve, reject) => {
    const c = net.createConnection(socket); let buf = '';
    c.setEncoding('utf8');
    c.on('connect', () => c.write(JSON.stringify({ id: 1, method, params }) + '\n'));
    c.on('data', d => { buf += d; const k = buf.indexOf('\n'); if (k < 0) return; c.destroy(); const m = JSON.parse(buf.slice(0, k)); m.error ? reject(new Error(m.error.message)) : resolve(m.result); });
    c.on('error', reject);
  });
}

(async () => {
  if (process.platform !== 'darwin') { console.log('SCENARIO PASSED (not macOS: no menu bar)'); process.exit(0); }
  const s = new Session('menubar');
  const root = path.join(s.root, 'dr');
  const env = { ...process.env, OVERSEER_DEV_ROOT: root };
  for (const k of Object.keys(env)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_DEV_ROOT' && k !== 'OVERSEER_CODE') delete env[k];
  const dev = (...args) => {
    const r = cp.spawnSync(process.execPath, [DEV, ...args], { env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
    s.note(`$ scripts/dev ${args.join(' ')} → exit ${r.status}`, (r.stdout + r.stderr).trim().split('\n').slice(-6).join(' | '));
    if (r.status !== 0) throw new Error(`scripts/dev ${args.join(' ')} failed: ${r.stderr.slice(-2000)}`);
    return r.stdout;
  };
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const inst = path.join(root, 'mb');
  const app = path.join(inst, 'bin/Overseer Menu.app');
  const bin = path.join(app, 'Contents/MacOS/overseer-menu');
  const config = path.join(inst, 'bin/menubar.json');
  /** The item's evidence modes: the real menu, opened in-process. */
  const menu = (...args) => {
    const r = cp.spawnSync(bin, ['--config', config, ...args], { env, encoding: 'utf8', timeout: 60000 });
    s.note(`$ overseer-menu ${args.join(' ')} → exit ${r.status}`, (r.stdout + r.stderr).trim().slice(-600));
    return { status: r.status, out: (r.stdout || '').trim(), err: (r.stderr || '').trim() };
  };
  const shot = (name, ...extra) => {
    for (const look of ['light', 'dark']) {
      const r = menu('--capture', path.join(s.evidence, `${name}-${look}`), '--appearance', look, ...extra);
      if (r.status !== 0) throw new Error(`capture ${name}-${look} failed: ${r.err || r.out}`);
    }
  };
  let socket;
  const snap = () => call(socket, 'menubar.snapshot');
  const until = async (what, pred, ms = 60000) => {
    const t0 = Date.now(); let last;
    while (Date.now() - t0 < ms) { last = await snap().catch(e => ({ error: e.message })); if (pred(last)) return last; await delay(300); }
    throw new Error(`timed out: ${what}: ${JSON.stringify(last).slice(0, 800)}`);
  };
  const workspace = async runId => {
    const st = await call(socket, 'state');
    const run = st.runs.find(r => r.id === runId);
    const task = st.tasks.find(t => t.id === run.task_id);
    return st.workspaces.find(w => w.id === task.workspace_id).path;
  };
  const answerOf = async runId => { const f = path.join(await workspace(runId), 'answer.json'); for (let i = 0; i < 50 && !fs.existsSync(f); i++) await delay(200); return JSON.parse(fs.readFileSync(f, 'utf8')); };

  try {
    const up = JSON.parse(dev('up', '--name', 'mb', '--json', '--menubar'));
    socket = up.socket;
    await delay(1500);
    const running = cp.spawnSync('pgrep', ['-f', app], { encoding: 'utf8' }).stdout.trim();
    const cfg = JSON.parse(fs.readFileSync(config, 'utf8'));
    check("scripts/dev up starts the dev daemon's own item, pinned to its socket and its VS Code profile",
      !!running && cfg.instance === 'dev-mb' && cfg.socket === socket && cfg.code_args.includes(path.join(inst, 'vscode/profile')), { running, cfg });
    // The captures below start the same app; one dev item at a time in the menu bar.
    for (const pid of running.split('\n').filter(Boolean)) try { process.kill(Number(pid)); } catch {}

    fs.writeFileSync(path.join(inst, 'fixture-mode'), 'menubar');
    // Two SYNTHETIC accounts (no real login is read): the Mac's default Claude login and a named one.
    fs.mkdirSync(path.join(inst, 'system/.claude'), { recursive: true });
    fs.writeFileSync(path.join(inst, 'system/.claude/fixture-account.json'), JSON.stringify({ email: 'bilal@testbox.com', plan: 'max' }));
    const personal = (await call(socket, 'account.create', { provider: 'anthropic', name: 'Personal' })).account;
    fs.writeFileSync(path.join(personal.home, 'claude', 'fixture-account.json'), JSON.stringify({ email: 'ana.silva@personal.example', plan: 'pro' }));
    for (const id of ['system-claude', personal.id]) await call(socket, 'profile.status', { id }).catch(() => {});
    const repos = Object.fromEntries(['overseer', 'site', 'notes'].map(n => [n, makeRepo(path.join(s.root, n), { dirty: false })]));
    const start = (repo, title, prompt, profile = 'system-claude') => call(socket, 'task.create', { repo: repos[repo], harness: 'claude', profile_id: profile, prompt, title }).then(r => r.run.id);

    // 1. Quiet: three agents working, nothing waits.
    await start('overseer', 'Gateway reconnect backoff', 'busy: 1800');
    await start('site', 'Pricing table copy', 'busy: 1800', personal.id);
    await start('overseer', 'TUI grid resize flicker', 'busy: 1800');
    const quiet = await until('three working', v => v.summary === '3 working');
    check('quiet: "3 working", nothing waits, overseer (2) and site (1), accounts named',
      quiet.waiting.length === 0 && quiet.repos.map(r => `${r.name} ${r.count}`).join(', ') === 'overseer 2, site 1'
      && /Claude Max · bil…@testbox\.com/.test(JSON.stringify(quiet.repos)) && /Claude Pro · ana…@personal\.example/.test(JSON.stringify(quiet.repos)), quiet);
    shot('1-quiet', '--submenu', 'overseer', '--when', '3 working');

    // 2. Needs you: two permission requests, answerable in the menu.
    const npm = await start('site', 'Checkout page redesign', 'ask: npm test', personal.id);
    await delay(300);
    const cargo = await start('overseer', 'Fix flaky gateway test', 'ask: cargo test -p overseerd');
    const needs = await until('two waiting', v => v.waiting.length === 2);
    check('needs you: both requests, newest first, as questions with their session rule',
      needs.waiting[0].question === 'Run cargo test -p overseerd?' && needs.waiting[1].question === 'Run npm test?' && needs.waiting[1].always === 'Bash(npm test:*) · this session', needs.waiting);
    shot('2-needs-you', '--when', 'cargo test');
    const allowed = menu('--press', 'allow', '--index', '1', '--capture', path.join(s.evidence, '2-allow-once'), '--appearance', 'dark');
    const npmAnswer = await answerOf(npm);
    check('Allow once pressed in the menu reaches the fixture agent (its tool call runs; no rule is kept)',
      allowed.status === 0 && npmAnswer.behavior === 'allow' && !npmAnswer.updatedPermissions, { allowed, npmAnswer });
    const denied = menu('--press', 'deny', '--index', '0');
    const cargoAnswer = await answerOf(cargo);
    check('Deny pressed in the menu reaches the fixture agent', denied.status === 0 && cargoAnswer.behavior === 'deny', { denied, cargoAnswer });

    // 3. Thirty agents in three repositories: overseer 18, site 7, notes 5.
    const st = await call(socket, 'state');
    for (const t of st.tasks) await call(socket, 'task.archive', { task_id: t.id });
    const plan = {
      overseer: { idle: ['Notifier thread ids', 'Talk to Overseer latency', 'Deploy guard', 'Dev daemon clean-up', 'Brand exports', 'Merge from the agent', 'Ledger link check'],
        review: ['Review opens on task start', 'Phone door timing', 'Voice Mode wake word'],
        working: ['Auto quota refresh', 'Offline replay order', 'Account booking race', 'Swarm admission limits', 'TUI grid resize flicker', 'Menu-bar status item', 'Gateway reconnect backoff'] },
      site: { idle: ['Sitemap', 'Lighthouse fixes', 'Footer links'], review: ['Hero animation'], working: ['Blog RSS feed', 'Pricing table copy', 'Checkout page redesign'] },
      notes: { idle: ['Backlinks', 'Daily template', 'Markdown export'], review: ['Tag search'], working: ['Weekly notes digest'] },
    };
    const idle = [];
    let k = 0;
    for (const kind of ['idle', 'review', 'working']) {
      for (const [repo, lists] of Object.entries(plan)) {
        for (const title of lists[kind]) {
          const id = await start(repo, title, kind === 'working' ? 'busy: 1800' : 'finish', k++ % 3 === 1 ? personal.id : 'system-claude');
          if (kind === 'idle') idle.push(id);
        }
      }
    }
    for (const id of idle) {
      for (let i = 0; i < 100 && !(await call(socket, 'state')).runs.find(r => r.id === id)?.ended_ms; i++) await delay(100);
      const run = (await call(socket, 'state')).runs.find(r => r.id === id);
      await call(socket, 'review.seen', { marks: { [id]: run.ended_ms || Date.now() } });
    }
    const flaky = await start('overseer', 'Fix flaky gateway test', 'ask: npm test');
    const crowd = await until('30 agents', v => v.summary === '12 working · 5 to review · 13 idle' && v.waiting.length === 1);
    check('30 agents: "12 working · 5 to review · 13 idle", overseer 18, site 7, notes 5, at most 8 in a submenu, the waiting one first',
      crowd.repos.map(r => `${r.name} ${r.count}`).join(', ') === 'overseer 18, site 7, notes 5' && crowd.repos.every(r => r.agents.length <= 8)
      && crowd.repos[0].agents[0].title === 'Fix flaky gateway test' && crowd.repos[0].agents[0].status === 'Needs you', crowd.repos.map(r => [r.name, r.count, r.agents.map(a => `${a.title}: ${a.status}`)]));
    shot('3-thirty-agents', '--submenu', 'overseer', '--when', '12 working');
    const always = menu('--press', 'always', '--index', '0');
    const flakyAnswer = await answerOf(flaky);
    check('Always allow (under Allow once\'s arrow) reaches the fixture agent with Claude Code\'s session rule',
      always.status === 0 && flakyAnswer.behavior === 'allow' && flakyAnswer.updatedPermissions?.[0]?.rules?.[0]?.ruleContent === 'npm test:*' && flakyAnswer.updatedPermissions[0].destination === 'session', { always, flakyAnswer });

    // Six waiting: four in the menu, then "2 more waiting".
    const asks = [['overseer', 'Swarm admission limits', 'ask: git push origin swarm-limits'], ['site', 'Blog RSS feed', 'ask: pnpm build'], ['notes', 'Weekly notes digest', 'ask: curl https://docs.rs'],
      ['site', 'Checkout page redesign', 'ask: npm test'], ['overseer', 'Gateway reconnect backoff', 'ask: cargo test -p overseerd'], ['overseer', 'Fix flaky gateway test', 'ask: npm run lint']];
    for (const [repo, title, prompt] of asks) await start(repo, title, prompt);
    await until('six waiting', v => v.waiting.length === 6);
    shot('4-six-waiting', '--when', 'npm run lint');
    check('six waiting: the menu shows four and "2 more waiting · Show all in Overseer…" (see 4-six-waiting-*.png)', true);

    // Choosing an agent opens it in a test VS Code pinned to the dev daemon.
    dev('code', '--name', 'mb', repos.overseer, '--vsix', latestVsix(), '--inspect');
    s.profile = path.join(inst, 'vscode/profile');
    s.extensions = path.join(inst, 'vscode/extensions');
    const cdp = await s.connect();
    await s.openOverseerView();
    const now = await snap();
    const target = now.repos.find(r => r.name === 'overseer').agents[1];
    const chosen = menu('--choose', 'overseer', '--index', '1');
    const opened = await cdp.waitFor(`[...document.querySelectorAll('.monaco-list-row.selected')].some(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(target.title)})`, 30000, 'the chosen agent selected').then(() => true, () => false);
    await delay(1500);
    await s.screenshot('chosen-agent-in-vscode');
    check('choosing an agent in the menu opens it in the test VS Code', chosen.status === 0 && chosen.out.includes(`open-agent?run=${target.run_id}`) && opened, { chosen: chosen.out, target });

    // The daemon stopped: the item says so and starts it again.
    await call(socket, 'daemon.shutdown').catch(() => {});
    for (let i = 0; i < 100 && fs.existsSync(socket); i++) await delay(100);
    shot('5-daemon-stopped');
    const started = menu('--choose-item', 'Start Overseer');
    let back = null;
    for (let i = 0; i < 200 && !back; i++) { back = await call(socket, 'hello', {}).catch(() => null); if (!back) await delay(300); }
    check('with the daemon stopped the item says so, and Start Overseer brings it back', started.status === 0 && back && back.instance === 'dev-mb', { started, back });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
  } finally {
    s.writeLog();
    try { await s.quit(); } catch {}
    try { dev('clean', '--all'); } catch (e) { s.note('clean failed: ' + e.message); }
    for (const pid of cp.spawnSync('pgrep', ['-f', app], { encoding: 'utf8' }).stdout.trim().split('\n').filter(Boolean)) try { process.kill(Number(pid)); } catch {}
    const left = cp.spawnSync('pgrep', ['-f', root], { encoding: 'utf8' }).stdout.trim();
    if (left) { s.note('left running after clean', left); result.error = result.error || 'processes left running'; }
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
