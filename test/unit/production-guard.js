// AC-212: the installed (production) extension never points at a dev version. With dev variables
// leaked into its environment it still reaches the standard socket; it never runs a daemon binary
// marked dev; it refuses a daemon that reports a dev instance. An extension loaded from another
// extensions folder (the UI test harness, dev profiles) keeps honouring OVERSEER_HOME/SOCKET.
// Fake daemons on temporary Unix sockets; nothing real is started. Run: node test/unit/production-guard.js
const assert = require('assert');
const fs = require('fs');
const net = require('net');
const os = require('os');
const path = require('path');
const { DaemonClient, isProductionInstall, productionEnv, devMarker, DEV_MARKER } = require(path.resolve(__dirname, '../../extension/src/daemon-client.js'));

const tmp = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-pg-'));
const servers = [];
let failures = 0;
const check = async (name, fn) => { try { await fn(); console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.stack || e.message); } };
const delay = ms => new Promise(r => setTimeout(r, ms));

/** A fake overseerd socket answering hello with `instance`; counts connections. */
function fakeDaemon(file, instance = null) {
  const s = { connections: 0, file };
  s.server = net.createServer(conn => {
    s.connections++;
    let buf = '';
    conn.setEncoding('utf8');
    conn.on('data', d => {
      buf += d; let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const msg = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
        const result = msg.method === 'hello' ? { protocol: 1, pid: 1, instance } : {};
        conn.write(JSON.stringify({ id: msg.id, result }) + '\n');
      }
    });
    conn.on('error', () => {});
  });
  servers.push(s.server);
  return new Promise(r => s.server.listen(file, () => r(s)));
}

/** A fake overseerd binary: `socket-path` prints $OVERSEER_SOCKET, else the standard socket;
 *  every run is logged (so a test can tell whether it was run at all). */
function fakeBinary(dir, standardSocket) {
  fs.mkdirSync(dir, { recursive: true });
  const bin = path.join(dir, 'overseerd');
  fs.writeFileSync(bin, `#!/bin/sh\necho "$1" >> "${dir}/runs.log"\nif [ "$1" = socket-path ]; then echo "\${OVERSEER_SOCKET:-${standardSocket}}"; fi\n`);
  fs.chmodSync(bin, 0o755);
  return bin;
}
const runs = bin => { try { return fs.readFileSync(path.join(path.dirname(bin), 'runs.log'), 'utf8').split('\n').filter(Boolean); } catch { return []; } };

(async () => {
  const std = await fakeDaemon(path.join(tmp, 'std.sock'));
  const dev = await fakeDaemon(path.join(tmp, 'dev.sock'), 'dev-a');
  const leaked = { ...process.env, OVERSEER_HOME: path.join(tmp, 'dev-home'), OVERSEER_SOCKET: dev.file, OVERSEER_INSTANCE: 'dev-a' };

  await check('production is the standard extensions folder, and only that', () => {
    const home = path.join(tmp, 'home');
    const installed = path.join(home, '.vscode/extensions/beelol.overseer-0.1.0');
    fs.mkdirSync(installed, { recursive: true });
    assert.strictEqual(isProductionInstall(installed, home), true);
    assert.strictEqual(isProductionInstall(path.join(home, '.vscode-insiders/extensions/beelol.overseer-0.1.0'), home), true);
    assert.strictEqual(isProductionInstall(path.join(tmp, 'ovs-ui-x/extensions/beelol.overseer-0.1.0'), home), false, 'the UI harness');
    assert.strictEqual(isProductionInstall(path.join(tmp, 'dev/a/vscode/extensions/beelol.overseer-0.1.0'), home), false, 'a dev profile');
  });

  await check('production drops OVERSEER_HOME, OVERSEER_SOCKET and OVERSEER_INSTANCE', () => {
    const env = productionEnv({ ...leaked, PATH: '/usr/bin' });
    assert.deepStrictEqual(['OVERSEER_HOME', 'OVERSEER_SOCKET', 'OVERSEER_INSTANCE'].filter(k => k in env), []);
    assert.strictEqual(env.PATH, '/usr/bin');
  });

  await check('production with dev variables leaked in reaches the standard socket, never the dev one', async () => {
    const bin = fakeBinary(path.join(tmp, 'p1'), std.file);
    const c = new DaemonClient(bin, () => {}, { production: true, env: leaked });
    await c.start();
    assert.strictEqual(c.socketPath(), std.file);
    assert.strictEqual(c.connected, true);
    assert.strictEqual(dev.connections, 0, 'the dev daemon saw no connection');
    assert.deepStrictEqual(runs(bin), ['socket-path'], 'no daemon was started');
    c.dispose();
  });

  await check('an isolated (non-production) extension keeps honouring OVERSEER_SOCKET', async () => {
    const bin = fakeBinary(path.join(tmp, 'p2'), std.file);
    const c = new DaemonClient(bin, () => {}, { production: false, env: leaked });
    await c.start();
    assert.strictEqual(c.socketPath(), dev.file);
    assert.strictEqual(dev.connections, 1);
    c.dispose();
    dev.connections = 0;
  });

  await check('production never runs a daemon binary marked dev', async () => {
    const bin = fakeBinary(path.join(tmp, 'p3/bin'), std.file);
    fs.writeFileSync(path.join(path.dirname(bin), DEV_MARKER), 'dev-b\n');
    assert.strictEqual(devMarker(bin), 'dev-b');
    const c = new DaemonClient(bin, () => {}, { production: true, env: process.env });
    await assert.rejects(c.start(), /Refusing .*overseerd: it is a dev build \(dev-b\)/);
    assert.deepStrictEqual(runs(bin), [], 'the marked binary was never run');
    c.dispose();
  });

  await check('production refuses a daemon that reports a dev instance, once, and does not retry', async () => {
    const odd = await fakeDaemon(path.join(tmp, 'odd.sock'), 'dev-q');
    const bin = fakeBinary(path.join(tmp, 'p4'), odd.file);
    const said = [];
    const c = new DaemonClient(bin, () => {}, { production: true, env: process.env });
    c.on('refused', m => said.push(m));
    await assert.rejects(c.start(), /The installed Overseer refuses to use dev daemon dev-q/);
    await delay(1500);
    assert.strictEqual(odd.connections, 1, 'no retry into the dev daemon');
    assert.strictEqual(c.connected, false);
    assert.strictEqual(said.length, 1);
    assert.deepStrictEqual(runs(bin), ['socket-path'], 'no daemon was started');
    c.dispose();
  });

  for (const s of servers) s.close();
  fs.rmSync(tmp, { recursive: true, force: true });
  console.log(failures ? `${failures} failed` : 'all passed');
  process.exit(failures ? 1 : 0);
})();
