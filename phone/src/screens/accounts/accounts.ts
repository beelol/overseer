/**
 * What the Mac says about accounts, read without assuming its shape, and what a row says.
 * Pure functions: the screen holds the answers, the daemon's state holds the accounts.
 */
import { agents, number, record, type Profile } from '@/model';

import { ACCOUNTS, PROVIDER_NAME } from './words';

const str = (value: unknown): string | null =>
  typeof value === 'string' && value.length > 0 ? value : null;

/** An account: a profile of the daemon's state, under its provider. */
export interface Account {
  readonly id: string;
  readonly name: string;
  readonly harness: string;
  readonly provider: string;
  /** True for the desktop app's own login, which Overseer never changes. */
  readonly followsApp: boolean;
}

export interface ProviderGroup {
  readonly id: string;
  readonly name: string;
  /** The logo's name for `Logo`, or the provider's id when it has none. */
  readonly logo: string;
  readonly accounts: readonly Account[];
}

/** `account.list`: which provider each account belongs to, and the providers in the Mac's order. */
export interface Listed {
  readonly providerOf: ReadonlyMap<string, string>;
  readonly providers: readonly { readonly id: string; readonly label: string }[];
}

/** `profile.status`: the sign-in state. Nothing of the identity is kept but the plan. */
export interface Status {
  readonly installed: boolean;
  readonly signedIn: boolean;
  readonly plan: string | null;
  /** The account is set up with an API key, which Overseer does not use. */
  readonly apiKey: boolean;
}

export interface UsageWindow {
  readonly label: string;
  /** From 0 to 1. */
  readonly used: number;
  readonly resetsAt: number | null;
}

/** `account.usage`: only what the provider reports. Nothing is estimated. */
export interface Usage {
  readonly reported: boolean;
  readonly plan: string | null;
  readonly limited: boolean;
  readonly windows: readonly UsageWindow[];
}

// The provider of a harness, until the Mac has answered `account.list` (daemon/src/accounts.rs).
const PROVIDER_OF: Readonly<Record<string, string>> = {
  codex: 'openai',
  'codex-app': 'openai',
  claude: 'anthropic',
  opencode: 'local',
};
const ORDER: readonly string[] = ['openai', 'anthropic', 'local'];

// The harnesses whose provider gives a code to sign in with (daemon/src/device_login.rs).
const WITH_CODE: ReadonlySet<string> = new Set(['codex']);

/** True when the account is signed in with a code from the phone. Any other is signed in on the Mac. */
export function hasCode(account: Account): boolean {
  return !account.followsApp && WITH_CODE.has(account.harness);
}

export function readList(answer: unknown): Listed {
  const given = record(answer);
  const providerOf = new Map<string, string>();
  for (const item of Array.isArray(given['accounts']) ? given['accounts'] : []) {
    const account = record(item);
    const id = str(account['id']);
    const provider = str(account['provider']);
    if (id && provider) providerOf.set(id, provider);
  }
  const providers: { id: string; label: string }[] = [];
  for (const item of Array.isArray(given['providers']) ? given['providers'] : []) {
    const provider = record(item);
    const id = str(provider['id']);
    if (id) providers.push({ id, label: str(provider['label']) ?? id });
  }
  return { providerOf, providers };
}

export function readStatus(answer: unknown): Status {
  const given = record(answer);
  const method = str(given['method'])?.toLowerCase() ?? '';
  const apiKey = method.includes('api');
  return {
    installed: given['installed'] !== false,
    signedIn: given['logged_in'] === true && !apiKey,
    plan: str(record(given['identity'])['plan']),
    apiKey,
  };
}

export function readUsage(answer: unknown): Usage {
  const given = record(answer);
  const windows: UsageWindow[] = [];
  for (const item of Array.isArray(given['windows']) ? given['windows'] : []) {
    const window = record(item);
    const label = str(window['label']);
    const used = number(window['used']);
    if (label && used !== undefined)
      windows.push({
        label,
        used: Math.min(1, Math.max(0, used)),
        resetsAt: number(window['resets_at_ms']) ?? null,
      });
  }
  return {
    reported: given['reported'] === true,
    plan: str(given['plan']),
    limited: given['limited'] === true,
    windows,
  };
}

/** The accounts by provider: the Mac's order of providers, and only providers that have an account. */
export function byProvider(
  profiles: readonly Profile[],
  listed: Listed | null,
): readonly ProviderGroup[] {
  const groups = new Map<string, Account[]>();
  for (const profile of profiles) {
    const provider =
      listed?.providerOf.get(profile.id) ?? PROVIDER_OF[profile.harness] ?? profile.harness;
    const held = groups.get(provider) ?? [];
    held.push({
      id: profile.id,
      name: profile.name,
      harness: profile.harness,
      provider,
      followsApp: profile.is_system,
    });
    groups.set(provider, held);
  }
  const known = listed?.providers.map((provider) => provider.id) ?? ORDER;
  const order = [
    ...known.filter((id) => groups.has(id)),
    ...[...groups.keys()].filter((id) => !known.includes(id)),
  ];
  return order.map((id) => ({
    id,
    name:
      PROVIDER_NAME[id] ?? listed?.providers.find((provider) => provider.id === id)?.label ?? id,
    logo: agents.logoForProvider(id) ?? id,
    accounts: groups.get(id) ?? [],
  }));
}

/** "Pro" for the provider's "pro". */
export function planName(status: Status | undefined, usage: Usage | undefined): string | null {
  const plan = status?.plan ?? usage?.plan ?? null;
  return plan ? plan.charAt(0).toUpperCase() + plan.slice(1) : null;
}

/** "Signed in · Pro", "Signed out", or that it is not known yet. */
export function stateText(
  status: Status | undefined,
  usage: Usage | undefined,
  checking: boolean,
): string {
  if (!status) return checking ? ACCOUNTS.checking : ACCOUNTS.notChecked;
  if (!status.installed) return ACCOUNTS.notInstalled;
  if (!status.signedIn) return ACCOUNTS.signedOut;
  return [ACCOUNTS.signedIn, planName(status, usage)].filter(Boolean).join(' · ');
}

const sameDay = (a: Date, b: Date): boolean =>
  a.getFullYear() === b.getFullYear() &&
  a.getMonth() === b.getMonth() &&
  a.getDate() === b.getDate();
const WEEK = 7 * 24 * 60 * 60 * 1000;

/** The time a limit resets, in the phone's own way of writing times: today's as a time, later ones with their day. */
export function resetTime(resetsAt: number, now: number): string {
  const at = new Date(resetsAt);
  const time = at.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  if (sameDay(at, new Date(now))) return time;
  if (resetsAt - now < WEEK && resetsAt > now)
    return `${at.toLocaleDateString(undefined, { weekday: 'short' })} ${time}`;
  return `${at.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })} ${time}`;
}

/** "5 hours · 12% used · resets 3:40 PM". A reset that has passed is not said. */
export function usageText(window: UsageWindow, now: number): string {
  const parts = [window.label, ACCOUNTS.used(Math.round(window.used * 100))];
  if (window.resetsAt !== null && window.resetsAt > now)
    parts.push(ACCOUNTS.resets(resetTime(window.resetsAt, now)));
  return parts.join(' · ');
}

/** How a window reads at a glance: fine, close to its limit, or at it. */
export function usageTone(window: UsageWindow, limited: boolean): 'accent' | 'amber' | 'red' {
  if (window.used >= 1 || (limited && window.used >= NEAR)) return 'red';
  return window.used >= NEAR ? 'amber' : 'accent';
}
const NEAR = 0.8;

/** What `profile.device_login` answered. The code is shown and never kept. */
export type SignInAnswer =
  | {
      readonly kind: 'code';
      readonly url: string;
      readonly code: string;
      readonly validMs: number | null;
    }
  | { readonly kind: 'signedIn' }
  | { readonly kind: 'failed' };

export function readSignIn(answer: unknown): SignInAnswer {
  const given = record(answer);
  if (given['finished'] === true)
    return given['logged_in'] === true ? { kind: 'signedIn' } : { kind: 'failed' };
  const url = str(given['url']);
  const code = str(given['code']);
  // Only a page of the web, over an encrypted connection, is ever opened.
  if (!url || !code || !/^https:\/\/[^\s]+$/i.test(url)) return { kind: 'failed' };
  return { kind: 'code', url, code, validMs: number(given['valid_ms']) ?? null };
}
