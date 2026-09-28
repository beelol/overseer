/**
 * What the app measures about itself (AC-135): moments, and the time between two of them.
 * Pure bookkeeping: it costs a clock reading and an array push, so it stays on in release builds,
 * where the budgets are checked.
 */

export interface Measure {
  readonly name: string;
  /** Milliseconds. */
  readonly ms: number;
  /** When it ended, in milliseconds since the app's process started. */
  readonly at: number;
}

export interface PerfReport {
  /** Moments, in milliseconds since the app's process started. The first of each name. */
  readonly marks: Readonly<Record<string, number>>;
  readonly measures: readonly Measure[];
  /** Per name: how many, the median, the 95th percentile and the longest, in milliseconds. */
  readonly summary: Readonly<Record<string, { readonly count: number; readonly p50: number; readonly p95: number; readonly max: number }>>;
}

interface StartupTiming {
  readonly startTime?: number | null;
  readonly executeJavaScriptBundleEntryPointStart?: number | null;
  readonly endTime?: number | null;
}

declare const performance: { now(): number; readonly rnStartupTiming?: StartupTiming };

const KEEP = 2_000;

export class Perf {
  private readonly marks = new Map<string, number>();
  private readonly open = new Map<string, number>();
  private measures: Measure[] = [];
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly now: () => number = () => performance.now(),
    /** When the process started, on the clock of `now`. */
    private readonly origin: () => number = () => performance.rnStartupTiming?.startTime ?? 0,
  ) {}

  /** A moment. Only the first of a name is kept: "the first screen" happens once. */
  mark(name: string): void {
    if (this.marks.has(name)) return;
    this.marks.set(name, this.now() - this.origin());
    this.changed();
  }

  /**
   * A moment that happened at `at`, on the clock of `now` (the UI thread's frame times share it),
   * rather than when the app's logic hears of it. Only the first of a name is kept.
   */
  markAt(name: string, at: number): void {
    if (this.marks.has(name)) return;
    this.marks.set(name, at - this.origin());
    this.changed();
  }

  /** The start of something timed. A second `begin` of the same name starts it again. */
  begin(name: string): void {
    this.open.set(name, this.now());
  }

  /** The end of what `begin` started. Without a `begin` it does nothing. */
  end(name: string): void {
    const started = this.open.get(name);
    if (started === undefined) return;
    this.open.delete(name);
    this.record(name, this.now() - started);
  }

  record(name: string, ms: number): void {
    this.measures.push({ name, ms, at: this.now() - this.origin() });
    if (this.measures.length > KEEP) this.measures = this.measures.slice(-KEEP);
    this.changed();
  }

  report(): PerfReport {
    const by = new Map<string, number[]>();
    for (const m of this.measures) by.set(m.name, [...(by.get(m.name) ?? []), m.ms]);
    const summary: Record<string, { count: number; p50: number; p95: number; max: number }> = {};
    for (const [name, values] of by) {
      const sorted = [...values].sort((a, b) => a - b);
      summary[name] = { count: sorted.length, p50: percentile(sorted, 0.5), p95: percentile(sorted, 0.95), max: sorted[sorted.length - 1] ?? 0 };
    }
    return { marks: Object.fromEntries(this.marks), measures: this.measures, summary };
  }

  reset(): void {
    this.marks.clear();
    this.open.clear();
    this.measures = [];
    this.changed();
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  }

  private changed(): void {
    for (const listener of [...this.listeners]) listener();
  }
}

/** The value below which `fraction` of the sorted values lie (nearest rank). */
export function percentile(sorted: readonly number[], fraction: number): number {
  if (sorted.length === 0) return 0;
  const rank = Math.min(sorted.length, Math.max(1, Math.ceil(fraction * sorted.length)));
  return round(sorted[rank - 1] ?? 0);
}

const round = (ms: number): number => Math.round(ms * 10) / 10;

/** The app's one record of its own speed. */
export const perf = new Perf();

/** What React Native measured of its own start, in milliseconds since the process started. */
export function startup(): Readonly<Record<string, number>> {
  const t = typeof performance === 'undefined' ? undefined : performance.rnStartupTiming;
  const origin = t?.startTime ?? 0;
  const out: Record<string, number> = {};
  if (typeof t?.executeJavaScriptBundleEntryPointStart === 'number') out['javascript.start'] = round(t.executeJavaScriptBundleEntryPointStart - origin);
  if (typeof t?.endTime === 'number') out['javascript.loaded'] = round(t.endTime - origin);
  return out;
}
