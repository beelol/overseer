import { useRouter } from 'expo-router';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ScrollView, View } from 'react-native';

import { useCapabilities } from '@/platform';
import { routes } from '@/routes';
import { useSession, useSessionValue } from '@/session';
import { Button, makeStyles, Row, Screen, Section, Txt, WatchOnlyLine } from '@/ui';

import { readStored, writeStored } from './agents/stored';
import { listed } from './agents/useAgentsList';
import { Choices, type Choice } from './new/Choices';
import { choose, formOf, missing, modeLabel, NOTHING_CHOSEN, paramsOf, type AccountChoice, type Form } from './new/form';
import { TaskField } from './new/TaskField';
import { useChoices, useNewStore, type NewStore } from './new/useChoices';
import { WORDS } from './new/words';

type Part = 'repo' | 'agent' | 'account' | 'model' | 'effort' | 'mode' | 'where';

const useStyles = makeStyles((theme) => ({
  content: { paddingBottom: theme.space[8] },
  footer: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], gap: theme.space[2] },
}));

const why = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/** "signed in", "not signed in", and the plan where the Mac knows it. */
function accountDetail(account: AccountChoice): string {
  const state = account.signedIn === null ? '' : account.signedIn ? WORDS.signedIn : WORDS.notSignedIn;
  return [state, account.plan].filter(Boolean).join(' · ');
}

/** Test ids of choices by their name; a name that comes twice is numbered. */
function named(names: readonly string[]): readonly string[] {
  const seen = new Map<string, number>();
  return names.map((name) => {
    const times = (seen.get(name) ?? 0) + 1;
    seen.set(name, times);
    return times === 1 ? name : `${name}.${times}`;
  });
}

/**
 * New agent: the repository, the agent and its account, what the agent can be told, where it
 * works, and the task. What was chosen the last time is chosen again; Start sends one request
 * and opens the new agent's conversation.
 */
export function NewAgentScreen() {
  const styles = useStyles();
  const router = useRouter();
  const session = useSession();
  const { haptics } = useCapabilities();
  const kept = useNewStore();
  const state = useSessionValue(listed);
  const canChange = useSessionValue((s) => s.scope !== 'watch');
  const online = useSessionValue((s) => s.connection === 'online');
  const outbox = useSessionValue((s) => s.outbox);
  const offer = useChoices(canChange);

  const [form, setForm] = useState<Form>(() => ({ ...NOTHING_CHOSEN, ...readStored<NewStore, 'last'>(kept, 'last', NOTHING_CHOSEN) }));
  const [task, setTask] = useState(() => readStored<NewStore, 'draft'>(kept, 'draft', ''));
  const [open, setOpen] = useState<Part | null>(null);
  const [asked, setAsked] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shown = useRef(true);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);

  const chosen = useMemo(() => choose(form, offer, state), [form, offer, state]);
  const lacks = missing(chosen, task);
  const options = chosen.harness?.options ?? null;

  const change = useCallback((part: Partial<Form>) => setForm((was) => ({ ...was, ...part })), []);
  const close = useCallback(() => setOpen(null), []);
  const type = useCallback(
    (typed: string) => {
      setTask(typed);
      writeStored<NewStore, 'draft'>(kept, 'draft', typed);
    },
    [kept],
  );

  const start = useCallback(() => {
    const params = lacks === null ? paramsOf(chosen, task) : null;
    if (params === null) {
      setAsked(true);
      haptics.play('reject');
      return;
    }
    haptics.play('confirm');
    setError(null);
    setSending(true);
    writeStored<NewStore, 'last'>(kept, 'last', formOf(chosen));
    writeStored<NewStore, 'draft'>(kept, 'draft', '');
    // One request, sent once: while the Mac is away it waits, and is sent when the Mac is back.
    session.request('task.create', params).then(
      (made) => {
        if (shown.current) router.replace(routes.agent(made.run.id));
      },
      (refused: unknown) => {
        writeStored<NewStore, 'draft'>(kept, 'draft', params.prompt);
        if (!shown.current) return;
        setSending(false);
        setError(WORDS.notStarted(why(refused)));
      },
    );
  }, [lacks, chosen, task, haptics, kept, session, router]);

  const waiting = useMemo(() => outbox.filter((entry) => entry.method === 'task.create' && (entry.state === 'queued' || entry.state === 'sending')).length, [outbox]);
  const queued = online ? 0 : Math.max(waiting, sending ? 1 : 0);

  const repoIds = useMemo(() => named(offer.repos.map((repo) => repo.name)), [offer.repos]);
  const choices = useMemo((): readonly Choice[] => {
    switch (open) {
      case 'repo':
        return offer.repos.map((repo, at) => ({ id: repoIds[at] ?? repo.name, label: repo.name, ...(repo.branch ? { detail: repo.branch } : {}), icon: 'repo', selected: repo.root === chosen.repo?.root, onChoose: () => change({ repo: repo.root }) }));
      case 'agent':
        return offer.harnesses.map((h) => ({ id: h.harness, label: h.label, ...(h.version ? { detail: h.version } : {}), logo: h.harness, selected: h.harness === chosen.harness?.harness, onChoose: () => change({ harness: h.harness }) }));
      case 'account':
        return chosen.accounts.map((account) => ({ id: account.id, label: account.name, ...(accountDetail(account) ? { detail: accountDetail(account) } : {}), icon: 'account', selected: account.id === chosen.account?.id, onChoose: () => change({ account: account.id }) }));
      case 'model':
        return [{ id: 'default', label: WORDS.standard, selected: !chosen.model, onChoose: () => change({ model: '' }) }, ...(options?.models ?? []).map((model) => ({ id: model, label: model, selected: chosen.model === model, onChoose: () => change({ model }) }))];
      case 'effort':
        return [{ id: 'default', label: WORDS.standard, selected: !chosen.effort, onChoose: () => change({ effort: '' }) }, ...(options?.efforts ?? []).map((effort) => ({ id: effort, label: effort, selected: chosen.effort === effort, onChoose: () => change({ effort }) }))];
      case 'mode':
        return [{ id: 'default', label: WORDS.standard, selected: !chosen.mode, onChoose: () => change({ mode: '' }) }, ...(options?.modes ?? []).map((mode) => ({ id: mode, label: modeLabel(mode), selected: chosen.mode === mode, onChoose: () => change({ mode }) }))];
      case 'where':
        return [
          { id: 'worktree', label: WORDS.worktree, detail: WORDS.worktreeDetail, icon: 'git-branch', selected: chosen.where === 'worktree', onChoose: () => change({ where: 'worktree' }) },
          { id: 'current', label: WORDS.current, detail: WORDS.currentDetail, icon: 'repo', selected: chosen.where === 'current', onChoose: () => change({ where: 'current' }) },
        ];
      default:
        return [];
    }
  }, [open, offer, repoIds, chosen, options, change]);

  const sheets: Readonly<Record<Part, { readonly title: string; readonly empty?: string }>> = {
    repo: { title: WORDS.repository, empty: WORDS.noRepositories },
    agent: { title: WORDS.agent, empty: WORDS.noAgents },
    account: { title: WORDS.account, empty: WORDS.noAccounts },
    model: { title: WORDS.model },
    effort: { title: WORDS.effort },
    mode: { title: WORDS.permissions },
    where: { title: WORDS.workspace },
  };
  const sheet = open ? sheets[open] : null;

  if (!canChange) {
    return (
      <Screen id="new" title={WORDS.title} footer={<WatchOnlyLine />}>
        <View />
      </Screen>
    );
  }

  const account = chosen.account;
  return (
    <Screen
      id="new"
      title={WORDS.title}
      footer={
        <View style={styles.footer}>
          {error ? (
            <Txt testID="new.error" kind="label" tone="red" accessibilityRole="alert">
              {error}
            </Txt>
          ) : asked && lacks !== null ? (
            <Txt testID="new.missing" kind="label" tone="amber" accessibilityRole="alert" accessibilityLiveRegion="polite">
              {lacks}
            </Txt>
          ) : queued > 0 ? (
            <Txt testID="new.queued" kind="label" tone="muted" accessibilityLiveRegion="polite">
              {WORDS.queued(queued)}
            </Txt>
          ) : null}
          <Button testID="new.start" label={sending && online ? WORDS.starting : WORDS.start} kind="primary" wide disabled={sending} onPress={start} />
        </View>
      }
    >
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled" keyboardDismissMode="interactive">
        <Section title={WORDS.task}>
          <TaskField value={task} onChange={type} editable={!sending} />
        </Section>
        <Section title={WORDS.agent} {...(offer.repos.length === 0 && !offer.loading ? { note: WORDS.noRepositories } : {})}>
          <Row testID="new.repo" label={WORDS.repository} value={chosen.repo?.name ?? WORDS.none} {...(chosen.repo?.branch ? { detail: chosen.repo.branch } : {})} onPress={() => setOpen('repo')} />
          <Row testID="new.agent" label={WORDS.agent} value={chosen.harness?.label ?? WORDS.none} divided onPress={() => setOpen('agent')} />
          <Row testID="new.account" label={WORDS.account} value={account?.name ?? WORDS.none} {...(account && accountDetail(account) ? { detail: accountDetail(account) } : {})} divided onPress={() => setOpen('account')} />
          {account?.signedIn === false ? <Row testID="new.account.signin" label={WORDS.signIn} detail={WORDS.signInDetail(account.name)} tone="link" divided onPress={() => router.push(routes.accounts)} /> : null}
        </Section>
        <Section title={WORDS.options}>
          {options?.models ? <Row testID="new.model" label={WORDS.model} value={chosen.model || WORDS.standard} onPress={() => setOpen('model')} /> : null}
          {options?.efforts ? <Row testID="new.effort" label={WORDS.effort} value={chosen.effort || WORDS.standard} divided={Boolean(options.models)} onPress={() => setOpen('effort')} /> : null}
          {options?.modes ? <Row testID="new.mode" label={WORDS.permissions} value={chosen.mode ? modeLabel(chosen.mode) : WORDS.standard} divided={Boolean(options.models || options.efforts)} onPress={() => setOpen('mode')} /> : null}
          <Row testID="new.where" label={WORDS.workspace} value={chosen.where === 'current' ? WORDS.current : WORDS.worktree} divided={Boolean(options?.models || options?.efforts || options?.modes)} onPress={() => setOpen('where')} />
        </Section>
      </ScrollView>

      <Choices
        testID={open ? `new.${open}` : 'new.choices'}
        open={open !== null}
        onClose={close}
        title={sheet?.title ?? ''}
        {...(sheet?.empty ? { empty: sheet.empty } : {})}
        choices={choices}
        {...(open === 'model' ? { other: { label: WORDS.otherModel, action: WORDS.use, value: options?.models?.includes(chosen.model) ? '' : chosen.model, onChoose: (model: string) => change({ model }) } } : {})}
      />
    </Screen>
  );
}
