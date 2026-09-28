// Client for the overseerd local protocol (newline-delimited JSON over a Unix socket).
// Keeps one connection, reconnects (starting the daemon if needed) and resumes the
// event stream from the last seen cursor so reconnects neither drop nor duplicate events.
const net = require('net');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, execFileSync } = require('child_process');
const { EventEmitter } = require('events');

// Production never points at a dev version (AC-212). Everything dev is marked: a dev daemon runs
// with OVERSEER_INSTANCE=dev-<name> and reports it in `hello`, and dev binaries carry this file.
const DEV_MARKER = 'overseer-dev-instance';
// Variables that point a daemon at another instance: production ignores them (a terminal where a
// dev instance was set up, or a launcher, can leak them into VS Code's environment).
const LEAKY = ['OVERSEER_HOME', 'OVERSEER_SOCKET', 'OVERSEER_INSTANCE'];

const real = p => { try { return fs.realpathSync(p); } catch { return path.resolve(p); } };

/** Production is the extension installed in the owner's standard VS Code extensions folder. Any
 *  other folder (`code --extensions-dir`: the UI test harness, dev profiles) is not. */
function isProductionInstall(extensionPath, home = os.homedir()) {
  const parent = real(path.dirname(extensionPath));
  return ['.vscode', '.vscode-insiders'].some(d => real(path.join(home, d, 'extensions')) === parent);
}

/** The dev instance a binary is marked with (the file next to it), or null. */
function devMarker(binary) {
  try { return fs.readFileSync(path.join(path.dirname(binary), DEV_MARKER), 'utf8').trim() || 'dev'; } catch { return null; }
}

/** The environment production runs daemon commands with: the leaky variables removed. */
function productionEnv(env = process.env) {
  const out = { ...env };
  for (const key of LEAKY) delete out[key];
  return out;
}

class DaemonClient extends EventEmitter {
  /** opts.production: the installed extension (see isProductionInstall); opts.env: the base environment. */
  constructor(binary, log, opts = {}) {
    super();
    this.binary = binary;
    this.log = log;
    this.production = !!opts.production;
    this.env = this.production ? productionEnv(opts.env || process.env) : (opts.env || process.env);
    this.refusal = null;
    this.nextId = 1;
    this.pending = new Map();
    this.cursor = 0;
    this.connected = false;
    this.disposed = false;
    this.buffer = '';
    this.seen = new Set();
  }

  /** Production never starts or asks a daemon binary marked dev. */
  checkBinary() {
    if (!this.production) return;
    const marker = devMarker(this.binary);
    if (marker) throw new Error(`Refusing ${this.binary}: it is a dev build (${marker}). The installed Overseer only runs its own daemon; clear overseer.daemonPath, and use scripts/dev code for dev instances.`);
  }

  socketPath() {
    if (!this._socket) {
      this.checkBinary();
      this._socket = execFileSync(this.binary, ['socket-path'], { encoding: 'utf8', env: this.env }).trim();
    }
    return this._socket;
  }

  async start() {
    this.stopped = false;
    for (let attempt = 0; attempt < 50 && !this.disposed; attempt++) {
      try { await this.connect(); return; } catch (error) {
        if (this.refusal) throw error;
        if (attempt === 0) this.spawnDaemon();
        await new Promise(r => setTimeout(r, 200));
      }
    }
    throw new Error('Could not connect to overseerd.');
  }

  spawnDaemon() {
    if (!fs.existsSync(this.binary)) throw new Error(`overseerd not found at ${this.binary}`);
    this.checkBinary();
    this.log(`starting daemon ${this.binary}`);
    // Detached: the daemon must outlive this window.
    const child = spawn(this.binary, ['serve'], { detached: true, stdio: 'ignore', env: this.env });
    child.unref();
  }

  connect() {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath());
      let opened = false, ready = false;
      socket.setEncoding('utf8');
      socket.on('connect', () => {
        opened = true; this.socket = socket; this.buffer = '';
        // Identify as a VS Code window: the daemon notifies when the last one closes while agents run.
        // Its answer says which daemon this is: production refuses a dev instance (AC-212).
        this.request('hello', { client: 'vscode' }).then(hello => {
          const refusal = this.refuse(hello || {});
          if (refusal) {
            this.refusal = refusal; this.log(refusal);
            this.emit('refused', refusal);
            socket.destroy(); reject(new Error(refusal)); return;
          }
          ready = true; this.connected = true;
          this.log('connected to daemon');
          this.emit('connected');
          this.request('events.subscribe', { after: this.cursor }).catch(e => this.log('subscribe failed: ' + e.message));
          // Voice Mode's live channel (state, levels, words in progress): never stored by the daemon.
          this.request('voice.subscribe', {}).catch(() => { /* a daemon without Voice Mode */ });
          resolve();
        }, e => { this.log('hello failed: ' + e.message); reject(e); });
      });
      socket.on('data', chunk => this.onData(chunk));
      socket.on('error', error => { if (!opened) reject(error); });
      socket.on('close', () => {
        if (!opened) return;
        if (this.refusal || !ready) {
          // Refused, or closed before it said who it is: the caller's retry decides what next.
          this.socket = undefined; this.connected = false;
          for (const { reject: fail } of this.pending.values()) fail(new Error('Daemon connection lost.'));
          this.pending.clear();
          return;
        }
        this.connected = false; this.socket = undefined;
        for (const { reject: fail } of this.pending.values()) fail(new Error('Daemon connection lost.'));
        this.pending.clear();
        this.emit('disconnected');
        // A deliberate "Stop Agents and Daemon" (from any window) must not respawn the daemon.
        if (this.stopped) { this.emit('stopped'); return; }
        if (!this.disposed) this.reconnectLater();
      });
    });
  }

  /** Why this daemon must not be used, or null. */
  refuse(hello) {
    if (this.production && hello.instance) return `The installed Overseer refuses to use dev instance ${hello.instance} (socket ${this.socketPath()}). Dev instances are for dev VS Code profiles (scripts/dev code); production uses only the standard daemon.`;
    return null;
  }

  reconnectLater() {
    if (this.refusal) return;
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
        if (event.kind === 'daemon_stopping') this.stopped = true;
        this.emit('event', event);
      } else if (msg.method === 'resync') {
        this.log('event stream lagged; resubscribing from cursor ' + this.cursor);
        this.request('events.subscribe', { after: this.cursor }).catch(() => {});
      } else if (msg.method === 'replayed') {
        this.emit('replayed', msg.params);
      } else if (msg.method === 'voice') {
        this.emit('voice', msg.params);
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

module.exports = { DaemonClient, resolveBinary, isProductionInstall, devMarker, productionEnv, DEV_MARKER, LEAKY };
