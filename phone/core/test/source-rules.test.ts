/**
 * The rules for `src/`: it runs unchanged in Node 24 and in React Native's Hermes, so it may
 * import nothing of a platform and use only syntax that disappears when the types are removed.
 */

import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const root = new URL("../", import.meta.url);
const files = readdirSync(new URL("src/", root)).filter((name) => name.endsWith(".ts"));
const sources = files.map((name) => ({ name, text: readFileSync(new URL(`src/${name}`, root), "utf8") }));

/** The code without comments and without the content of strings. */
function codeOf(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|[^:])\/\/.*$/gm, "$1")
    .replace(/"(?:[^"\\\n]|\\.)*"/g, '""')
    .replace(/`(?:[^`\\]|\\.)*`/g, "``");
}

describe("the source", () => {
  it("has the modules of the package", () => {
    for (const name of ["noise.ts", "frames.ts", "pairing-code.ts", "socket.ts", "session.ts", "client.ts", "index.ts"]) expect(files).toContain(name);
  });

  it("imports only its own modules, the three noble packages and the generated protocol", () => {
    const allowed = /^(\.\/[a-z0-9-]+\.ts|@noble\/(curves|ciphers|hashes)\/[a-z0-9]+\.js)$/;
    const generated = "../../protocol/protocol.generated.ts";
    const users: string[] = [];
    for (const { name, text } of sources) {
      const specifiers = [...text.matchAll(/(?:from|import)\s*\(?\s*"([^"]+)"/g)].map((m) => m[1] as string);
      for (const specifier of specifiers) {
        if (specifier === generated) users.push(name);
        else expect(specifier, `${name} imports ${specifier}`).toMatch(allowed);
      }
      expect(text, name).not.toMatch(/\brequire\s*\(/);
    }
    expect(users.sort()).toEqual(["client-types.ts", "client.ts"]);
    // The generated file obeys the same rules: it imports nothing and is only types and constants.
    const text = readFileSync(new URL(`src/${generated}`, root), "utf8");
    expect(text).not.toMatch(/^\s*import\s/m);
    expect(codeOf(text)).not.toMatch(/\benum\s+[A-Za-z]|\bnamespace\s+[A-Za-z]|\bclass\s+[A-Za-z]|\bfunction\b/);
  });

  it("declares exactly the dependencies that were agreed", () => {
    const manifest = JSON.parse(readFileSync(new URL("package.json", root), "utf8")) as { name: string; private: boolean; type: string; dependencies: object; devDependencies: object };
    expect([manifest.name, manifest.private, manifest.type]).toEqual(["@overseer/phone-core", true, "module"]);
    expect(Object.keys(manifest.dependencies).sort()).toEqual(["@noble/ciphers", "@noble/curves", "@noble/hashes"]);
    expect(Object.keys(manifest.devDependencies).sort()).toEqual(["@types/node", "@types/ws", "typescript", "vitest", "ws"]);
  });

  it("uses nothing of a platform by name", () => {
    for (const { name, text } of sources) {
      const code = codeOf(text);
      for (const word of [/\bprocess\./, /\bBuffer\b/, /\b__dirname\b/, /\bwindow\./, /\bdocument\./, /\bnavigator\./, /\blocalStorage\b/, /\bconsole\./, /\bcrypto\./, /\bMath\.random\b/, /\bDate\.now\b/, /\bnew Date\b/]) {
        expect(code, `${name} uses ${String(word)}`).not.toMatch(word);
      }
      // Timers and codecs are reached through `globalThis`, in two places only.
      if (code.includes("globalThis")) expect(["bytes.ts", "platform.ts"]).toContain(name);
      if (/\bsetTimeout\b|\bWebSocket\b\s*\(/.test(code)) expect(name).toBe("platform.ts");
    }
  });

  it("uses only syntax that disappears with the types", () => {
    for (const { name, text } of sources) {
      const code = codeOf(text);
      expect(code, `${name}: enum`).not.toMatch(/\benum\s+[A-Za-z]/);
      expect(code, `${name}: namespace`).not.toMatch(/\b(namespace|module)\s+[A-Za-z_]+\s*\{/);
      expect(code, `${name}: decorator`).not.toMatch(/^\s*@[A-Za-z]/m);
      // A parameter property is a modifier in front of a parameter's name; `x: readonly T[]` is a type.
      expect(code, `${name}: parameter property`).not.toMatch(/constructor\s*\((?:[^()]*,)?\s*(public|private|protected|readonly)\s+[A-Za-z_]/);
      expect(code, `${name}: declare field`).not.toMatch(/^\s*(?:private |protected |public )?declare\s/m);
      expect(code, `${name}: import =`).not.toMatch(/\bimport\s+[A-Za-z_]+\s*=/);
      expect(code, `${name}: any`).not.toMatch(/(:|<|\bas)\s*any\b|\bany\[\]/);
    }
    const config = JSON.parse(readFileSync(new URL("tsconfig.json", root), "utf8")) as { compilerOptions: Record<string, unknown> };
    expect(config.compilerOptions).toMatchObject({ strict: true, erasableSyntaxOnly: true, types: [], lib: ["ES2022"] });
  });

  it("leaves nothing unfinished", () => {
    for (const { name, text } of sources) expect(text, name).not.toMatch(/\b(TODO|FIXME|XXX|HACK)\b/);
    for (const name of readdirSync(new URL("test/", root))) {
      const text = readFileSync(new URL(`test/${name}`, root), "utf8");
      if (name !== "source-rules.test.ts") expect(text, name).not.toMatch(/\b(it|describe|test)\.(skip|only|todo)\b|\bxit\(|\bTODO\b/);
    }
  });

  it("never writes a key, a secret or message content to the log or into an error", () => {
    /** The arguments of the call that opens at `from`, up to its closing parenthesis. */
    const argumentsAt = (code: string, from: number): string => {
      let depth = 1;
      let quote = "";
      for (let i = from; i < code.length; i++) {
        const c = code.charAt(i);
        if (quote) {
          if (c === "\\") i++;
          else if (c === quote) quote = "";
        } else if (c === '"' || c === "`") quote = c;
        else if (c === "(") depth++;
        else if (c === ")" && --depth === 0) return code.slice(from, i);
      }
      throw new Error("a call without an end");
    };
    /** What a message is made of: the expressions in it, without the words of its fixed text. */
    const expressions = (argument: string): string =>
      argument
        .replace(/"(?:[^"\\\n]|\\.)*"/g, '""')
        .replace(/`(?:[^`\\]|\\.)*`/g, (template) => [...template.matchAll(/\$\{([^}]*)\}/g)].map((m) => m[1]).join(" ; "));
    const forbidden = /[Pp]rivateKey|[Pp]ublicKey|[Ss]ecret|psk|bytesToHex|[Pp]ayload|plaintext|params|JSON\.stringify|utf8Decode|(?<![.\w])code\b|\bvalue\b|\bframe\b|\bmessage1\b/;
    expect(expressions('`pairing ${code} failed`')).toMatch(forbidden);
    expect(expressions('"the secret is wrong", `${error.code}; again`')).not.toMatch(forbidden);

    let checked = 0;
    for (const { name, text } of sources) {
      const code = text.replace(/\/\*[\s\S]*?\*\//g, "");
      for (const call of code.matchAll(/log\?\.\(|new [A-Za-z]*Error\(/g)) {
        const said = argumentsAt(code, (call.index as number) + call[0].length);
        checked += 1;
        expect(expressions(said), `${name}: ${said}`).not.toMatch(forbidden);
      }
    }
    expect(checked).toBeGreaterThan(60);
  });
});

describe("Node 24 runs the source as it is", () => {
  it("imports src/index.ts without a build step and completes both handshakes", () => {
    const script = `
      import { Handshake, generateKeyPair, pskFromSecret, seal, Opener, encodePairingCode, decodePairingCode, PhoneClient, webSocketFactory } from ${JSON.stringify(new URL("src/index.ts", root).href)};
      import { randomBytes } from "node:crypto";
      const random = (n) => new Uint8Array(randomBytes(n));
      const out = [];
      for (const kind of ["session", "pairing"]) {
        const device = generateKeyPair(random), gateway = generateKeyPair(random);
        const psk = kind === "pairing" ? { psk: pskFromSecret(random(16)) } : {};
        const i = new Handshake({ kind, role: "initiator", staticPrivateKey: device.privateKey, remoteStaticPublicKey: gateway.publicKey, random, ...psk });
        const r = new Handshake({ kind, role: "responder", staticPrivateKey: gateway.privateKey, random, ...psk });
        r.readMessage(i.writeMessage(new Uint8Array(10)));
        i.readMessage(r.writeMessage(new Uint8Array(10)));
        const a = i.split(), b = r.split();
        const opener = new Opener(1 << 20);
        let joined = null;
        for (const frame of seal(a.send, new Uint8Array(70000))) joined = opener.open(b.receive, frame);
        out.push(kind + ":" + joined.length);
      }
      const code = encodePairingCode({ gatewayPublicKey: random(32), secret: random(16), port: 47810, addresses: ["127.0.0.1"] });
      out.push(decodePairingCode(code).port, typeof PhoneClient, typeof webSocketFactory(WebSocket));
      console.log(out.join(" "));
    `;
    // The generated protocol file is in the app's package, which does not name its module kind;
    // Node says so once. That warning is the only one silenced.
    const output = execFileSync(process.execPath, ["--disable-warning=MODULE_TYPELESS_PACKAGE_JSON", "--input-type=module", "--eval", script], { encoding: "utf8", cwd: new URL(".", root) });
    expect(output.trim()).toBe("session:70000 pairing:70000 47810 function function");
    expect(Number(process.versions.node.split(".")[0])).toBeGreaterThanOrEqual(24);
  });
});
