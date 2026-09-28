// Test-only reference phone: the device's side of docs/rfcs/phone-remote-protocol.md, written
// with Node's own crypto (X25519, ChaCha20-Poly1305, SHA-256, HMAC) and Node's WebSocket. No
// packages. It pairs with a real gateway the way a phone does and then holds a session.
// test/unit/ref-phone.js checks it against the shared vectors in protocol/vectors/noise.json.
//
// As a program (the terminal UI's tests start it):
//   node test/ui/ref-phone.js pair <code> [--name N] [--platform ios|android] [--host H] [--hold SECONDS]
// It prints one JSON object per line: {"event":"asked"}, then {"event":"paired",...} or
// {"event":"refused"}, then {"event":"notice","state":"off"|"revoked"} and {"event":"closed"}.
const crypto = require('crypto');

const PROLOGUE = Buffer.from('overseer-gateway-v1');
const PATTERN = { session: 'Noise_IK_25519_ChaChaPoly_SHA256', pairing: 'Noise_IKpsk1_25519_ChaChaPoly_SHA256' };
const KIND = { session: 0x01, pairing: 0x02 };
const CHUNK = 65000;

// ------------------------------------------------------------------ keys

const PKCS8 = Buffer.from('302e020100300506032b656e04220420', 'hex');
const SPKI = Buffer.from('302a300506032b656e032100', 'hex');
const privateKey = raw => crypto.createPrivateKey({ key: Buffer.concat([PKCS8, raw]), format: 'der', type: 'pkcs8' });
const publicKey = raw => crypto.createPublicKey({ key: Buffer.concat([SPKI, raw]), format: 'der', type: 'spki' });
const publicOf = raw => crypto.createPublicKey(privateKey(raw)).export({ format: 'der', type: 'spki' }).subarray(SPKI.length);
const dh = (priv, pub) => crypto.diffieHellman({ privateKey: privateKey(priv), publicKey: publicKey(pub) });
function keypair(priv = crypto.randomBytes(32)) { const p = Buffer.from(priv); return { private: p, public: Buffer.from(publicOf(p)) }; }
const sha256 = (...parts) => { const h = crypto.createHash('sha256'); for (const p of parts) h.update(p); return h.digest(); };
const hmac = (key, ...parts) => { const h = crypto.createHmac('sha256', key); for (const p of parts) h.update(p); return h.digest(); };
const fingerprint = pub => sha256(pub).toString('hex').slice(0, 16);

function hkdf(ck, ikm, n) {
  const temp = hmac(ck, ikm);
  const out = [hmac(temp, Buffer.from([1]))];
  for (let i = 2; i <= n; i++) out.push(hmac(temp, out[out.length - 1], Buffer.from([i])));
  return out;
}

// ------------------------------------------------------------------ cipher

function nonce(n) { const b = Buffer.alloc(12); b.writeBigUInt64LE(BigInt(n), 4); return b; }
function seal(key, n, ad, plain) {
  const c = crypto.createCipheriv('chacha20-poly1305', key, nonce(n), { authTagLength: 16 });
  c.setAAD(ad, { plaintextLength: plain.length });
  return Buffer.concat([c.update(plain), c.final(), c.getAuthTag()]);
}
function open(key, n, ad, sealed) {
  if (sealed.length < 16) throw new Error('short message');
  const d = crypto.createDecipheriv('chacha20-poly1305', key, nonce(n), { authTagLength: 16 });
  d.setAAD(ad, { plaintextLength: sealed.length - 16 });
  d.setAuthTag(sealed.subarray(sealed.length - 16));
  return Buffer.concat([d.update(sealed.subarray(0, sealed.length - 16)), d.final()]);
}

// ------------------------------------------------------------------ handshake (initiator)

/** IK, or IKpsk1 when `psk` is given. The device starts; the gateway's static key is known. */
class Handshake {
  constructor({ kind, staticKey, gatewayPublic, psk, ephemeral }) {
    this.kind = kind; this.s = staticKey; this.rs = Buffer.from(gatewayPublic); this.psk = psk;
    this.e = ephemeral || keypair();
    const name = Buffer.from(PATTERN[kind]);
    this.h = name.length <= 32 ? Buffer.concat([name, Buffer.alloc(32 - name.length)]) : sha256(name);
    this.ck = this.h;
    this.k = undefined; this.n = 0;
    this.mixHash(PROLOGUE);
    this.mixHash(this.rs); // pre-message: <- s
  }
  mixHash(data) { this.h = sha256(this.h, data); }
  mixKey(ikm) { const [ck, k] = hkdf(this.ck, ikm, 2); this.ck = ck; this.k = k; this.n = 0; }
  mixKeyAndHash(ikm) { const [ck, h, k] = hkdf(this.ck, ikm, 3); this.ck = ck; this.mixHash(h); this.k = k; this.n = 0; }
  encryptAndHash(plain) { const out = this.k ? seal(this.k, this.n++, this.h, plain) : Buffer.from(plain); this.mixHash(out); return out; }
  decryptAndHash(sealed) { const out = this.k ? open(this.k, this.n++, this.h, sealed) : Buffer.from(sealed); this.mixHash(sealed); return out; }

  /** -> e, es, s, ss [, psk] and the payload. */
  write(payload) {
    const out = [this.e.public];
    this.mixHash(this.e.public);
    if (this.psk) this.mixKey(this.e.public);
    this.mixKey(dh(this.e.private, this.rs));
    out.push(this.encryptAndHash(this.s.public));
    this.mixKey(dh(this.s.private, this.rs));
    if (this.psk) this.mixKeyAndHash(this.psk);
    out.push(this.encryptAndHash(Buffer.from(payload)));
    return Buffer.concat(out);
  }

  /** <- e, ee, se and the payload. Returns the payload; the transport keys are ready after it. */
  read(message) {
    const m = Buffer.from(message);
    if (m.length < 32 + 16) throw new Error('short handshake reply');
    const re = m.subarray(0, 32);
    this.mixHash(re);
    if (this.psk) this.mixKey(re);
    this.mixKey(dh(this.e.private, re));
    this.mixKey(dh(this.s.private, re));
    const payload = this.decryptAndHash(m.subarray(32));
    const [send, receive] = hkdf(this.ck, Buffer.alloc(0), 2);
    this.transport = new Transport(send, receive);
    return payload;
  }
}

class Transport {
  constructor(send, receive) { this.send = send; this.receive = receive; this.ns = 0; this.nr = 0; this.pending = []; }
  /** One protocol message as encrypted frames (chunks of at most 65,000 bytes). */
  seal(message) {
    const bytes = Buffer.from(message);
    const frames = [];
    let at = 0;
    do {
      const chunk = bytes.subarray(at, at + CHUNK); at += chunk.length;
      frames.push(seal(this.send, this.ns++, Buffer.alloc(0), Buffer.concat([Buffer.from([at >= bytes.length ? 1 : 0]), chunk])));
    } while (at < bytes.length);
    return frames;
  }
  /** A whole message when `frame` was its last chunk, else undefined. */
  open(frame) {
    const plain = open(this.receive, this.nr++, Buffer.alloc(0), Buffer.from(frame));
    this.pending.push(plain.subarray(1));
    if (plain[0] === 0) return undefined;
    if (plain[0] !== 1) throw new Error('bad chunk flag');
    const whole = Buffer.concat(this.pending); this.pending = [];
    return whole;
  }
}

// ------------------------------------------------------------------ the pairing code

const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
function base32Decode(text) {
  const out = []; let buffer = 0, bits = 0;
  for (const ch of text.toUpperCase()) {
    if (ch === '-' || /\s/.test(ch)) continue;
    const v = B32.indexOf(ch);
    if (v < 0) throw new Error('not a pairing code');
    buffer = (buffer << 5) | v; bits += 5;
    if (bits >= 8) { bits -= 8; out.push((buffer >> bits) & 0xff); buffer &= (1 << bits) - 1; }
  }
  return Buffer.from(out);
}

function parseCode(text) {
  const t = String(text).trim();
  if (!t.startsWith('OVSR1-')) throw new Error('not a pairing code');
  const b = base32Decode(t.slice(6));
  if (b.length < 52 || b[0] !== 1) throw new Error('not a pairing code');
  const addresses = []; let at = 52;
  for (let i = 0; i < b[51]; i++) { const n = b[at]; addresses.push(b.subarray(at + 1, at + 1 + n).toString('ascii')); at += 1 + n; }
  if (at !== b.length) throw new Error('the pairing code has bytes after its last address');
  return { gatewayPublic: Buffer.from(b.subarray(1, 33)), secret: Buffer.from(b.subarray(33, 49)), port: b.readUInt16BE(49), addresses };
}

// ------------------------------------------------------------------ the phone

class Phone {
  constructor({ name = 'Test Phone', platform = 'ios', app = '0.1.0-test', keys = keypair() } = {}) {
    this.name = name; this.platform = platform; this.app = app; this.keys = keys;
    this.inbox = []; this.waiters = []; this.nextId = 1000; this.closed = false; this.counter = 0;
    this.ended = new Promise(resolve => { this.end = resolve; });
  }

  /**
   * Connects and sends the first frame. Resolves with the gateway's hello once it answers (for
   * pairing: after the owner chose Pair); rejects when the gateway closes without an answer.
   * An answer is not yet a session that serves: a gateway may complete the handshake of a
   * revoked phone only to tell it so (the notice "revoked") and close. `serves()` says which.
   */
  connect({ kind, host = '127.0.0.1', port, gatewayPublic, secret, device = '', wait = 70000 }) {
    this.gatewayPublic = Buffer.from(gatewayPublic); this.port = port; this.host = host;
    // Each connection is its own session: what ended before says nothing about this one.
    clearInterval(this.alive);
    this.closed = false; this.asked = false; this.error = undefined; this.transport = undefined;
    this.ended = new Promise(resolve => { this.end = resolve; });
    const hs = new Handshake({ kind, staticKey: this.keys, gatewayPublic, psk: kind === 'pairing' ? sha256(secret) : undefined });
    this.counter = Math.max(Date.now(), this.counter + 1);
    const payload = JSON.stringify({ device, name: this.name, platform: this.platform, app: this.app, counter: this.counter });
    const first = Buffer.concat([Buffer.from([0x01, KIND[kind]]), hs.write(payload)]);
    return new Promise((resolve, reject) => {
      let answered = false;
      const ws = new WebSocket(`ws://${host}:${port}/v1`);
      ws.binaryType = 'arraybuffer';
      this.ws = ws;
      const timer = setTimeout(() => { if (!answered) { try { ws.close(); } catch {} reject(new Error('the gateway did not answer in time')); } }, wait);
      ws.addEventListener('open', () => { ws.send(first); this.asked = true; this.onAsked?.(); });
      ws.addEventListener('message', event => {
        if (this.ws !== ws) return;
        const frame = Buffer.from(event.data);
        try {
          if (!answered) {
            const hello = JSON.parse(hs.read(frame).toString('utf8'));
            answered = true; clearTimeout(timer);
            this.transport = hs.transport; this.hello = hello; this.device = hello.device;
            resolve(hello);
            return;
          }
          const whole = this.transport.open(frame);
          if (whole) this.deliver(JSON.parse(whole.toString('utf8')));
        } catch (error) { this.error = error; try { ws.close(); } catch {} }
      });
      const over = () => {
        if (this.ws !== ws) return; // an earlier connection of this phone
        clearTimeout(timer); clearInterval(this.alive);
        this.closed = true;
        if (!answered) reject(new Error(this.asked ? 'the gateway closed the connection without an answer' : 'could not connect'));
        for (const w of this.waiters.splice(0)) w.reject(new Error('the session ended'));
        this.end();
      };
      ws.addEventListener('close', over);
      ws.addEventListener('error', () => { if (!this.asked) over(); });
    });
  }

  pair(code, opts = {}) {
    const c = typeof code === 'string' ? parseCode(code) : code;
    return this.connect({ kind: 'pairing', port: c.port, gatewayPublic: c.gatewayPublic, secret: c.secret, ...opts });
  }

  session(opts = {}) {
    return this.connect({ kind: 'session', port: this.port, host: this.host, gatewayPublic: this.gatewayPublic, device: this.device, ...opts });
  }

  deliver(message) {
    const i = message.id === undefined ? -1 : this.waiters.findIndex(w => w.id === message.id);
    if (i >= 0) { const [w] = this.waiters.splice(i, 1); w.resolve(message); return; }
    this.inbox.push(message);
    this.onMessage?.(message);
  }

  /** Sends a request and resolves with its whole reply ({result} or {error}). */
  ask(method, params = {}, requestId) {
    const id = this.nextId++;
    const message = { id, method, params };
    if (requestId) message.request_id = requestId;
    return new Promise((resolve, reject) => {
      if (this.closed) { reject(new Error('the session ended')); return; }
      this.waiters.push({ id, resolve, reject });
      for (const frame of this.transport.seal(JSON.stringify(message))) this.ws.send(frame);
    });
  }

  /**
   * Whether this session serves requests: asks for `hello` and waits for the answer. False when
   * the session ends first or answers with an error (a revoked phone, phone access turned off).
   */
  async serves(ms = 4000) {
    if (this.closed || !this.transport) return false;
    const reply = await Promise.race([this.ask('hello', { client: 'phone' }).catch(() => undefined), this.ended.then(() => undefined), new Promise(r => setTimeout(() => r(undefined), ms))]);
    return !!reply?.result;
  }

  /** Keeps the session from going silent: the gateway ends one that says nothing for a minute. */
  keepAlive(everyMs = 15000) {
    clearInterval(this.alive);
    this.alive = setInterval(() => { if (!this.closed) this.ask('hello', { client: 'phone' }).catch(() => {}); }, everyMs);
    this.alive.unref?.();
  }

  /** The gateway's notices so far: "off" before phone access is turned off, "revoked" before a revoked session ends. */
  notices() { return this.inbox.filter(m => m.method === 'gateway').map(m => m.params?.state); }

  /** True when the session ends within `ms`. */
  endsWithin(ms) { return this.closed ? Promise.resolve(true) : Promise.race([this.ended.then(() => true), new Promise(r => setTimeout(() => r(false), ms))]); }

  close() { clearInterval(this.alive); try { this.ws?.close(); } catch {} }
}

/** True when nothing accepts a TCP connection on the port. */
function refused(port, host = '127.0.0.1') {
  return new Promise(resolve => {
    const socket = require('net').connect({ port, host });
    const done = v => { socket.destroy(); resolve(v); };
    socket.once('connect', () => done(false));
    socket.once('error', () => done(true));
    socket.setTimeout(1500, () => done(true));
  });
}

/** A port nothing listens on right now. */
function freePort() {
  return new Promise((resolve, reject) => {
    const server = require('net').createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => { const { port } = server.address(); server.close(() => resolve(port)); });
  });
}

module.exports = { Phone, Handshake, Transport, keypair, parseCode, base32Decode, fingerprint, refused, freePort, sha256 };

if (require.main === module) {
  const [command, code, ...rest] = process.argv.slice(2);
  const opt = (name, fallback) => { const i = rest.indexOf('--' + name); return i >= 0 ? rest[i + 1] : fallback; };
  const say = o => process.stdout.write(JSON.stringify(o) + '\n');
  if (command !== 'pair' || !code) { console.error('usage: ref-phone.js pair <code> [--name N] [--platform ios|android] [--host H] [--hold SECONDS]'); process.exit(2); }
  const phone = new Phone({ name: opt('name', 'Test Phone'), platform: opt('platform', 'ios') });
  phone.onAsked = () => say({ event: 'asked', name: phone.name, key: fingerprint(phone.keys.public) });
  phone.onMessage = m => { if (m.method === 'gateway') say({ event: 'notice', state: m.params?.state }); };
  phone.pair(code, { host: opt('host', '127.0.0.1') }).then(async hello => {
    say({ event: 'paired', device: hello.device, scope: hello.scope, gateway: hello.gateway, fingerprint: hello.fingerprint });
    phone.keepAlive();
    const ended = await phone.endsWithin(Number(opt('hold', '30')) * 1000);
    if (!ended) phone.close();
    await phone.endsWithin(2000);
    say({ event: 'closed', by: ended ? 'mac' : 'phone' });
    process.exit(0);
  }, error => { say({ event: 'refused', why: error.message }); process.exit(1); });
}
