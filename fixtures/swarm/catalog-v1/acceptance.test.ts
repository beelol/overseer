import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { once } from "node:events";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createCatalogServer } from "./src/server.ts";

const root = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const names: string[] = manifest.resource_modules;

test("fixture pins 24 distinct TypeScript resource modules", () => {
  assert.equal(manifest.version, 1);
  assert.equal(names.length, 24);
  assert.equal(new Set(names).size, 24);
  const actual = readdirSync(join(root, "src/resources"))
    .filter((name) => name.endsWith(".ts") && name !== "index.ts")
    .map((name) => name.slice(0, -3)).sort();
  assert.deepEqual(actual, [...names].sort());
});

test("all Catalog routes preserve shape and cursor stability across a tied key and insertion", async (t) => {
  const server = createCatalogServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => new Promise<void>((resolve, reject) =>
    server.close((error) => error ? reject(error) : resolve())));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const base = `http://127.0.0.1:${address.port}`;

  for (const resource of names) {
    await t.test(resource, async () => {
      await fetch(`${base}/__fixture/reset?resource=${resource}`);
      const firstResponse = await fetch(`${base}/api/${resource}?limit=2`);
      assert.equal(firstResponse.status, 200);
      const first = await firstResponse.json();
      assert.deepEqual(Object.keys(first).sort(), ["data", "meta"]);
      assert.deepEqual(Object.keys(first.meta).sort(), ["hasMore", "limit", "nextCursor"]);
      assert.deepEqual(first.data.map((row: { id: string }) => row.id).sort(),
        [`${resource}-002`, `${resource}-003`]);
      assert.equal(first.meta.hasMore, true);
      assert.equal(first.meta.limit, 2);
      assert.equal(typeof first.meta.nextCursor, "string");

      const inserted = await fetch(`${base}/__fixture/insert?resource=${resource}&id=${resource}-new&createdAt=110`);
      assert.equal(inserted.status, 200);
      const secondResponse = await fetch(`${base}/api/${resource}?limit=2&cursor=${encodeURIComponent(first.meta.nextCursor)}`);
      assert.equal(secondResponse.status, 200);
      const second = await secondResponse.json();
      assert.deepEqual(Object.keys(second).sort(), ["data", "meta"]);
      assert.deepEqual(Object.keys(second.meta).sort(), ["hasMore", "limit", "nextCursor"]);
      assert.deepEqual(second.data.map((row: { id: string }) => row.id),
        [`${resource}-001`, `${resource}-004`]);
      assert.equal(second.meta.hasMore, true);
      assert.equal(typeof second.meta.nextCursor, "string");
      const thirdResponse = await fetch(`${base}/api/${resource}?limit=2&cursor=${encodeURIComponent(second.meta.nextCursor)}`);
      assert.equal(thirdResponse.status, 200);
      const third = await thirdResponse.json();
      assert.deepEqual(third.data.map((row: { id: string }) => row.id), [`${resource}-005`]);
      assert.equal(third.meta.hasMore, false);
      assert.equal(third.meta.nextCursor, null);
      assert.deepEqual(first.data.map((row: { id: string }) => row.id),
        [`${resource}-003`, `${resource}-002`], "equal sort keys need a stable id tie-breaker");
    });
  }
  for (const query of ["page=2", "limit=0", "cursor=not-a-cursor"]) {
    const invalid = await fetch(`${base}/api/${names[0]}?${query}`);
    assert.equal(invalid.status, 400, `invalid pagination request was accepted: ${query}`);
  }
});
