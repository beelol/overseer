import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { review, text } from '@/model';
import { useSession, useSessionValue, type Session } from '@/session';

import { askComparison, chosenFor, fromKey, resolve, useReviewStore, type Resolved } from './comparison';
import { hunksKey, memoryOf, type Chosen, type HunksResult } from './data';
import { notConnected, sentence } from './errors';
import { languageOf, type Language } from './syntax';

export interface FileChanges {
  readonly loading: boolean;
  /** What the Mac refused, in one line. */
  readonly error: string | null;
  /** The file's hunks as the Mac sent them; `null` until they are known. */
  readonly result: HunksResult | null;
  readonly workspaceId: string | null;
  /** The commit the file is compared with. */
  readonly base: string | null;
  /** The comparison's name: "Latest run". */
  readonly comparison: string | null;
  /** Asks the Mac again. `comparison`: the comparison may have moved (a turn started), ask for it too. */
  reload(options?: { comparison?: boolean }): Promise<void>;
}

interface Place {
  readonly workspaceId: string;
  readonly base: string;
  readonly label: string;
}

const placeOf = (resolved: Resolved): Place | null => (resolved.current?.base ? { workspaceId: resolved.options.workspace.id, base: resolved.current.base, label: resolved.current.label } : null);

async function comparisonFor(session: Session, runId: string, chosen: Chosen | undefined, ask: boolean): Promise<Resolved | null> {
  return ask ? askComparison(session, runId, chosen) : null;
}

/**
 * One file's changes against the comparison the route names, or the one kept for the run.
 * `onHunks` is told the hunks the moment they arrive, before they are drawn.
 */
export function useFileChanges(runId: string, path: string, comparisonKey: string | undefined, onHunks: (hunks: HunksResult['hunks']) => void): FileChanges {
  const session = useSession();
  const kept = useReviewStore();
  const memory = memoryOf(session);
  const online = useSessionValue((s) => s.connection === 'online');
  const chosen = useMemo<Chosen | undefined>(() => fromKey(comparisonKey) ?? chosenFor(kept, runId), [comparisonKey, kept, runId]);

  // What the changed-files list learned a moment ago saves asking for the comparison again.
  const remembered = useMemo<Place | null>(() => {
    const stored = memory.changes.get(runId);
    if (!stored) return null;
    const resolved = resolve(stored.options, chosen);
    // The one asked for, not the one that stands in for it when it is missing.
    const same = resolved.current !== null && (chosen === undefined || (resolved.current.mode === chosen.mode && (!chosen.branch || resolved.current.branch === chosen.branch)));
    return same ? placeOf(resolved) : null;
  }, [memory, runId, chosen]);

  const [place, setPlace] = useState<Place | null>(remembered);
  const [result, setResult] = useState<HunksResult | null>(() => (remembered ? (memory.hunks.get(hunksKey(remembered.workspaceId, remembered.base, path)) ?? null) : null));
  const [error, setError] = useState<string | null>(null);
  const [settled, setSettled] = useState(false);
  const loading = online && !settled && Boolean(runId && path);
  const known = useRef<Place | null>(remembered);
  const generation = useRef(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const reload = useCallback(
    async (options?: { comparison?: boolean }): Promise<void> => {
      const mine = ++generation.current;
      const current = (): boolean => alive.current && mine === generation.current;
      try {
        // The comparison is asked for when it is not known yet, or may have moved.
        const resolved = await comparisonFor(session, runId, chosen, known.current === null || options?.comparison === true);
        if (!current()) return;
        const at = resolved ? placeOf(resolved) : known.current;
        if (at === null) {
          setError(text.TEXT.review.comparisonUnavailable(resolved?.why || text.TEXT.review.unavailable));
          return;
        }
        if (resolved) {
          known.current = at;
          setPlace(at);
        }
        const answered = await session.request('workspace.hunks', { workspace_id: at.workspaceId, path, base: at.base, run_id: runId });
        if (!current()) return;
        memory.hunks.set(hunksKey(at.workspaceId, at.base, path), answered);
        onHunks(answered.hunks);
        setResult(answered);
        setError(null);
      } catch (failure) {
        if (!current()) return;
        setError(notConnected(failure) ? null : sentence(failure));
      } finally {
        if (current()) setSettled(true);
      }
    },
    [session, memory, runId, path, chosen, onHunks],
  );

  useEffect(() => {
    if (runId && path && online) void reload();
  }, [runId, path, online, reload]);

  return useMemo(() => ({ loading, error, result, workspaceId: place?.workspaceId ?? null, base: place?.base ?? null, comparison: place?.label ?? null, reload }), [loading, error, result, place, reload]);
}

/** A line longer than this shows its start and says how much more there is: never one giant text. */
export const LONGEST_LINE = 2000;

export type FileItem =
  | { readonly kind: 'hunk'; readonly key: string; readonly hunk: review.HunkView }
  | { readonly kind: 'line'; readonly key: string; readonly row: review.DiffRow; readonly text: string; readonly more: number };

export interface FileItems {
  readonly items: readonly FileItem[];
  /** Where each hunk starts in `items`, by key. */
  readonly starts: ReadonlyMap<string, number>;
  /** The places in `items` of the hunks' headings. */
  readonly headings: number[];
  /** The longest line, in characters. */
  readonly longest: number;
  /** How many digits the largest line number has. */
  readonly digits: number;
  readonly language: Language | null;
}

const shown = (line: string): string => line.replace(/\t/g, '  ');

/** A file's hunks as the rows of a list: a heading for each hunk, then its lines, one row each. */
export function fileItems(diff: review.FileDiff, hidden: ReadonlySet<string>): FileItems {
  const items: FileItem[] = [];
  const starts = new Map<string, number>();
  const headings: number[] = [];
  let longest = 0;
  let largest = 1;
  for (const hunk of diff.hunks) {
    if (hidden.has(hunk.key)) continue;
    starts.set(hunk.key, items.length);
    headings.push(items.length);
    items.push({ kind: 'hunk', key: `hunk:${hunk.key}`, hunk });
    for (const row of hunk.rows) {
      const whole = shown(row.text);
      const line = whole.length > LONGEST_LINE ? whole.slice(0, LONGEST_LINE) : whole;
      longest = Math.max(longest, line.length);
      largest = Math.max(largest, row.baseLine ?? 0, row.modifiedLine ?? 0);
      items.push({ kind: 'line', key: row.key, row, text: line, more: whole.length - line.length });
    }
  }
  return { items, starts, headings, longest, digits: String(largest).length, language: languageOf(diff.path) };
}

/**
 * What an edit of the conversation opens the file with: the hunk the agent edited, which is
 * the first one not reviewed yet (`review.editTarget`). The conversation's screen sends the
 * same word; it belongs beside the routes (reported).
 */
export const EDITED_HUNK = 'edited';

/**
 * The row a route's `hunk` names: a hunk's key, "edited", or a line of the working copy (the
 * hunk that holds it, else the first that follows it). Nothing named, or nothing found: the top.
 */
export function placeOfHunk(diff: review.FileDiff, items: FileItems, named: string | undefined): number {
  if (!named) return 0;
  const direct = items.starts.get(named === EDITED_HUNK ? (review.editTarget(diff)?.hunk ?? '') : named);
  if (direct !== undefined) return direct;
  if (!/^\d+$/.test(named)) return 0;
  const line = Number(named);
  const found = diff.hunks.find((hunk) => items.starts.has(hunk.key) && hunk.modifiedStart + Math.max(1, hunk.added) > line);
  return (found && items.starts.get(found.key)) ?? 0;
}
