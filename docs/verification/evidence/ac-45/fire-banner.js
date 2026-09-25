const cp = require('child_process'), fs = require('fs'), net = require('net'), path = require('path');
const BIN = path.resolve('target/release/overseerd');
const home = fs.mkdtempSync('/tmp/ovs-banner-');
const env = { ...process.env, OVERSEER_HOME: home, OVERSEER_BACKGROUND_NOTICE_MS: '1500' };
const ctl = (m, p = {}) => JSON.parse(cp.execFileSync(BIN, ['ctl', m, JSON.stringify(p)], { env, encoding: 'utf8' }).split('\n')[0]).result;
const sleep = ms => new Promise(r => setTimeout(r, ms));
(async () => {
  const d = cp.spawn(BIN, ['serve'], { env, stdio: 'ignore' });
  for (let i = 0; i < 50; i++) { try { ctl('hello'); break; } catch { await sleep(100); } }
  const repo = path.join(home, 'demo'); fs.mkdirSync(repo);
  const git = (...a) => cp.execFileSync('git', a, { cwd: repo });
  git('init', '-q', '-b', 'main'); git('-c', 'user.name=t', '-c', 'user.email=t@example.invalid', 'commit', '-q', '--allow-empty', '-m', 'base');
  const t = ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 60'], prompt: '', title: 'banner demo agent' });
  for (let i = 0; i < 30 && ctl('state').runs.find(r => r.id === t.run.id).status !== 'running'; i++) await sleep(200);
  const sock = cp.execFileSync(BIN, ['socket-path'], { env, encoding: 'utf8' }).trim();
  const c = net.createConnection(sock); await new Promise(r => c.on('connect', r));
  c.write(JSON.stringify({ id: 1, method: 'hello', params: { client: 'vscode' } }) + '\n'); await sleep(500);
  c.destroy(); // "the last VS Code window closed"
  await sleep(3000);
  const notice = ctl('events.list', { after: 0, limit: 5000 }).events.find(e => e.kind === 'background_notice');
  console.log(JSON.stringify({ at: new Date().toISOString(), title: notice?.payload.title, body: notice?.payload.body, via: notice?.payload.delivered_via }, null, 2));
  const shot = process.argv[2]; const cap = cp.spawnSync('/usr/sbin/screencapture', ['-x', shot]); console.log('screencapture exit', cap.status);
  ctl('run.interrupt', { run_id: t.run.id }); await sleep(1000); try { ctl('daemon.shutdown'); } catch {} d.kill(); fs.rmSync(home, { recursive: true, force: true });
})();
