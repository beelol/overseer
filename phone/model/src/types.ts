// The daemon's records, exactly as it sends them. They are not declared here: they come from
// phone/protocol/protocol.generated.ts, which is generated from protocol/protocol.json (the same
// description the gateway's method classes are built from), so the phone and the daemon cannot
// disagree about a field on one side only.
//
// What is declared here is what that description does not have yet and this package reads. Each
// is narrowed from `unknown` by a function below, never assumed.

import type { Attention, Event, Mark, Run, Turn } from '../../protocol/protocol.generated.ts';

export type {
  Attention, Change, Comparison, Event, EventPayloads, FileSide, GitStatus, Hunk, KnownEventKind, Mark, NotificationSettings,
  Profile, Run, RunStatus, State, Task, Turn, TypedEvent, Workspace,
} from '../../protocol/protocol.generated.ts';

/** An event of the daemon's stream: `seq` grows by one event at a time. */
export type DaemonEvent = Event;

/** The statuses in which a run is still going (the daemon's ACTIVE list). */
export const ACTIVE_STATUSES: ReadonlyArray<string> = ['queued', 'starting', 'running', 'waiting_for_user'];

export function isActive(status: string | null | undefined): boolean {
  return status !== null && status !== undefined && ACTIVE_STATUSES.includes(status);
}

/** A payload as a record, or an empty one when it is anything else. */
export function record(value: unknown): Readonly<Record<string, unknown>> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

export function text(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

export function number(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

export function isRun(value: unknown): value is Run {
  const r = record(value);
  return typeof r['id'] === 'string' && typeof r['task_id'] === 'string' && typeof r['status'] === 'string' && typeof r['created_ms'] === 'number';
}

export function isTurn(value: unknown): value is Turn {
  const t = record(value);
  return typeof t['id'] === 'string' && typeof t['run_id'] === 'string' && typeof t['n'] === 'number';
}

export function isAttention(value: unknown): value is Attention {
  const a = record(value);
  return typeof a['kind'] === 'string' && a['request_id'] !== undefined && a['request_id'] !== null;
}

export function isMark(value: unknown): value is Mark {
  const m = record(value);
  return typeof m['key'] === 'string' && typeof m['path'] === 'string';
}

// ---- Event kinds the daemon sends that protocol/protocol.json does not describe yet.

/** `workspace_removed`: a worktree was cleaned up. */
export interface WorkspaceRemovedPayload {
  workspace_id: string;
  path: string;
  branch_kept?: string | null;
  discarded_dirty: boolean;
}

/** `profile`: an account profile was added or removed, or a sign-in or sign-out ran. */
export interface ProfilePayload {
  profile?: unknown;
  profile_id?: string;
  action?: string;
  at?: number;
}

/** An entry of the connection library's outbox (phone/core), taken as given. */
export interface OutboxEntry {
  requestId: string;
  method: string;
  params: unknown;
  state: 'queued' | 'sending' | 'done' | 'failed';
  createdAt: number;
}
