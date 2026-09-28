import { createServer } from "node:http";
import { routes } from "./resources/index.ts";
import { insert, reset } from "./data.ts";

export function createCatalogServer() {
  return createServer((request, response) => {
    const url = new URL(request.url || "/", "http://localhost");
    const name = url.pathname.replace(/^\/api\//, "");
    response.setHeader("content-type", "application/json");
    if (url.pathname === "/__fixture/reset") {
      const resource = url.searchParams.get("resource") || "";
      if (!routes.has(resource)) { response.writeHead(404).end(); return; }
      reset(resource);
      response.end(JSON.stringify({ reset: resource }));
    } else if (url.pathname === "/__fixture/insert") {
      const resource = url.searchParams.get("resource") || "";
      const id = url.searchParams.get("id") || "";
      const createdAt = Number(url.searchParams.get("createdAt"));
      if (!routes.has(resource) || !id || !Number.isFinite(createdAt)) {
        response.writeHead(400).end(); return;
      }
      insert(resource, { id, createdAt, name: "inserted" });
      response.end(JSON.stringify({ inserted: id }));
    } else if (url.pathname.startsWith("/api/") && routes.has(name)) {
      try {
        response.end(JSON.stringify(routes.get(name)!(url.searchParams)));
      } catch {
        response.writeHead(400).end(JSON.stringify({ error: "invalid pagination request" }));
      }
    } else {
      response.writeHead(404).end();
    }
  });
}
