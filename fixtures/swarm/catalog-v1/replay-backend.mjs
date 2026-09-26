import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const source = dirname(fileURLToPath(import.meta.url));
const fixture = mkdtempSync(join(tmpdir(), "overseer-catalog-v1-"));
const manifest = JSON.parse(readFileSync(join(source, "manifest.json"), "utf8"));
const names = manifest.resource_modules;
const trace = { fixture: manifest.fixture, version: manifest.version, modules: names.length, stages: [] };

function check(stage, expectedPass) {
  const result = spawnSync(process.execPath,
    ["--test", "--test-reporter=tap", join(fixture, "acceptance.test.ts")],
    { encoding: "utf8", timeout: 15000 });
  const output = `${result.stdout || ""}\n${result.stderr || ""}`;
  const passed = result.status === 0;
  assert.equal(passed, expectedPass, `${stage} unexpected test outcome:\n${output.slice(-4000)}`);
  assert.match(output, /fixture pins 24 distinct TypeScript resource modules/);
  if (!expectedPass) {
    assert.match(output, /AssertionError/);
    assert.match(output, /accounts-003/);
  }
  trace.stages.push({ stage, passed, exitCode: result.status });
}

try {
  cpSync(source, fixture, { recursive: true });
  check("offset-baseline", false);

  writeFileSync(join(fixture, "src/pagination.ts"),
    readFileSync(join(source, "reference/contract-v1.ts")));
  const template = readFileSync(join(source, "reference/module.ts.template"), "utf8");
  for (const name of names) {
    writeFileSync(join(fixture, `src/resources/${name}.ts`),
      template.replace("RESOURCE_NAME", name));
  }
  check("cursor-contract-v1-createdAt-only", false);

  writeFileSync(join(fixture, "src/pagination.ts"),
    readFileSync(join(source, "reference/contract-v2.ts")));
  check("cursor-contract-v2-createdAt-plus-id", true);
  process.stdout.write(`${JSON.stringify(trace)}\n`);
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
