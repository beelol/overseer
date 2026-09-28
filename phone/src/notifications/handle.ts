import type { Json, PushCategory, PushNotification, PushResponse } from '@/platform';

/** The categories the daemon names in a notification (daemon/src/gateway/push.rs). */
export const CATEGORY_PERMISSION = 'OVERSEER_PERMISSION';
export const CATEGORY_AGENT = 'OVERSEER_AGENT';
export const ACTION_ALLOW = 'allow';
export const ACTION_DENY = 'deny';

export type NotificationKind = 'permission' | 'question' | 'failure' | 'finished';
export const KINDS: readonly NotificationKind[] = ['permission', 'question', 'failure', 'finished'];

/** What a notification says for each kind, as the daemon writes it. */
export const SENTENCE: Readonly<Record<NotificationKind, string>> = {
  permission: 'Needs your permission',
  question: 'Has a question for you',
  failure: 'Stopped with an error',
  finished: 'Finished',
};

/**
 * The actions a notification offers. Allow and Deny are delivered only after the phone is
 * unlocked, and neither opens the app: the answer goes to the Mac from where the owner is.
 */
export const CATEGORIES: readonly PushCategory[] = [
  {
    id: CATEGORY_PERMISSION,
    actions: [
      { id: ACTION_ALLOW, title: 'Allow', requiresUnlock: true, destructive: false, opensApp: false },
      { id: ACTION_DENY, title: 'Deny', requiresUnlock: true, destructive: true, opensApp: false },
    ],
  },
  { id: CATEGORY_AGENT, actions: [] },
];

/** What the daemon puts in a notification besides its text. */
export interface OverseerPayload {
  readonly kind: NotificationKind;
  readonly runId: string;
  readonly taskId: string | null;
  readonly requestId: string | null;
}

const text = (value: Json | undefined): string | null => (typeof value === 'string' && value.length > 0 ? value : null);

/** The daemon's part of a notification, or null when the notification is not Overseer's. */
export function payloadOf(notification: PushNotification): OverseerPayload | null {
  const given = notification.data['overseer'];
  if (given === null || typeof given !== 'object' || Array.isArray(given)) return null;
  const overseer = given as { readonly [key: string]: Json };
  const kind = text(overseer['kind']);
  const runId = text(overseer['run_id']);
  if (!runId || !kind || !(KINDS as readonly string[]).includes(kind)) return null;
  return { kind: kind as NotificationKind, runId, taskId: text(overseer['task_id']), requestId: text(overseer['request_id']) };
}

export type NotificationAct =
  /** Open the agent the notification is about. */
  | { readonly do: 'open'; readonly runId: string }
  /** Answer the permission request, from the notification. */
  | { readonly do: 'answer'; readonly runId: string; readonly requestId: string; readonly allow: boolean };

/** What the owner's tap on a notification, or on one of its actions, asks for. */
export function actOf(response: PushResponse): NotificationAct | null {
  const payload = payloadOf(response.notification);
  if (!payload) return null;
  if ((response.actionId === ACTION_ALLOW || response.actionId === ACTION_DENY) && payload.kind === 'permission' && payload.requestId) {
    return { do: 'answer', runId: payload.runId, requestId: payload.requestId, allow: response.actionId === ACTION_ALLOW };
  }
  return { do: 'open', runId: payload.runId };
}
