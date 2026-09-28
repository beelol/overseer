// Two small persistent structures. An update returns a new value that shares everything it did
// not change with the old one, so a reducer never copies a large array or map for one event.
//
//   PMap  a map with text keys in 256 buckets: a change copies one bucket and the 256 slots.
//   PVec  a list in chunks of 64: a change at the end copies one chunk and the list of chunks.

const BUCKETS = 256;
const CHUNK = 64;

/** FNV-1a over UTF-16 code units, folded to a bucket number. */
function bucketOf(key: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) h = Math.imul(h ^ key.charCodeAt(i), 16777619);
  return (h ^ (h >>> 8) ^ (h >>> 16)) & (BUCKETS - 1);
}

type Bucket<V> = { readonly [key: string]: V };

export interface PMap<V> {
  readonly size: number;
  readonly buckets: ReadonlyArray<Bucket<V> | undefined>;
}

const EMPTY_BUCKETS: ReadonlyArray<undefined> = Object.freeze(new Array<undefined>(BUCKETS).fill(undefined));
const EMPTY_MAP: PMap<never> = Object.freeze({ size: 0, buckets: EMPTY_BUCKETS });

export function pmap<V>(): PMap<V> {
  return EMPTY_MAP;
}

export function mapGet<V>(map: PMap<V>, key: string): V | undefined {
  const bucket = map.buckets[bucketOf(key)];
  return bucket !== undefined && Object.hasOwn(bucket, key) ? bucket[key] : undefined;
}

export function mapHas<V>(map: PMap<V>, key: string): boolean {
  const bucket = map.buckets[bucketOf(key)];
  return bucket !== undefined && Object.hasOwn(bucket, key);
}

export function mapSet<V>(map: PMap<V>, key: string, value: V): PMap<V> {
  const at = bucketOf(key);
  const bucket = map.buckets[at];
  const had = bucket !== undefined && Object.hasOwn(bucket, key);
  if (had && bucket[key] === value) return map;
  const buckets = map.buckets.slice();
  // A null prototype: a key such as "constructor" is a key like any other.
  const next: { [key: string]: V } = Object.assign(Object.create(null) as { [key: string]: V }, bucket);
  next[key] = value;
  buckets[at] = next;
  return { size: map.size + (had ? 0 : 1), buckets };
}

export function mapDelete<V>(map: PMap<V>, key: string): PMap<V> {
  const at = bucketOf(key);
  const bucket = map.buckets[at];
  if (bucket === undefined || !Object.hasOwn(bucket, key)) return map;
  const buckets = map.buckets.slice();
  const next: { [key: string]: V } = Object.assign(Object.create(null) as { [key: string]: V }, bucket);
  delete next[key];
  buckets[at] = Object.keys(next).length ? next : undefined;
  return { size: map.size - 1, buckets };
}

/** Every value, in no particular order. */
export function mapValues<V>(map: PMap<V>): V[] {
  const out: V[] = [];
  for (const bucket of map.buckets) if (bucket !== undefined) for (const key of Object.keys(bucket)) out.push(bucket[key] as V);
  return out;
}

export interface PVec<T> {
  readonly length: number;
  /** Every chunk holds exactly 64 items, except the last one. */
  readonly chunks: ReadonlyArray<ReadonlyArray<T>>;
}

const EMPTY_VEC: PVec<never> = Object.freeze({ length: 0, chunks: Object.freeze([]) });

export function pvec<T>(): PVec<T> {
  return EMPTY_VEC;
}

export function vecFrom<T>(items: ReadonlyArray<T>): PVec<T> {
  if (!items.length) return EMPTY_VEC;
  const chunks: T[][] = [];
  for (let i = 0; i < items.length; i += CHUNK) chunks.push(items.slice(i, i + CHUNK));
  return { length: items.length, chunks };
}

export function vecGet<T>(vec: PVec<T>, index: number): T | undefined {
  if (index < 0 || index >= vec.length) return undefined;
  return (vec.chunks[index >> 6] as ReadonlyArray<T>)[index & 63];
}

export function vecPush<T>(vec: PVec<T>, item: T): PVec<T> {
  const chunks = vec.chunks.slice();
  const last = chunks[chunks.length - 1];
  if (last !== undefined && last.length < CHUNK) chunks[chunks.length - 1] = [...last, item];
  else chunks.push([item]);
  return { length: vec.length + 1, chunks };
}

export function vecSet<T>(vec: PVec<T>, index: number, item: T): PVec<T> {
  if (index < 0 || index >= vec.length) throw new RangeError(`no item ${index} in a list of ${vec.length}`);
  const at = index >> 6;
  const chunk = vec.chunks[at] as ReadonlyArray<T>;
  if (chunk[index & 63] === item) return vec;
  const chunks = vec.chunks.slice();
  const next = chunk.slice();
  next[index & 63] = item;
  chunks[at] = next;
  return { length: vec.length, chunks };
}

/**
 * Removes `remove` items at `index` and puts `items` there. Chunks before the change are shared;
 * the ones after it are cut again, which costs the number of items after the change.
 */
export function vecSplice<T>(vec: PVec<T>, index: number, remove: number, items: ReadonlyArray<T>): PVec<T> {
  if (index < 0 || index > vec.length || remove < 0 || index + remove > vec.length) throw new RangeError(`cannot change ${remove} items at ${index} in a list of ${vec.length}`);
  if (!remove && !items.length) return vec;
  if (!remove && index === vec.length && items.length === 1) return vecPush(vec, items[0] as T);
  const first = index >> 6;
  const chunks = vec.chunks.slice(0, first);
  const tail: T[] = [];
  const head = vec.chunks[first];
  if (head !== undefined) for (let i = 0; i < (index & 63); i++) tail.push(head[i] as T);
  for (const item of items) tail.push(item);
  let skip = remove;
  for (let c = first; c < vec.chunks.length; c++) {
    const chunk = vec.chunks[c] as ReadonlyArray<T>;
    for (let i = c === first ? index & 63 : 0; i < chunk.length; i++) {
      if (skip > 0) skip--;
      else tail.push(chunk[i] as T);
    }
  }
  for (let i = 0; i < tail.length; i += CHUNK) chunks.push(tail.slice(i, i + CHUNK));
  return { length: vec.length - remove + items.length, chunks };
}

const arrays = new WeakMap<PVec<unknown>, ReadonlyArray<unknown>>();

/** The list as one array. Built once for each version of the list. */
export function vecArray<T>(vec: PVec<T>): ReadonlyArray<T> {
  const known = arrays.get(vec);
  if (known !== undefined) return known as ReadonlyArray<T>;
  const out: T[] = [];
  for (const chunk of vec.chunks) for (const item of chunk) out.push(item);
  arrays.set(vec, out);
  return out;
}
