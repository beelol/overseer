import type { CatalogRow } from "./data.ts";

export type CatalogPage = {
  data: CatalogRow[];
  meta: { hasMore: boolean; limit: number; nextCursor: string | null };
};

// Deliberately incomplete first contract: equal timestamps are not disambiguated.
export function cursorPage(rows: CatalogRow[], request: URLSearchParams): CatalogPage {
  const limit = Math.min(20, Math.max(1, Number(request.get("limit") || 2)));
  const encoded = request.get("cursor");
  const after = encoded ? Number(Buffer.from(encoded, "base64url").toString("utf8")) : null;
  const ordered = [...rows].sort((a, b) => b.createdAt - a.createdAt);
  const candidates = after === null ? ordered : ordered.filter((row) => row.createdAt < after);
  const data = candidates.slice(0, limit);
  const hasMore = candidates.length > limit;
  return {
    data,
    meta: {
      hasMore,
      limit,
      nextCursor: hasMore ? Buffer.from(String(data.at(-1)!.createdAt)).toString("base64url") : null
    }
  };
}
