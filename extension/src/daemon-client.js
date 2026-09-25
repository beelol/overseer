// Client for the overseerd local protocol (newline-delimited JSON over a Unix socket).
// Keeps one connection, reconnects (starting the daemon if needed) and resumes the
// event stream from the last seen cursor so reconnects neither drop nor duplicate events.
const net = require('net');
const fs = require('fs');
const path = require('path');
const { spawn, execFileSync } = require('child_process');
const { EventEmitter } = require('events');

class DaemonClient extends EventEmitter {
  constructor(binary, log) {
    super();
    this.binary = binary;
    this.log = log;
    this.nextId = 1;
    this.pending = new Map();
    this.cursor = 0;
    this.connected = false;
    this.disposed = false;
    this.buffer = '';
    this.seen = new Set();
  }

  socketPath() {
    if (!this._socket) this._socket = execFileSync(this.binary, ['socket-path'], { encoding: 'utf8' }).trim();
    return this._socket;
  }

  async start() {
    for (let attempt = 0; attempt < 50 && !this.disposed; attempt++) {
      try { await this.connect(); return; } catch (error) {
        if (attempt === 0) this.spawnDaemon();
        await new Promise(r => setTimeout(r, 200));
      }
    }
    throw new Error('Could not connect to overseerd.');
  }

  spawnDaemon() {
    if (!fs.existsSync(this.binary)) throw new Error(`overseerd not found at ${this.binary}`);
    this.log(`starting daemon ${this.binary}`);
    // Detached: the daemon must outlive this window.
    const child = spawn(this.binary, ['serve'], { detached: true, stdio: 'ignore' });
    child.unref();
  }

  connect() {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath());
      let opened = false;
      socket.setEncoding('utf8');
      socket.on('connect', () => {
        opened = true; this.socket = socket; this.connected = true; this.buffer = '';
        this.log('connected to daemon');
        this.emit('connected');
        this.request('events.subscribe', { after: this.cursor }).catch(e => this.log('subscribe failed: ' + e.message));
        resolve();
      });
      socket.on('data', chunk => this.onData(chunk));
      socket.on('error', error => { if (!opened) reject(error); });
      socket.on('close', () => {
        if (!opened) return;
        this.connected = false; this.socket = undefined;
        for (const { reject: fail } of this.pending.values()) fail(new Error('Daemon connection lost.'));
        this.pending.clear();
        this.emit('disconnected');
        if (!this.disposed) this.reconnectLater();
      });
    });
  }

  reconnectLater() {
    clearTimeout(this.retry);
    this.retry = setTimeout(async () => {
      try { await this.connect(); } catch {
        try { this.spawnDaemon(); } catch (e) { this.log(e.message); }
        this.reconnectLater();
      }
    }, 1000);
  }

  onData(chunk) {
    this.buffer += chunk;
    let index;
    while ((index = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, index); this.buffer = this.buffer.slice(index + 1);
      if (!line) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { this.log('bad line from daemon'); continue; }
      if (msg.method === 'event') {
        const event = msg.params;
        if (event.seq <= this.cursor) continue; // replay overlap: never apply twice
        this.cursor = event.seq;
        this.emit('event', event);
      } else if (msg.method === 'resync') {
        this.log('event stream lagged; resubscribing from cursor ' + this.cursor);
        this.request('events.subscribe', { after: this.cursor }).catch(() => {});
      } else if (msg.method === 'replayed') {
        this.emit('replayed', msg.params);
      } else if (msg.id !== undefined && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id); this.pending.delete(msg.id);
        if (msg.error) reject(new Error(msg.error.message || msg.error.code)); else resolve(msg.result);
      }
    }
  }

  waitConnected(timeout = 15000) {
    if (this.connected) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.off('connected', done); reject(new Error('Timed out connecting to overseerd.')); }, timeout);
      const done = () => { clearTimeout(timer); resolve(); };
      this.once('connected', done);
    });
  }

  request(method, params = {}) {
    if (!this.socket) return Promise.reject(new Error('Not connected to overseerd.'));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.write(JSON.stringify({ id, method, params }) + '\n');
    });
  }

  dispose() {
    this.disposed = true; clearTimeout(this.retry);
    this.socket?.destroy();
  }
}

function resolveBinary(context, configured) {
  if (configured) return configured;
  const bundled = path.join(context.extensionPath, 'bin', `overseerd-${process.platform}-${process.arch}`);
  if (fs.existsSync(bundled)) return bundled;
  return path.join(context.extensionPath, 'bin', 'overseerd');
}

module.exports = { DaemonClient, resolveBinary };
