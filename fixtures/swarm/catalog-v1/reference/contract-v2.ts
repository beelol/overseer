import type { CatalogRow } from "./data.ts";

export type CatalogPage = {
  data: CatalogRow[];
  meta: { hasMore: boolean; limit: number; nextCursor: string | null };
};

// The stable key is (createdAt, id), both descending. Rows inserted before an
// existing cursor cannot shift older rows onto a different page.
export function cursorPage(rows: CatalogRow[], request: URLSearchParams): CatalogPage {
  if (request.has("page") || request.has("offset")) {
    throw new Error("offset parameters are no longer supported");
  }
  const limit = Number(request.get("limit") || 2);
  if (!Number.isInteger(limit) || limit < 1 || limit > 20) {
    throw new Error("invalid limit");
  }
  const encoded = request.get("cursor");
  if (encoded && (encoded.length > 256 || !/^[A-Za-z0-9_-]+$/.test(encoded))) {
    throw new Error("invalid cursor encoding");
  }
  const after: [number, string] | null = encoded ?
    JSON.parse(Buffer.from(encoded, "base64url").toString("utf8")) : null;
  if (after && (!Array.isArray(after) || after.length !== 2 ||
      !Number.isSafeInteger(after[0]) || typeof after[1] !== "string" ||
      after[1].length === 0 || after[1].length > 128)) {
    throw new Error("invalid cursor tuple");
  }
  const ordered = [...rows].sort((a, b) =>
    b.createdAt - a.createdAt || b.id.localeCompare(a.id));
  const candidates = after === null ? ordered : ordered.filter((row) =>
    row.createdAt < after[0] || (row.createdAt === after[0] && row.id < after[1]));
  const data = candidates.slice(0, limit);
  const hasMore = candidates.length > limit;
  return {
    data,
    meta: {
      hasMore,
      limit,
      nextCursor: hasMore
        ? Buffer.from(JSON.stringify([data.at(-1)!.createdAt, data.at(-1)!.id])).toString("base64url")
        : null
    }
  };
}
