// Checks that Hermes accepts the library: `npm run check:hermes`.
//
// The source is bundled, passed through React Native's Babel preset as Metro does, and
// compiled to bytecode by the Hermes compiler. The result of Babel is also run in Node, to
// show that the transforms left the arithmetic of the key exchange intact.
//
// This proves that Hermes accepts the syntax. It does not run the code in Hermes: that needs
// the app on a simulator or a phone.
//
// The tools are the app's own (phone/node_modules) and the bundler that comes with vitest.
// Where they are missing the check says so and is skipped; nothing is installed for it.

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const core = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const app = resolve(core, "..");
const requireCore = createRequire(join(core, "package.json"));
const requireApp = createRequire(join(app, "package.json"));

function skip(why) {
  console.log(`skipped: ${why}`);
  process.exit(0);
}

const compilers = { darwin: "osx-bin/hermesc", linux: "linux64-bin/hermesc", win32: "win64-bin/hermesc.exe" };
const hermesc = join(app, "node_modules/hermes-compiler/hermesc", compilers[process.platform] ?? "");
if (!compilers[process.platform] || !existsSync(hermesc)) skip("the Hermes compiler is not installed (run npm install in phone/)");
let babel, preset, rolldown;
try {
  babel = requireApp("@babel/core");
  preset = requireApp.resolve("@react-native/babel-preset");
} catch {
  skip("React Native's Babel preset is not installed (run npm install in phone/)");
}
try {
  ({ rolldown } = await import(requireCore.resolve("rolldown")));
} catch {
  skip("the bundler is not installed (run npm install in phone/core/)");
}

const out = mkdtempSync(join(tmpdir(), "overseer-hermes-"));
try {
  const entry = join(out, "entry.ts");
  writeFileSync(
    entry,
    `import * as core from ${JSON.stringify(join(core, "src/index.ts"))};
     const random = (n) => { const b = new Uint8Array(n); for (let i = 0; i < n; i++) b[i] = (i * 37 + 11) & 255; return b; };
     const device = core.generateKeyPair(random);
     const gateway = core.generateKeyPair((n) => random(n).map((v) => v ^ 0x5a));
     const seen = [];
     for (const kind of ["session", "pairing"]) {
       const psk = kind === "pairing" ? { psk: core.pskFromSecret(random(16)) } : {};
       const i = new core.Handshake({ kind, role: "initiator", staticPrivateKey: device.privateKey, remoteStaticPublicKey: gateway.publicKey, random, ...psk });
       const r = new core.Handshake({ kind, role: "responder", staticPrivateKey: gateway.privateKey, random, ...psk });
       r.readMessage(i.writeMessage(core.utf8Encode("{}")));
       i.readMessage(r.writeMessage(core.utf8Encode("{}")));
       const a = i.split(), b = r.split();
       const opener = new core.Opener(1 << 20);
       let joined = null;
       for (const frame of core.seal(a.send, new Uint8Array(70000))) joined = opener.open(b.receive, frame);
       seen.push(kind + ":" + joined.length + ":" + core.bytesToHex(a.handshakeHash).length);
     }
     seen.push(typeof core.PhoneClient, core.decodePairingCode(core.encodePairingCode({ gatewayPublicKey: random(32), secret: random(16), port: 47810, addresses: ["10.0.2.2"] })).addresses[0]);
     globalThis.__overseerCheck = seen.join(" ");`,
  );
  const bundle = await rolldown({ input: entry, platform: "neutral", treeshake: false, logLevel: "silent" });
  const { output } = await bundle.generate({ format: "iife" });
  const bundled = output[0].code;
  console.log(`bundled         ${String(bundled.length).padStart(8)} bytes of JavaScript (the library and the three noble packages)`);

  const transformed = babel.transformSync(bundled, {
    babelrc: false,
    configFile: false,
    filename: join(out, "bundle.js"),
    presets: [[preset, { disableImportExportTransform: true, enableBabelRuntime: false }]],
    compact: false,
    caller: { name: "metro", bundler: "metro", platform: "ios" },
  }).code;
  console.log(`after Babel     ${String(transformed.length).padStart(8)} bytes (React Native's preset, as Metro applies it)`);

  const expected = "session:70000:64 pairing:70000:64 function 10.0.2.2";
  delete globalThis.__overseerCheck;
  new Function(transformed)();
  if (globalThis.__overseerCheck !== expected) throw new Error(`the transformed bundle computed "${globalThis.__overseerCheck}" and not "${expected}"`);
  console.log("run in Node     both handshakes, a message of two frames and a pairing code: correct after Babel");

  let failed = false;
  for (const [name, code] of [["after Babel", transformed], ["as written", bundled]]) {
    const file = join(out, name === "after Babel" ? "babel.js" : "plain.js");
    writeFileSync(file, code);
    try {
      execFileSync(hermesc, ["-emit-binary", "-O", "-out", `${file}.hbc`, file], { stdio: "pipe" });
      console.log(`Hermes compiled ${String(statSync(`${file}.hbc`).size).padStart(8)} bytes of bytecode from the bundle ${name}`);
    } catch (error) {
      // Only the bundle after Babel is what the app ships; the other is for information.
      if (name === "after Babel") failed = true;
      console.log(`Hermes REFUSED the bundle ${name}:\n${String(error.stderr).split("\n").slice(0, 10).join("\n")}`);
    }
  }
  if (failed) process.exit(1);
  console.log("ok: Hermes accepts the library. Whether it runs there is for the app's scenarios to show.");
} finally {
  rmSync(out, { recursive: true, force: true });
}
