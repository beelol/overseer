// The test-only reference phone (test/ui/ref-phone.js) against the shared Noise vectors
// (protocol/vectors/noise.json): with the vectors' fixed keys its first handshake message is
// byte for byte the vector's, it reads the gateway's reply, and its transport frames match.
// Run: node test/unit/ref-phone.js
const fs = require('fs');
const path = require('path');
const { Handshake, keypair, parseCode, fingerprint, sha256 } = require('../ui/ref-phone');

let failures = 0;
const ok = (name, pass, detail) => { if (!pass) failures++; console.log(pass ? 'ok  ' : 'FAIL', name, pass ? '' : JSON.stringify(detail)); };
const hex = s => Buffer.from(s, 'hex');
const { vectors, chunk } = JSON.parse(fs.readFileSync(path.join(__dirname, '../../protocol/vectors/noise.json'), 'utf8'));
ok('the vectors use the chunk size the phone uses', chunk === 65000, chunk);

for (const v of vectors) {
  const kind = v.name;
  const s = keypair(hex(v.initiator_static_private)), e = keypair(hex(v.initiator_ephemeral_private));
  ok(`${kind}: public keys derive from private keys`, s.public.toString('hex') === v.initiator_static_public && e.public.toString('hex') === v.initiator_ephemeral_public);
  ok(`${kind}: the gateway key's fingerprint`, fingerprint(hex(v.responder_static_public)) === v.responder_fingerprint);
  if (v.pairing_secret) ok(`${kind}: the pre-shared key is SHA-256 of the pairing secret`, sha256(hex(v.pairing_secret)).toString('hex') === v.psk);
  const hs = new Handshake({ kind, staticKey: s, gatewayPublic: hex(v.responder_static_public), psk: v.psk ? hex(v.psk) : undefined, ephemeral: e });
  const m1 = hs.write(v.payload1);
  ok(`${kind}: message 1 is the vector's, byte for byte`, m1.toString('hex') === v.message1, { got: m1.toString('hex').slice(0, 80) });
  let payload2 = '';
  try { payload2 = hs.read(hex(v.message2)).toString('utf8'); } catch (error) { payload2 = 'error: ' + error.message; }
  ok(`${kind}: message 2 decrypts to the vector's payload`, payload2 === v.payload2, payload2);
  ok(`${kind}: the handshake hash`, hs.h.toString('hex') === v.handshake_hash);
  let frames = true, opened = true;
  for (const t of v.transport) {
    if (t.from === 'device') frames = frames && JSON.stringify(hs.transport.seal(t.plaintext).map(f => f.toString('hex'))) === JSON.stringify(t.frames);
    else { let whole; for (const f of t.frames) whole = hs.transport.open(hex(f)); opened = opened && whole?.toString('utf8') === t.plaintext; }
  }
  ok(`${kind}: frames the phone sends are the vector's`, frames);
  ok(`${kind}: frames from the gateway open to the vector's text`, opened);
  if (v.psk) {
    const wrong = new Handshake({ kind, staticKey: s, gatewayPublic: hex(v.responder_static_public), psk: sha256(Buffer.from('a guess')), ephemeral: e });
    ok(`${kind}: another secret gives another first message`, wrong.write(v.payload1).toString('hex') !== v.message1);
  }
}

// The pairing code layout (docs/rfcs/phone-remote-protocol.md, "Pairing code").
const key = Buffer.from(Array.from({ length: 32 }, (_, i) => i)), secret = Buffer.alloc(16, 7);
const bytes = Buffer.concat([Buffer.from([1]), key, secret, Buffer.from([47810 >> 8, 47810 & 255, 2, 12]), Buffer.from('192.168.1.20'), Buffer.from([9]), Buffer.from('127.0.0.1')]);
const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
let text = '', buffer = 0, bits = 0;
for (const b of bytes) { buffer = (buffer << 8) | b; bits += 8; while (bits >= 5) { bits -= 5; text += B32[(buffer >> bits) & 31]; } buffer &= (1 << bits) - 1; }
if (bits) text += B32[(buffer << (5 - bits)) & 31];
const code = parseCode('OVSR1-' + text);
ok('a pairing code gives the key, the secret, the port and the addresses', code.gatewayPublic.equals(key) && code.secret.equals(secret) && code.port === 47810 && JSON.stringify(code.addresses) === '["192.168.1.20","127.0.0.1"]', code);
let refusedBad = false; try { parseCode('OVSR2-' + text); } catch { refusedBad = true; }
ok('text that is not a pairing code is refused', refusedBad);

console.log(failures ? `${failures} failure(s)` : 'the reference phone matches the shared vectors');
process.exit(failures ? 1 : 0);
