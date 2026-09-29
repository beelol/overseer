export type CatalogRow = { id: string; createdAt: number; name: string };

const rows = new Map<string, CatalogRow[]>();

export function reset(resource: string): void {
  rows.set(resource, [
    { id: `${resource}-001`, createdAt: 100, name: "one" },
    { id: `${resource}-002`, createdAt: 100, name: "two" },
    { id: `${resource}-003`, createdAt: 100, name: "three" },
    { id: `${resource}-004`, createdAt: 90, name: "four" },
    { id: `${resource}-005`, createdAt: 80, name: "five" }
  ]);
}

export function read(resource: string): CatalogRow[] {
  if (!rows.has(resource)) reset(resource);
  return [...rows.get(resource)!];
}

export function insert(resource: string, row: CatalogRow): void {
  if (!rows.has(resource)) reset(resource);
  rows.get(resource)!.push(row);
}
