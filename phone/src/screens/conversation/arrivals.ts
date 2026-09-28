/**
 * Which rows arrive and which were there: a row of the loaded history is drawn in place, a row
 * that came live afterwards arrives once (AC-137). A row asks when it is first drawn, so nothing
 * is looked through when an event comes in.
 */
export interface Arrivals {
  /**
   * True for a row that came after the history, the first time it is drawn and for as long
   * as its arrival lasts; false ever after, so coming back on screen does not arrive again.
   */
  arriving(key: string): boolean;
}

/**
 * @param there   The keys of the rows of the loaded history, or `null` while it is loading:
 *                nothing arrives until then.
 * @param lastsMs How long an arrival lasts (a token).
 */
export function createArrivals(there: ReadonlySet<string> | null, now: () => number, lastsMs: number): Arrivals {
  const since = new Map<string, number>();
  const arrived = new Set<string>();
  return {
    arriving(key) {
      if (there === null || there.has(key) || arrived.has(key)) return false;
      const first = since.get(key);
      if (first === undefined) {
        since.set(key, now());
        return true;
      }
      if (now() - first <= lastsMs) return true;
      since.delete(key);
      arrived.add(key);
      return false;
    },
  };
}
