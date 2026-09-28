import { describe, expect, it } from "vitest";
import { bytesToHex, concatBytes, equalBytes, hexToBytes, utf8Decode, utf8DecodePure, utf8Encode, utf8EncodePure, wipe } from "../src/bytes.ts";
import { prng } from "./helpers.ts";

describe("bytes", () => {
  it("hex round trips and refuses what is not hex", () => {
    const bytes = new Uint8Array(256).map((_, i) => i);
    expect(hexToBytes(bytesToHex(bytes))).toEqual(bytes);
    expect(hexToBytes("00FFaB")).toEqual(Uint8Array.of(0, 255, 0xab));
    expect(() => hexToBytes("abc")).toThrowError(/odd/);
    expect(() => hexToBytes("zz")).toThrowError(/not hex/);
  });

  it("compares without an early exit and wipes", () => {
    expect(equalBytes(Uint8Array.of(1, 2, 3), Uint8Array.of(1, 2, 3))).toBe(true);
    expect(equalBytes(Uint8Array.of(1, 2, 3), Uint8Array.of(1, 2, 4))).toBe(false);
    expect(equalBytes(Uint8Array.of(1, 2, 3), Uint8Array.of(1, 2))).toBe(false);
    expect(equalBytes(new Uint8Array(0), new Uint8Array(0))).toBe(true);
    const secret = Uint8Array.of(9, 9, 9);
    wipe(secret, null, undefined);
    expect(secret).toEqual(new Uint8Array(3));
    expect(concatBytes(Uint8Array.of(1), new Uint8Array(0), Uint8Array.of(2, 3))).toEqual(Uint8Array.of(1, 2, 3));
  });
});

describe("UTF-8 in plain JavaScript (what runs where the platform has no codec)", () => {
  const samples = ["", "plain", "é", "€", "😀", "a\u0000b", "﻿mark", "日本語のテキスト", "mixed é € 😀 end", "퟿￿", "\u{10000}\u{10ffff}"];

  it("encodes as the platform does", () => {
    const platform = new TextEncoder();
    for (const text of [...samples, "lone \ud800 high", "lone \udc00 low", "\ud83d"]) {
      expect(bytesToHex(utf8EncodePure(text)), JSON.stringify(text)).toBe(bytesToHex(platform.encode(text)));
      expect(bytesToHex(utf8Encode(text))).toBe(bytesToHex(platform.encode(text)));
    }
  });

  it("decodes as the platform does", () => {
    const platform = new TextEncoder();
    for (const text of samples) {
      expect(utf8DecodePure(platform.encode(text))).toBe(text);
      expect(utf8Decode(platform.encode(text))).toBe(text);
    }
  });

  it("round trips long and random text, across the internal buffer edges", () => {
    const next = prng(5);
    let text = "";
    while (text.length < 40_000) {
      const r = next();
      if (r < 0.6) text += String.fromCharCode(32 + Math.floor(next() * 95));
      else if (r < 0.8) text += String.fromCharCode(0xa0 + Math.floor(next() * 0x700));
      else if (r < 0.95) text += String.fromCharCode(0x800 + Math.floor(next() * 0xc000));
      else text += String.fromCodePoint(0x10000 + Math.floor(next() * 0xfffff));
    }
    text = text.replace(/[\ud800-\udfff]/g, (c, i: number, all: string) => {
      const code = c.charCodeAt(0);
      const paired = code < 0xdc00 ? /[\udc00-\udfff]/.test(all.charAt(i + 1)) : /[\ud800-\udbff]/.test(all.charAt(i - 1));
      return paired ? c : "?";
    });
    const bytes = new TextEncoder().encode(text);
    expect(bytesToHex(utf8EncodePure(text))).toBe(bytesToHex(bytes));
    expect(utf8DecodePure(bytes)).toBe(text);
  });

  it("refuses bytes that are not valid UTF-8, exactly where the strict platform decoder does", () => {
    const strict = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
    const bad: number[][] = [
      [0xff],
      [0x80],
      [0xc0, 0x80],
      [0xc1, 0xbf],
      [0xc3],
      [0xc3, 0x28],
      [0xe0, 0x80, 0x80],
      [0xe0, 0x9f, 0xbf],
      [0xe2, 0x82],
      [0xed, 0xa0, 0x80],
      [0xed, 0xbf, 0xbf],
      [0xf0, 0x80, 0x80, 0x80],
      [0xf0, 0x8f, 0xbf, 0xbf],
      [0xf4, 0x90, 0x80, 0x80],
      [0xf5, 0x80, 0x80, 0x80],
      [0xf0, 0x9f, 0x98],
      [0x61, 0xf0, 0x9f, 0x98, 0x61],
    ];
    for (const bytes of bad) {
      const array = Uint8Array.from(bytes);
      expect(() => strict.decode(array), bytesToHex(array)).toThrowError();
      expect(() => utf8DecodePure(array), bytesToHex(array)).toThrowError(/not valid UTF-8/);
      expect(() => utf8Decode(array), bytesToHex(array)).toThrowError(/not valid UTF-8/);
    }
    const next = prng(11);
    for (let round = 0; round < 3000; round++) {
      const array = new Uint8Array(1 + Math.floor(next() * 6)).map(() => Math.floor(next() * 256));
      let expected: string | null;
      try {
        expected = strict.decode(array);
      } catch {
        expected = null;
      }
      if (expected === null) expect(() => utf8DecodePure(array), bytesToHex(array)).toThrowError();
      else expect(utf8DecodePure(array), bytesToHex(array)).toBe(expected);
    }
  });
});
