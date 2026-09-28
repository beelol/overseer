// AC-209, AC-208: a dev profile's window is pinned to one dev instance. It connects only to that
// socket and only to a daemon reporting that instance; when the instance is down it says so, never
// starts a daemon and never falls back to another socket; it picks the instance up again when it
// restarts. Production ignores the pin, and the pin settings are machine-scoped.
// Fake daemons on temporary Unix sockets. Run: node test/unit/dev-pin.js
const assert = require('assert');
const fs = require('fs');
const net = require('net');
const path = require('path');
const { DaemonClient } = require(path.resolve(__dirname, '../../extension/src/daemon-client.js'));

const tmp = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-pin-'));
const servers = [];
let failures = 0;
const check = async (name, fn) => { try { await fn(); console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.stack || e.message); } };
const delay = ms => new Promise(r => setTimeout(r, ms));

function fakeDaemon(file, instance = null) {
  const s = { connections: 0, file, conns: new Set() };
  s.server = net.createServer(conn => {
    s.connections++; s.conns.add(conn); conn.on('close', () => s.conns.delete(conn));
    let buf = '';
    conn.setEncoding('utf8');
    conn.on('data', d => {
      buf += d; let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const msg = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
        conn.write(JSON.stringify({ id: msg.id, result: msg.method === 'hello' ? { instance } : {} }) + '\n');
      }
    });
    conn.on('error', () => {});
  });
  s.stop = () => new Promise(r => { for (const c of s.conns) c.destroy(); s.server.close(r); });
  servers.push(s);
  return new Promise(r => s.server.listen(file, () => r(s)));
}

function fakeBinary(dir, socket) {
  fs.mkdirSync(dir, { recursive: true });
  const bin = path.join(dir, 'overseerd');
  fs.writeFileSync(bin, `#!/bin/sh\necho "$1" >> "${dir}/runs.log"\n[ "$1" = socket-path ] && echo "${socket}"\n`);
  fs.chmodSync(bin, 0o755);
  return bin;
}
const runs = bin => { try { return fs.readFileSync(path.join(path.dirname(bin), 'runs.log'), 'utf8').split('\n').filter(Boolean); } catch { return []; } };

(async () => {
  const standard = await fakeDaemon(path.join(tmp, 'std.sock'));
  const bin = fakeBinary(path.join(tmp, 'bin'), standard.file);
  const aSock = path.join(tmp, 'a.sock');

  await check('a pinned window with its instance down says so, never starts a daemon, never falls back', async () => {
    const said = [];
    const c = new DaemonClient(bin, () => {}, { production: false, pin: { socket: aSock, instance: 'dev-a' } });
    c.on('unreachable', m => said.push(m));
    await assert.rejects(c.start(), /Dev instance dev-a is not running \(socket .*a\.sock\)\. Start it with scripts\/dev up --name a\./);
    assert.strictEqual(standard.connections, 0, 'the standard daemon saw no connection');
    assert.deepStrictEqual(runs(bin), [], 'the daemon binary was never run');
    c.reconnectLater();
    await delay(2500);
    assert.strictEqual(said.length, 1, 'said once per outage');
    assert.strictEqual(standard.connections, 0);
    assert.deepStrictEqual(runs(bin), []);
    // The instance comes up (scripts/dev up): the waiting window connects to it.
    const a = await fakeDaemon(aSock, 'dev-a');
    for (let i = 0; i < 30 && !c.connected; i++) await delay(100);
    assert.strictEqual(c.connected, true, 'picked up when the instance starts');
    // Restarted (scripts/dev up --restart): connection lost, then back, still without spawning.
    await a.stop();
    for (let i = 0; i < 30 && c.connected; i++) await delay(100);
    const again = await fakeDaemon(aSock, 'dev-a');
    for (let i = 0; i < 40 && !c.connected; i++) await delay(100);
    assert.strictEqual(c.connected, true, 'reconnected after a restart');
    assert.deepStrictEqual(runs(bin), [], 'still never ran the binary');
    c.dispose();
    await again.stop();
  });

  await check('a pinned window refuses a daemon reporting another instance, or none', async () => {
    const b = await fakeDaemon(path.join(tmp, 'b.sock'), 'dev-b');
    const c = new DaemonClient(bin, () => {}, { production: false, pin: { socket: b.file, instance: 'dev-a' } });
    await assert.rejects(c.start(), /Pinned to dev instance dev-a, but the daemon at .*b\.sock is dev instance dev-b/);
    const d = new DaemonClient(bin, () => {}, { production: false, pin: { socket: standard.file, instance: 'dev-a' } });
    await assert.rejects(d.start(), /is not a dev instance; refusing it/);
    await delay(1200);
    assert.strictEqual(b.connections, 1, 'no retry');
    c.dispose(); d.dispose();
  });

  await check('production ignores the pin', async () => {
    const c = new DaemonClient(bin, () => {}, { production: true, pin: { socket: aSock, instance: 'dev-a' } });
    assert.strictEqual(c.pin, null);
    assert.strictEqual(c.socketPath(), standard.file);
  });

  await check('the pin settings are machine-scoped (a workspace cannot set them)', () => {
    const props = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../../extension/package.json'), 'utf8')).contributes.configuration.properties;
    for (const key of ['overseer.daemonPath', 'overseer.daemonSocket', 'overseer.devInstance']) assert.strictEqual(props[key]?.scope, 'machine', key);
  });

  for (const s of servers) s.server.close();
  fs.rmSync(tmp, { recursive: true, force: true });
  console.log(failures ? `${failures} failed` : 'all passed');
  process.exit(failures ? 1 : 0);
})();
