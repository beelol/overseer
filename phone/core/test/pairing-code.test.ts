import { describe, expect, it } from "vitest";
import { base32Decode, base32Encode, Base32Error } from "../src/base32.ts";
import { bytesToHex, concatBytes, utf8Encode } from "../src/bytes.ts";
import { decodePairingCode, encodePairingCode, type PairingCode, PairingCodeError, type PairingCodeProblem } from "../src/pairing-code.ts";

const key = new Uint8Array(32).map((_, i) => i);
const secret = new Uint8Array(16).fill(7);
const sample: PairingCode = { gatewayPublicKey: key, secret, port: 47_810, addresses: ["192.168.1.20", "127.0.0.1"] };

function layout(parts: { version?: number; key?: Uint8Array; secret?: Uint8Array; port?: number; count?: number; addresses?: (string | Uint8Array)[]; tail?: Uint8Array }): Uint8Array {
  const addresses = parts.addresses ?? ["10.0.0.1"];
  const port = parts.port ?? 47_810;
  const encoded = addresses.map((a) => {
    const bytes = typeof a === "string" ? utf8Encode(a) : a;
    return concatBytes(Uint8Array.of(bytes.length), bytes);
  });
  return concatBytes(
    Uint8Array.of(parts.version ?? 1),
    parts.key ?? key,
    parts.secret ?? secret,
    Uint8Array.of(port >> 8, port & 0xff),
    Uint8Array.of(parts.count ?? addresses.length),
    ...encoded,
    parts.tail ?? new Uint8Array(0),
  );
}

function problemOf(text: string): PairingCodeProblem {
  try {
    decodePairingCode(text);
  } catch (error) {
    expect(error).toBeInstanceOf(PairingCodeError);
    expect((error as PairingCodeError).code).toBe("bad_pairing_code");
    return (error as PairingCodeError).problem;
  }
  throw new Error("the code was accepted");
}

describe("base32", () => {
  it("matches the vectors of RFC 4648, without padding", () => {
    const cases: [string, string][] = [["", ""], ["f", "MY"], ["fo", "MZXQ"], ["foo", "MZXW6"], ["foob", "MZXW6YQ"], ["fooba", "MZXW6YTB"], ["foobar", "MZXW6YTBOI"]];
    for (const [plain, encoded] of cases) {
      expect(base32Encode(utf8Encode(plain))).toBe(encoded);
      expect(base32Decode(encoded)).toEqual(utf8Encode(plain));
      expect(base32Decode(encoded.toLowerCase())).toEqual(utf8Encode(plain));
    }
  });

  it("round trips every length, as the daemon's test does", () => {
    for (let n = 0; n < 70; n++) {
      const bytes = new Uint8Array(n).map((_, i) => (i * 37 + 11) & 0xff);
      expect(base32Decode(base32Encode(bytes))).toEqual(bytes);
    }
  });

  it("refuses characters, lengths and leftover bits that no encoder writes", () => {
    for (const bad of ["MZXW1", "MZXW8", "MZ=W", "MZ W", "MZ-W", "M0"]) expect(() => base32Decode(bad), bad).toThrowError(Base32Error);
    for (const bad of ["M", "MZX", "MZXW6Y"]) expect(() => base32Decode(bad), bad).toThrowError(/length/);
    expect(() => base32Decode("MZ")).toThrowError(/bits/);
    expect(() => base32Decode("MZXR")).toThrowError(/bits/);
  });
});

describe("pairing code", () => {
  it("writes the layout of the specification, as the daemon's test reads it", () => {
    const text = encodePairingCode(sample);
    expect(text.startsWith("OVSR1-")).toBe(true);
    expect(text.slice(6)).toMatch(/^[A-Z2-7]+$/);
    const bytes = base32Decode(text.slice(6));
    expect(bytes[0]).toBe(1);
    expect(bytes.subarray(1, 33)).toEqual(key);
    expect(bytes.subarray(33, 49)).toEqual(secret);
    expect(((bytes[49] as number) << 8) | (bytes[50] as number)).toBe(47_810);
    expect(bytes[51]).toBe(2);
    expect(bytes[52]).toBe("192.168.1.20".length);
    expect(bytes.subarray(53, 65)).toEqual(utf8Encode("192.168.1.20"));
    expect(bytes.length).toBe(53 + 12 + 1 + 9);
  });

  it("round trips", () => {
    const cases: PairingCode[] = [
      sample,
      { ...sample, addresses: [] },
      { ...sample, port: 1, addresses: ["a"] },
      { ...sample, port: 65_535, addresses: ["fe80::1%en0", "fd12:3456::1", "bilals-mac.local", "10.0.2.2", "my_mac-2.lan"] },
      { ...sample, addresses: ["x".repeat(255)] },
      { ...sample, addresses: Array.from({ length: 255 }, (_, i) => `10.0.${i}.1`) },
    ];
    for (const code of cases) {
      const decoded = decodePairingCode(encodePairingCode(code));
      expect(bytesToHex(decoded.gatewayPublicKey)).toBe(bytesToHex(code.gatewayPublicKey));
      expect(bytesToHex(decoded.secret)).toBe(bytesToHex(code.secret));
      expect(decoded.port).toBe(code.port);
      expect(decoded.addresses).toEqual(code.addresses);
    }
  });

  it("accepts lowercase, spaces, line breaks and extra hyphens", () => {
    const text = encodePairingCode(sample);
    const grouped = encodePairingCode(sample, { group: 4 });
    expect(grouped).toMatch(/^OVSR1-([A-Z2-7]{4}-)+[A-Z2-7]{1,4}$/);
    const variants = [
      text.toLowerCase(),
      grouped,
      grouped.toLowerCase(),
      `  ${text}  `,
      text.replace(/(.{5})/g, "$1 "),
      text.replace(/(.{6})/g, "$1-\n"),
      `ovsr1 ${text.slice(6).replace(/(.{3})/g, "$1--")}`,
      `OVSR-1-${text.slice(6)}`,
      `\tOVSR1${text.slice(6)}\r\n`,
    ];
    for (const variant of variants) expect(decodePairingCode(variant), variant).toEqual(decodePairingCode(text));
    expect(decodePairingCode(text).addresses).toEqual(sample.addresses);
  });

  it("refuses text that is not a pairing code", () => {
    for (const text of ["", "   ", "hello", "OVS", "OVSR", "OVSRX-AAAA", "XVSR1-" + encodePairingCode(sample).slice(6), "https://example.com/pair"]) {
      expect(problemOf(text), text).toBe("not_a_pairing_code");
    }
  });

  it("refuses another version, in the prefix and in the first byte", () => {
    const body = encodePairingCode(sample).slice(6);
    expect(problemOf(`OVSR2-${body}`)).toBe("unsupported_version");
    expect(problemOf(`OVSR0-${body}`)).toBe("unsupported_version");
    expect(problemOf(`OVSR1-${base32Encode(layout({ version: 2 }))}`)).toBe("unsupported_version");
    expect(problemOf(`OVSR1-${base32Encode(layout({ version: 0 }))}`)).toBe("unsupported_version");
  });

  it("refuses characters that are not base32", () => {
    const text = encodePairingCode(sample);
    for (const bad of ["0", "1", "8", "9", "!", "_", "é", "="]) {
      expect(problemOf(text.slice(0, 20) + bad + text.slice(21)), bad).toBe("bad_character");
    }
  });

  it("refuses wrong lengths: cut short at every point", () => {
    const bytes = layout({ addresses: ["192.168.1.20", "127.0.0.1"] });
    expect(decodePairingCode(`OVSR1-${base32Encode(bytes)}`).addresses.length).toBe(2);
    for (let length = 0; length < bytes.length; length++) {
      expect(problemOf(`OVSR1-${base32Encode(bytes.subarray(0, length))}`), `${length} bytes`).toBe("wrong_length");
    }
    const text = encodePairingCode(sample);
    expect(problemOf(text.slice(0, -1))).toBe("wrong_length");
    expect(problemOf(text.slice(0, 40))).toBe("wrong_length");
    expect(problemOf("OVSR1-")).toBe("wrong_length");
    expect(problemOf("OVSR1")).toBe("wrong_length");
  });

  it("refuses a count of addresses larger than what follows", () => {
    expect(problemOf(`OVSR1-${base32Encode(layout({ count: 2, addresses: ["10.0.0.1"] }))}`)).toBe("wrong_length");
  });

  it("refuses trailing bytes", () => {
    expect(problemOf(`OVSR1-${base32Encode(layout({ tail: Uint8Array.of(0) }))}`)).toBe("trailing_bytes");
    expect(problemOf(`OVSR1-${base32Encode(layout({ tail: utf8Encode("\u000410.0") }))}`)).toBe("trailing_bytes");
    expect(problemOf(`OVSR1-${base32Encode(layout({ count: 0, addresses: ["10.0.0.1"] }))}`)).toBe("trailing_bytes");
    // Leftover bits or characters that cannot belong to any byte.
    expect(problemOf(`${encodePairingCode(sample)}A`)).toBe("wrong_length");
  });

  it("refuses addresses that are empty or not a host", () => {
    const bad: (string | Uint8Array)[] = ["", "a b", "a/b", "ws://x", "host\n", "x@y", Uint8Array.of(0xc3, 0xa9), Uint8Array.of(0x00), "[::1]"];
    for (const address of bad) {
      expect(problemOf(`OVSR1-${base32Encode(layout({ addresses: [address] }))}`), String(address)).toBe("bad_address");
    }
  });

  it("refuses port zero", () => {
    expect(problemOf(`OVSR1-${base32Encode(layout({ port: 0 }))}`)).toBe("bad_port");
  });

  it("refuses to write what does not fit the layout", () => {
    const bad: PairingCode[] = [
      { ...sample, gatewayPublicKey: key.subarray(1) },
      { ...sample, secret: secret.subarray(1) },
      { ...sample, port: 0 },
      { ...sample, port: 65_536 },
      { ...sample, port: 1.5 },
      { ...sample, addresses: [""] },
      { ...sample, addresses: ["x".repeat(256)] },
      { ...sample, addresses: ["not a host"] },
      { ...sample, addresses: Array.from({ length: 256 }, () => "a") },
    ];
    for (const code of bad) {
      try {
        encodePairingCode(code);
        expect.unreachable();
      } catch (error) {
        expect((error as PairingCodeError).problem).toBe("cannot_encode");
      }
    }
  });

  it("never repeats the code or its secret in an error", () => {
    const text = encodePairingCode({ ...sample, secret: new Uint8Array(16).map((_, i) => 0xa0 + i) });
    const broken = [text.slice(0, -3), `${text}AAAA`, text.slice(0, 30) + "1" + text.slice(31), text.replace("OVSR1", "OVSR3")];
    for (const bad of broken) {
      try {
        decodePairingCode(bad);
        expect.unreachable();
      } catch (error) {
        const said = String(error) + JSON.stringify(error);
        expect(said).not.toContain(text.slice(6, 30));
        expect(said).not.toContain(bad.slice(60, 90));
        expect(said.toLowerCase()).not.toContain("a0a1a2a3");
      }
    }
  });
});
