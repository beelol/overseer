import type { CatalogRow } from "./data.ts";

export type CatalogPage = {
  data: CatalogRow[];
  meta: { hasMore: boolean; limit: number; nextCursor: string | null };
};

export function offsetPage(rows: CatalogRow[], request: URLSearchParams): CatalogPage {
  const limit = Math.min(20, Math.max(1, Number(request.get("limit") || 2)));
  const page = Math.max(1, Number(request.get("page") || 1));
  const offset = (page - 1) * limit;
  const ordered = [...rows].sort((a, b) => b.createdAt - a.createdAt);
  const data = ordered.slice(offset, offset + limit);
  return {
    data,
    meta: {
      hasMore: offset + limit < ordered.length,
      limit,
      nextCursor: offset + limit < ordered.length ? String(page + 1) : null
    }
  };
}
