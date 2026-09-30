import { useEffect, useMemo, useState } from 'react';

import { agents, record, store, text } from '@/model';
import type { Harness, KnownRepo } from '@/protocol';
import { useSession, useSessionValue } from '@/session';

import { useStore, writeStored } from '../agents/stored';
import { harnessLabel, type AccountChoice, type Choices, type Form, type HarnessChoice, type RepoChoice } from './form';
import { optionsOf } from './options';

type PhoneState = store.PhoneState;

/** What the Mac last said it offers, as the phone keeps it: the form opens on it at once. */
type Offered = {
  repos: { root: string; name: string; branch: string | null }[];
  harnesses: { harness: string; version: string | null; capabilities: Record<string, string> }[];
  accounts: { id: string; name: string; harnesses: string[]; signedIn: boolean | null; plan: string | null; email?: string | null }[];
};

/** What the New agent form keeps on the phone between launches (`keyValue.scope('new')`). */
export type NewStore = {
  /** What was chosen the last time an agent was started here. */
  last: Form;
  /** The task as far as it was typed. */
  draft: string;
  offered: Offered;
};

export function useNewStore() {
  return useStore<NewStore>('new');
}

/** Harnesses a phone cannot start: a program is the Mac's to start, and so is the app-server transport. */
const MAC_ONLY = new Set(['generic', 'codex-app']);

const words = (value: unknown): string | null => (typeof value === 'string' && value ? value : null);

function capabilitiesOf(value: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, said] of Object.entries(record(value))) if (typeof said === 'string') out[key] = said;
  return out;
}

function reposOf(known: readonly KnownRepo[]): Offered['repos'] {
  return known.filter((repo) => repo.exists).map((repo) => ({ root: repo.root, name: repo.name || text.basename(repo.root), branch: repo.branch ?? null }));
}

function harnessesOf(list: readonly Harness[]): Offered['harnesses'] {
  return list.filter((h) => h.installed && !MAC_ONLY.has(h.harness)).map((h) => ({ harness: h.harness, version: words(h.version), capabilities: capabilitiesOf(h.capabilities) }));
}

function accountsOf(list: readonly unknown[]): Offered['accounts'] {
  const out: Offered['accounts'] = [];
  for (const entry of list) {
    const account = record(entry);
    const id = words(account['id']);
    if (id === null) continue;
    const harnesses = Array.isArray(account['harnesses']) ? account['harnesses'].filter((h): h is string => typeof h === 'string') : [];
    // The Mac's own login is "Mac's default login"; the plan and the shortened email say which account it is (AC-235).
    const shown = record(account['account']);
    const own = words(account['name']) ?? id;
    const name = account['kind'] === 'follows-app' || / \(existing login\)$/.test(own) ? agents.DEFAULT_LOGIN : own;
    out.push({ id, name, harnesses, signedIn: null, plan: words(shown['plan']), email: words(shown['email']) });
  }
  return out;
}

/** What the state the phone holds shows of the Mac, for a form opened before the Mac answered. */
function fromState(state: PhoneState): Offered {
  const repos: Offered['repos'] = [];
  const tasks = store.rows(state.tasks);
  for (let at = tasks.length - 1; at >= 0; at--) {
    const root = tasks[at]?.repo_root;
    if (root && !repos.some((repo) => repo.root === root)) repos.push({ root, name: text.basename(root), branch: null });
  }
  const harnesses: Offered['harnesses'] = [];
  const runs = store.rows(state.runs);
  for (let at = runs.length - 1; at >= 0; at--) {
    const run = runs[at];
    if (!run || run.parent_run_id || MAC_ONLY.has(run.harness) || harnesses.some((h) => h.harness === run.harness)) continue;
    harnesses.push({ harness: run.harness, version: words(run.harness_version), capabilities: capabilitiesOf(run.capabilities) });
  }
  const accounts = store.rows(state.profiles).map((profile) => ({ id: profile.id, name: agents.accountName(profile), harnesses: [profile.harness], signedIn: null, plan: profile.account?.plan ?? null, email: profile.account?.email ?? null }));
  return { repos, harnesses, accounts };
}

/** The repositories the Mac named, then those of the agents the phone knows. */
function withRepos(offered: readonly RepoChoice[], known: readonly RepoChoice[]): readonly RepoChoice[] {
  const more = known.filter((repo) => !offered.some((o) => o.root === repo.root));
  return more.length > 0 ? [...offered, ...more] : offered;
}

export interface Offer extends Choices {
  /** True until the Mac has said what it offers, or the phone has it from the last time. */
  readonly loading: boolean;
}

/**
 * What can be chosen: the repositories Overseer knows, the installed agents with what each can
 * be told, the accounts with their sign-in state. Asked of the Mac when the form opens and
 * whenever the connection returns; until it answers, the form shows what it said the last time,
 * or what the agents the phone knows show.
 */
export function useChoices(enabled: boolean): Offer {
  const session = useSession();
  const kept = useNewStore();
  const online = useSessionValue((s) => s.connection === 'online');
  const tasks = useSessionValue((s) => s.state.tasks);
  const runs = useSessionValue((s) => s.state.runs);
  const profiles = useSessionValue((s) => s.state.profiles);
  const [offered, setOffered] = useState<Offered | null>(() => {
    try {
      return kept.get('offered');
    } catch {
      return null;
    }
  });

  useEffect(() => {
    if (!online || !enabled) return undefined;
    let current = true;
    void (async () => {
      const [known, harnesses, accounts] = await Promise.all([
        session.request('repo.known', {}).then((answer) => reposOf(answer.repos), () => null),
        session.request('harness.list', {}).then(harnessesOf, () => null),
        session.request('account.list', {}).then((answer) => accountsOf(answer.accounts), () => null),
      ]);
      if (!current || (known === null && harnesses === null && accounts === null)) return;
      const before = session.getSnapshot().state;
      const fallback = fromState(before);
      let next: Offered = { repos: known ?? fallback.repos, harnesses: harnesses ?? fallback.harnesses, accounts: accounts ?? fallback.accounts };
      setOffered(next);
      const states = await Promise.all(next.accounts.map((account) => session.request('profile.status', { id: account.id }).then((status) => ({ signedIn: status.logged_in, plan: words(record(status.identity)['plan']) }), () => null)));
      if (!current) return;
      next = { ...next, accounts: next.accounts.map((account, at) => { const read = states[at]; return read ? { ...account, signedIn: read.signedIn, plan: account.plan ?? read.plan } : account; }) };
      setOffered(next);
      writeStored<NewStore, 'offered'>(kept, 'offered', next);
    })();
    return () => {
      current = false;
    };
  }, [session, kept, online, enabled]);

  return useMemo(() => {
    const state = { ...store.EMPTY, tasks, runs, profiles };
    const known = fromState(state);
    const from = offered ?? known;
    const harnesses: HarnessChoice[] = from.harnesses.map((h) => ({ harness: h.harness, label: harnessLabel(h.harness), version: h.version, options: optionsOf(h.harness, h.capabilities) }));
    const accounts: AccountChoice[] = from.accounts;
    return { repos: withRepos(from.repos, known.repos), harnesses, accounts, loading: offered === null && enabled };
  }, [offered, tasks, runs, profiles, enabled]);
}
