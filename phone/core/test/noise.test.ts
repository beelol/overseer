import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { bytesToHex, hexToBytes, utf8Decode, utf8Encode } from "../src/bytes.ts";
import { Opener, seal } from "../src/frames.ts";
import {
  CipherState,
  fingerprint,
  generateKeyPair,
  Handshake,
  type HandshakeKind,
  NoiseError,
  PATTERN_PAIRING,
  PATTERN_SESSION,
  PROLOGUE,
  pskFromSecret,
  publicKeyOf,
  type TransportKeys,
} from "../src/noise.ts";
import { seededRandom } from "./helpers.ts";

interface Vector {
  name: string;
  pattern: string;
  prologue: string;
  initiator_static_private: string;
  initiator_static_public: string;
  initiator_ephemeral_private: string;
  initiator_ephemeral_public: string;
  responder_static_private: string;
  responder_static_public: string;
  responder_ephemeral_private: string;
  responder_ephemeral_public: string;
  responder_fingerprint: string;
  pairing_secret: string | null;
  psk: string | null;
  payload1: string;
  payload2: string;
  message1: string;
  message2: string;
  handshake_hash: string;
  transport: { from: "device" | "gateway"; plaintext: string; frames: string[] }[];
}

const file = new URL("../../../protocol/vectors/noise.json", import.meta.url);
const shared = JSON.parse(readFileSync(file, "utf8")) as { chunk: number; vectors: Vector[] };
const random = seededRandom(1);

function kindOf(v: Vector): HandshakeKind {
  return v.name === "pairing" ? "pairing" : "session";
}

function pair(v: Vector, overrides: { responderStatic?: Uint8Array; initiatorRemote?: Uint8Array; responderPsk?: Uint8Array } = {}) {
  const kind = kindOf(v);
  const psk = v.psk ? hexToBytes(v.psk) : undefined;
  const initiator = new Handshake({
    kind,
    role: "initiator",
    staticPrivateKey: hexToBytes(v.initiator_static_private),
    remoteStaticPublicKey: overrides.initiatorRemote ?? hexToBytes(v.responder_static_public),
    ephemeralPrivateKey: hexToBytes(v.initiator_ephemeral_private),
    random,
    ...(psk ? { psk } : {}),
  });
  const responderPsk = overrides.responderPsk ?? psk;
  const responder = new Handshake({
    kind,
    role: "responder",
    staticPrivateKey: overrides.responderStatic ?? hexToBytes(v.responder_static_private),
    ephemeralPrivateKey: hexToBytes(v.responder_ephemeral_private),
    random,
    ...(responderPsk ? { psk: responderPsk } : {}),
  });
  return { initiator, responder };
}

function complete(v: Vector): { device: TransportKeys; gateway: TransportKeys } {
  const { initiator, responder } = pair(v);
  responder.readMessage(initiator.writeMessage(utf8Encode(v.payload1)));
  initiator.readMessage(responder.writeMessage(utf8Encode(v.payload2)));
  return { device: initiator.split(), gateway: responder.split() };
}

describe("the shared vectors (protocol/vectors/noise.json)", () => {
  it("holds a session vector and a pairing vector for the patterns of the specification", () => {
    expect(shared.chunk).toBe(65_000);
    expect(shared.vectors.map((v) => v.name)).toEqual(["session", "pairing"]);
    expect(shared.vectors.map((v) => v.pattern)).toEqual([PATTERN_SESSION, PATTERN_PAIRING]);
    for (const v of shared.vectors) expect(v.prologue).toBe(PROLOGUE);
  });

  for (const v of shared.vectors) {
    describe(v.name, () => {
      it("derives the same public keys, pre-shared key and fingerprint", () => {
        expect(bytesToHex(publicKeyOf(hexToBytes(v.initiator_static_private)))).toBe(v.initiator_static_public);
        expect(bytesToHex(publicKeyOf(hexToBytes(v.initiator_ephemeral_private)))).toBe(v.initiator_ephemeral_public);
        expect(bytesToHex(publicKeyOf(hexToBytes(v.responder_static_private)))).toBe(v.responder_static_public);
        expect(bytesToHex(publicKeyOf(hexToBytes(v.responder_ephemeral_private)))).toBe(v.responder_ephemeral_public);
        expect(fingerprint(hexToBytes(v.responder_static_public))).toBe(v.responder_fingerprint);
        if (v.pairing_secret && v.psk) expect(bytesToHex(pskFromSecret(hexToBytes(v.pairing_secret)))).toBe(v.psk);
      });

      it("reproduces message1, message2 and the handshake hash byte for byte, in both roles", () => {
        const { initiator, responder } = pair(v);
        const message1 = initiator.writeMessage(utf8Encode(v.payload1));
        expect(bytesToHex(message1)).toBe(v.message1);

        expect(utf8Decode(responder.readMessage(hexToBytes(v.message1)))).toBe(v.payload1);
        expect(bytesToHex(responder.remoteStaticPublicKey as Uint8Array)).toBe(v.initiator_static_public);

        const message2 = responder.writeMessage(utf8Encode(v.payload2));
        expect(bytesToHex(message2)).toBe(v.message2);

        expect(utf8Decode(initiator.readMessage(hexToBytes(v.message2)))).toBe(v.payload2);
        expect(initiator.complete && responder.complete).toBe(true);

        const device = initiator.split();
        const gateway = responder.split();
        expect(bytesToHex(device.handshakeHash)).toBe(v.handshake_hash);
        expect(bytesToHex(gateway.handshakeHash)).toBe(v.handshake_hash);
        expect(bytesToHex(device.remoteStaticPublicKey)).toBe(v.responder_static_public);
        expect(bytesToHex(gateway.remoteStaticPublicKey)).toBe(v.initiator_static_public);
      });

      it("reproduces every transport frame byte for byte, and opens the daemon's frames", () => {
        const { device, gateway } = complete(v);
        expect(v.transport.length).toBeGreaterThan(0);
        for (const step of v.transport) {
          const [tx, rx] = step.from === "device" ? [device.send, gateway.receive] : [gateway.send, device.receive];
          const frames = seal(tx, utf8Encode(step.plaintext));
          expect(frames.map(bytesToHex)).toEqual(step.frames);
          const opener = new Opener(1 << 20);
          let joined: Uint8Array | null = null;
          for (const frame of step.frames) joined = opener.open(rx, hexToBytes(frame));
          expect(utf8Decode(joined as Uint8Array)).toBe(step.plaintext);
        }
      });

      it("a gateway with another key cannot read message1, and the device refuses its answer", () => {
        const other = generateKeyPair(seededRandom(7));
        const { initiator, responder } = pair(v, { responderStatic: other.privateKey });
        const message1 = initiator.writeMessage(utf8Encode(v.payload1));
        expect(() => responder.readMessage(message1)).toThrowError(NoiseError);
        // An impostor that answers anyway, with a handshake of its own, proves nothing.
        const impostor = pair(v, { responderStatic: other.privateKey, initiatorRemote: other.publicKey });
        impostor.responder.readMessage(impostor.initiator.writeMessage(utf8Encode(v.payload1)));
        const forged = impostor.responder.writeMessage(utf8Encode(v.payload2));
        expect(() => initiator.readMessage(forged)).toThrowError(NoiseError);
        expect(() => initiator.split()).toThrowError(NoiseError);
      });

      it("a device that expects another gateway key is refused", () => {
        const other = generateKeyPair(seededRandom(8));
        const { initiator, responder } = pair(v, { initiatorRemote: other.publicKey });
        expect(() => responder.readMessage(initiator.writeMessage(utf8Encode(v.payload1)))).toThrowError(NoiseError);
      });

      it("a tampered message1 is refused, whichever byte was changed", () => {
        const genuine = hexToBytes(v.message1);
        for (const index of [0, 31, 32, 60, 79, 80, 100, genuine.length - 17, genuine.length - 1]) {
          const { responder } = pair(v);
          const bad = genuine.slice();
          bad[index] = (bad[index] as number) ^ 0x01;
          expect(() => responder.readMessage(bad), `byte ${index}`).toThrowError(NoiseError);
          expect(() => responder.writeMessage(), "a failed handshake stays failed").toThrowError(NoiseError);
        }
      });

      it("a truncated or extended message1 and message2 are refused", () => {
        const m1 = hexToBytes(v.message1);
        for (const bad of [m1.subarray(0, 0), m1.subarray(0, 31), m1.subarray(0, 80), m1.subarray(0, m1.length - 1), new Uint8Array([...m1, 0])]) {
          expect(() => pair(v).responder.readMessage(bad)).toThrowError(NoiseError);
        }
        const m2 = hexToBytes(v.message2);
        for (const bad of [m2.subarray(0, 0), m2.subarray(0, 40), m2.subarray(0, m2.length - 1), new Uint8Array([...m2, 0])]) {
          const { initiator } = pair(v);
          initiator.writeMessage(utf8Encode(v.payload1));
          expect(() => initiator.readMessage(bad)).toThrowError(NoiseError);
        }
      });

      it("a tampered transport frame is refused and the genuine one still opens after it", () => {
        const { device, gateway } = complete(v);
        const [frame] = seal(device.send, utf8Encode("one")) as [Uint8Array];
        for (let index = 0; index < frame.length; index++) {
          const bad = frame.slice();
          bad[index] = (bad[index] as number) ^ 0x80;
          expect(() => new Opener(64).open(gateway.receive, bad), `byte ${index}`).toThrowError(NoiseError);
        }
        expect(utf8Decode(new Opener(64).open(gateway.receive, frame) as Uint8Array)).toBe("one");
      });

      it("a replayed transport frame is refused", () => {
        const { device, gateway } = complete(v);
        const [frame] = seal(device.send, utf8Encode("once")) as [Uint8Array];
        expect(new Opener(64).open(gateway.receive, frame)).not.toBeNull();
        expect(() => new Opener(64).open(gateway.receive, frame)).toThrowError(NoiseError);
      });

      it("reordered transport frames are refused", () => {
        const { device, gateway } = complete(v);
        const [first] = seal(device.send, utf8Encode("first")) as [Uint8Array];
        const [second] = seal(device.send, utf8Encode("second")) as [Uint8Array];
        expect(() => new Opener(64).open(gateway.receive, second)).toThrowError(NoiseError);
        expect(utf8Decode(new Opener(64).open(gateway.receive, first) as Uint8Array)).toBe("first");
        expect(utf8Decode(new Opener(64).open(gateway.receive, second) as Uint8Array)).toBe("second");
      });

      it("truncated transport frames are refused", () => {
        const { device, gateway } = complete(v);
        const [frame] = seal(device.send, utf8Encode("whole")) as [Uint8Array];
        for (const length of [0, 1, 16, 17, frame.length - 1]) {
          expect(() => new Opener(64).open(gateway.receive, frame.subarray(0, length)), `length ${length}`).toThrowError();
        }
        expect(new Opener(64).open(gateway.receive, frame)).not.toBeNull();
      });

      it("a frame sent in one direction does not open in the other", () => {
        const { device, gateway } = complete(v);
        const [frame] = seal(device.send, utf8Encode("to the gateway")) as [Uint8Array];
        expect(() => new Opener(64).open(device.receive, frame)).toThrowError(NoiseError);
        expect(new Opener(64).open(gateway.receive, frame)).not.toBeNull();
      });
    });
  }
});

describe("pairing", () => {
  const v = shared.vectors[1] as Vector;
  const wrong = pskFromSecret(hexToBytes("000102030405060708090a0b0c0d0e0f"));

  function initiatorWith(psk: Uint8Array): Handshake {
    return new Handshake({
      kind: "pairing",
      role: "initiator",
      staticPrivateKey: hexToBytes(v.initiator_static_private),
      remoteStaticPublicKey: hexToBytes(v.responder_static_public),
      ephemeralPrivateKey: hexToBytes(v.initiator_ephemeral_private),
      psk,
      random,
    });
  }

  it("is IKpsk1: the pattern name says so", () => {
    expect(PATTERN_PAIRING).toBe("Noise_IKpsk1_25519_ChaChaPoly_SHA256");
    expect(v.pattern).toBe(PATTERN_PAIRING);
  });

  it("a gateway with the right secret cannot read message 1 of a phone with a wrong secret", () => {
    const { responder } = pair(v);
    const message1 = initiatorWith(wrong).writeMessage(utf8Encode(v.payload1));
    expect(message1.length).toBe(hexToBytes(v.message1).length);
    expect(() => responder.readMessage(message1)).toThrowError(NoiseError);
    // Nothing of the handshake is usable after that: no answer can be written.
    expect(() => responder.writeMessage(utf8Encode(v.payload2))).toThrowError(NoiseError);
    expect(() => responder.split()).toThrowError(NoiseError);
  });

  it("a gateway cannot read message 1 of a phone that used no secret at all (a session handshake)", () => {
    const { responder } = pair(v);
    const sessionInitiator = new Handshake({
      kind: "session",
      role: "initiator",
      staticPrivateKey: hexToBytes(v.initiator_static_private),
      remoteStaticPublicKey: hexToBytes(v.responder_static_public),
      random,
    });
    expect(() => responder.readMessage(sessionInitiator.writeMessage(utf8Encode(v.payload1)))).toThrowError(NoiseError);
  });

  it("a phone with the right secret refuses a gateway that holds a wrong one", () => {
    const { initiator, responder } = pair(v, { responderPsk: wrong });
    expect(() => responder.readMessage(initiator.writeMessage(utf8Encode(v.payload1)))).toThrowError(NoiseError);
  });

  it("the secret changes message 1 from the payload on, and nothing before it", () => {
    const right = bytesToHex(initiatorWith(hexToBytes(v.psk as string)).writeMessage(utf8Encode(v.payload1)));
    const other = bytesToHex(initiatorWith(wrong).writeMessage(utf8Encode(v.payload1)));
    expect(right).toBe(v.message1);
    // e (32 bytes) and the encrypted static key (48 bytes) come before the psk token.
    expect(other.slice(0, 160)).toBe(right.slice(0, 160));
    expect(other.slice(160)).not.toBe(right.slice(160));
  });

  it("requires the pre-shared key, and a session refuses one", () => {
    const key = hexToBytes(v.initiator_static_private);
    const remote = hexToBytes(v.responder_static_public);
    expect(() => new Handshake({ kind: "pairing", role: "initiator", staticPrivateKey: key, remoteStaticPublicKey: remote, random })).toThrowError(NoiseError);
    expect(() => new Handshake({ kind: "pairing", role: "responder", staticPrivateKey: key, random })).toThrowError(NoiseError);
    expect(() => new Handshake({ kind: "session", role: "initiator", staticPrivateKey: key, remoteStaticPublicKey: remote, psk: new Uint8Array(32), random })).toThrowError(NoiseError);
  });
});

describe("handshake rules", () => {
  const v = shared.vectors[0] as Vector;

  it("uses a fresh ephemeral key for every handshake when none is injected", () => {
    const make = () =>
      new Handshake({
        kind: "session",
        role: "initiator",
        staticPrivateKey: hexToBytes(v.initiator_static_private),
        remoteStaticPublicKey: hexToBytes(v.responder_static_public),
        random: seededRandom(99, true),
      });
    const a = bytesToHex(make().writeMessage(utf8Encode("{}")));
    const b = bytesToHex(make().writeMessage(utf8Encode("{}")));
    expect(a.slice(0, 64)).not.toBe(b.slice(0, 64));
    expect(a).not.toBe(v.message1);
  });

  it("refuses messages out of order and use after the end", () => {
    const { initiator, responder } = pair(v);
    expect(() => initiator.readMessage(new Uint8Array(96))).toThrowError(/out of order/);
    expect(() => responder.writeMessage()).toThrowError(/out of order/);
    expect(() => initiator.split()).toThrowError(/not complete/);
    responder.readMessage(initiator.writeMessage());
    initiator.readMessage(responder.writeMessage());
    initiator.split();
    expect(() => initiator.split()).toThrowError(NoiseError);
    expect(() => initiator.writeMessage()).toThrowError(NoiseError);
  });

  it("refuses keys of the wrong size and a public key of low order", () => {
    const key = hexToBytes(v.initiator_static_private);
    expect(() => new Handshake({ kind: "session", role: "initiator", staticPrivateKey: key.subarray(1), remoteStaticPublicKey: key, random })).toThrowError(NoiseError);
    expect(() => new Handshake({ kind: "session", role: "initiator", staticPrivateKey: key, random })).toThrowError(NoiseError);
    const lowOrder = new Handshake({ kind: "session", role: "initiator", staticPrivateKey: key, remoteStaticPublicKey: new Uint8Array(32), random });
    expect(() => lowOrder.writeMessage()).toThrowError(NoiseError);
    expect(() => generateKeyPair(() => new Uint8Array(32))).toThrowError(NoiseError);
    expect(() => generateKeyPair(() => new Uint8Array(31))).toThrowError(NoiseError);
  });

  it("takes the static public key when it is given, and fails safely when it is not the right one", () => {
    const given = (publicKey: Uint8Array) =>
      new Handshake({
        kind: "session",
        role: "initiator",
        staticPrivateKey: hexToBytes(v.initiator_static_private),
        staticPublicKey: publicKey,
        remoteStaticPublicKey: hexToBytes(v.responder_static_public),
        ephemeralPrivateKey: hexToBytes(v.initiator_ephemeral_private),
        random,
      });
    expect(bytesToHex(given(hexToBytes(v.initiator_static_public)).writeMessage(utf8Encode(v.payload1)))).toBe(v.message1);
    const wrong = given(generateKeyPair(seededRandom(12)).publicKey).writeMessage(utf8Encode(v.payload1));
    expect(() => pair(v).responder.readMessage(wrong)).toThrowError(NoiseError);
    expect(() => given(new Uint8Array(31))).toThrowError(NoiseError);
  });

  it("keeps secrets out of error messages", () => {
    const secrets = [v.initiator_static_private, v.responder_static_private, v.initiator_ephemeral_private];
    const { responder } = pair(v);
    const bad = hexToBytes(v.message1);
    bad[50] = (bad[50] as number) ^ 1;
    try {
      responder.readMessage(bad);
      expect.unreachable();
    } catch (error) {
      const text = String(error) + JSON.stringify(error);
      for (const secret of secrets) expect(text).not.toContain(secret);
      expect(text).not.toContain(v.payload1);
    }
  });
});

describe("cipher state", () => {
  const key = hexToBytes("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");

  it("starts at nonce zero and counts every message", () => {
    const tx = new CipherState(key);
    expect(tx.nonce).toEqual({ high: 0, low: 0 });
    tx.encrypt(utf8Encode("a"));
    tx.encrypt(utf8Encode("b"));
    expect(tx.nonce).toEqual({ high: 0, low: 2 });
  });

  it("never produces the same ciphertext twice for the same plaintext", () => {
    const tx = new CipherState(key);
    const seen = new Set<string>();
    for (let i = 0; i < 200; i++) seen.add(bytesToHex(tx.encrypt(utf8Encode("same"))));
    expect(seen.size).toBe(200);
  });

  it("carries the counter from the low half into the high half", () => {
    const tx = new CipherState(key, { high: 0, low: 0xffff_ffff });
    const rx = new CipherState(key, { high: 0, low: 0xffff_ffff });
    expect(utf8Decode(rx.decrypt(tx.encrypt(utf8Encode("edge"))))).toBe("edge");
    expect(tx.nonce).toEqual({ high: 1, low: 0 });
    expect(utf8Decode(rx.decrypt(tx.encrypt(utf8Encode("next"))))).toBe("next");
  });

  it("refuses to use the last nonce, in both directions", () => {
    const tx = new CipherState(key, { high: 0xffff_ffff, low: 0xffff_fffe });
    const rx = new CipherState(key, { high: 0xffff_ffff, low: 0xffff_fffe });
    const last = tx.encrypt(utf8Encode("the last message"));
    expect(utf8Decode(rx.decrypt(last))).toBe("the last message");
    expect(() => tx.encrypt(utf8Encode("one too many"))).toThrowError(/as many messages/);
    expect(() => rx.decrypt(last)).toThrowError(/as many messages/);
    expect(tx.nonce).toEqual({ high: 0xffff_ffff, low: 0xffff_ffff });
  });

  it("binds the associated data and forgets a destroyed key", () => {
    const tx = new CipherState(key);
    const rx = new CipherState(key);
    const sealed = tx.encrypt(utf8Encode("x"), utf8Encode("context"));
    expect(() => rx.decrypt(sealed, utf8Encode("another"))).toThrowError(NoiseError);
    expect(utf8Decode(rx.decrypt(sealed, utf8Encode("context")))).toBe("x");
    tx.destroy();
    expect(() => tx.encrypt(utf8Encode("x"))).toThrowError(/destroyed/);
  });
});
