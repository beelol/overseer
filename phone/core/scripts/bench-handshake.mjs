// Times the handshakes and the transport in Node: `npm run bench`.
//
// Node 24 runs the TypeScript source as it is, so this measures the code the app ships. The
// phone runs the same code in Hermes, which is slower; these numbers are a baseline for Node
// and say nothing about a phone.

import { randomBytes } from "node:crypto";
import { performance } from "node:perf_hooks";
import { Handshake, HandshakeCounter, MemoryStore, Opener, connectSession, decodePairingCode, generateKeyPair, pskFromSecret, seal, utf8Encode, webSocketFactory } from "../src/index.ts";
import { MockGateway } from "../test/mock-gateway.ts";

const random = (n) => new Uint8Array(randomBytes(n));
const payload1 = utf8Encode(JSON.stringify({ device: "d-1", name: "Bilal's iPhone", platform: "ios", app: "0.1.0", counter: 1790000000000 }));
const payload2 = utf8Encode(JSON.stringify({ protocol: 1, device: "d-1", scope: "full", gateway: "Bilal's Mac", fingerprint: "937b1de964b2e131" }));

function summary(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  const mean = samples.reduce((sum, v) => sum + v, 0) / samples.length;
  return { median: at(0.5), p95: at(0.95), mean, min: sorted[0], runs: samples.length };
}

function line(name, s) {
  const f = (v) => `${v.toFixed(3)} ms`.padStart(12);
  console.log(`${name.padEnd(58)} median ${f(s.median)}   p95 ${f(s.p95)}   mean ${f(s.mean)}   (${s.runs} runs)`);
  return s;
}

async function measure(name, runs, warmup, body) {
  for (let i = 0; i < warmup; i++) await body();
  const samples = [];
  for (let i = 0; i < runs; i++) {
    const start = performance.now();
    await body();
    samples.push(performance.now() - start);
  }
  return line(name, summary(samples));
}

function handshake(kind, device, gateway, secret) {
  const psk = kind === "pairing" ? { psk: pskFromSecret(secret) } : {};
  const initiator = new Handshake({ kind, role: "initiator", staticPrivateKey: device.privateKey, remoteStaticPublicKey: gateway.publicKey, random, ...psk });
  const responder = new Handshake({ kind, role: "responder", staticPrivateKey: gateway.privateKey, random, ...psk });
  responder.readMessage(initiator.writeMessage(payload1));
  initiator.readMessage(responder.writeMessage(payload2));
  return { device: initiator.split(), gateway: responder.split() };
}

function phoneSide(kind, device, gateway, secret, answer) {
  const psk = kind === "pairing" ? { psk: pskFromSecret(secret) } : {};
  const initiator = new Handshake({ kind, role: "initiator", staticPrivateKey: device.privateKey, remoteStaticPublicKey: gateway.publicKey, random, ...psk, ephemeralPrivateKey: answer.ephemeral });
  initiator.writeMessage(payload1);
  initiator.readMessage(answer.message2);
  return initiator.split();
}

/** A recorded answer of the gateway, so that the phone's half can be timed alone. */
function recorded(kind, device, gateway, secret) {
  const ephemeral = random(32);
  const psk = kind === "pairing" ? { psk: pskFromSecret(secret) } : {};
  const initiator = new Handshake({ kind, role: "initiator", staticPrivateKey: device.privateKey, remoteStaticPublicKey: gateway.publicKey, random, ...psk, ephemeralPrivateKey: ephemeral });
  const responder = new Handshake({ kind, role: "responder", staticPrivateKey: gateway.privateKey, random, ...psk });
  responder.readMessage(initiator.writeMessage(payload1));
  return { ephemeral, message2: responder.writeMessage(payload2) };
}

const device = generateKeyPair(random);
const gateway = generateKeyPair(random);
const secret = random(16);

console.log(`Node ${process.version} on ${process.platform} ${process.arch}\n`);
console.log("Handshake, both sides in one process (no network)");
const results = {};
results.pairingBoth = await measure("  pairing  Noise_IKpsk1_25519_ChaChaPoly_SHA256", 300, 50, () => handshake("pairing", device, gateway, secret));
results.sessionBoth = await measure("  session  Noise_IK_25519_ChaChaPoly_SHA256", 300, 50, () => handshake("session", device, gateway, secret));

console.log("\nHandshake, the phone's side alone (write message 1, read message 2, split)");
const pairingAnswer = recorded("pairing", device, gateway, secret);
const sessionAnswer = recorded("session", device, gateway, secret);
results.pairingPhone = await measure("  pairing", 300, 50, () => phoneSide("pairing", device, gateway, secret, pairingAnswer));
results.sessionPhone = await measure("  session", 300, 50, () => phoneSide("session", device, gateway, secret, sessionAnswer));

console.log("\nHandshake over a WebSocket on the loopback address, against the mock gateway");
console.log("(the socket is opened, the counter is stored, both messages cross the wire, the socket is closed)");
const mock = new MockGateway();
await mock.start(0);
const url = `ws://127.0.0.1:${mock.port}/v1`;
const base = { url, socketFactory: webSocketFactory(WebSocket), gatewayPublicKey: mock.keys.publicKey, random, now: Date.now, counter: new HandshakeCounter(new MemoryStore(), "counter", Date.now) };
results.pairingWire = await measure("  pairing, the owner confirms at once", 100, 10, async () => {
  const code = decodePairingCode(mock.openPairing());
  const session = await connectSession({ ...base, kind: "pairing", staticPrivateKey: generateKeyPair(random).privateKey, pairingSecret: code.secret, hello: { device: "", name: "Bench Phone", platform: "ios", app: "0.1.0" } }).result;
  session.close();
});
const paired = generateKeyPair(random);
const pairedId = (await connectSession({ ...base, kind: "pairing", staticPrivateKey: paired.privateKey, pairingSecret: decodePairingCode(mock.openPairing()).secret, hello: { device: "", name: "Bench Phone", platform: "ios", app: "0.1.0" } }).result).gateway.device;
results.sessionWire = await measure("  session", 100, 10, async () => {
  const session = await connectSession({ ...base, kind: "session", staticPrivateKey: paired.privateKey, hello: { device: pairedId, name: "Bench Phone", platform: "ios", app: "0.1.0" } }).result;
  session.close();
});
await mock.stop();

console.log("\nTransport, one message of 1 MiB (17 frames)");
const message = random(1024 * 1024);
const keys = handshake("session", device, gateway, secret);
// Frames open in the order they were sealed (the nonce counts on), so every sealed message,
// also those of the warm-up, is kept and opened in turn.
const sealed = [];
let next = 0;
results.seal = await measure("  seal (split into chunks and encrypt)", 60, 10, () => {
  sealed.push(seal(keys.device.send, message));
});
results.open = await measure("  open (decrypt and join)", 60, 10, () => {
  const opener = new Opener(1024 * 1024);
  let joined = null;
  for (const frame of sealed[next++]) joined = opener.open(keys.gateway.receive, frame);
  if (joined === null || joined.length !== message.length) throw new Error("the message did not come back whole");
});
const megabytesPerSecond = (s) => (1000 / s.median).toFixed(0);
console.log(`\n  that is about ${megabytesPerSecond(results.seal)} MiB/s to seal and ${megabytesPerSecond(results.open)} MiB/s to open`);
