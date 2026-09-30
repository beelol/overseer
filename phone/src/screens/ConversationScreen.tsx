import { useLocalSearchParams, useRouter } from 'expo-router';
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { View } from 'react-native';

import { agents, conversation, pending, store, text, type OutboxEntry } from '@/model';
import { Pulse } from '@/motion';
import { useCapabilities } from '@/platform';
import { routes } from '@/routes';
import { useConversation, useSession, useSessionValue } from '@/session';
import { useTheme } from '@/theme';
import { Button, Empty, makeStyles, Screen, Txt, WatchOnlyLine } from '@/ui';

import type { RowActions } from './conversation/actions';
import { ActionsProvider } from './conversation/actions';
import { useAnswers } from './conversation/answers';
import { createArrivals } from './conversation/arrivals';
import { Composer } from './conversation/Composer';
import { HeaderActions } from './conversation/HeaderActions';
import { EDITED_HUNK } from './conversation/ids';
import { heldFor, working, type FollowUp } from './conversation/held';
import { ConversationList, type ConversationListRef, type ListExtra } from './conversation/List';
import { createOpenStore, OpenProvider } from './conversation/open';
import { choicesFor } from './conversation/options';
import { ToolSheet } from './conversation/ToolSheet';
import { changesSignal, useChanges } from './conversation/useChanges';
import { WORDS } from './conversation/words';

const NOTHING: ReadonlySet<string> = new Set();

const useStyles = makeStyles((theme) => ({
  fill: { flex: 1 },
  notice: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.chat.gutterNarrow, paddingVertical: theme.space[2] },
  words: { flexShrink: 1 },
  loading: { flex: 1, justifyContent: 'flex-end', paddingHorizontal: theme.chat.gutterNarrow, paddingBottom: theme.space[6], gap: theme.space[3] },
  bar: { height: theme.space[4], borderRadius: theme.radius.control, backgroundColor: theme.colors.raised },
  short: { width: '40%', alignSelf: 'flex-end' },
  long: { width: '90%' },
  middle: { width: '65%' },
}));

/** The conversation of one agent, live, with full control (phone/docs/app-spec.md, Conversation). */
export function ConversationScreen() {
  const params = useLocalSearchParams<{ run?: string }>();
  const runId = typeof params.run === 'string' ? params.run : '';
  if (!runId) {
    return (
      <Screen id="agent" title={text.TEXT.chat.agent}>
        <Empty testID="agent.empty" text={WORDS.noAgent} />
      </Screen>
    );
  }
  return <Conversation key={runId} runId={runId} />;
}

/** The same value for as long as its content is the same: what is built from it is not built again. */
function useSame<T>(value: T): T {
  const content = JSON.stringify(value);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => value, [content]);
}

/**
 * For each child on screen, the depths of the children that hold its rows, itself first. The
 * same map until a child appears, moves or goes.
 */
function useLines(rows: readonly conversation.Row[], state: store.PhoneState): ReadonlyMap<string, readonly number[]> {
  const written = useMemo(() => {
    const depths = new Map<string, number>();
    for (const row of rows) if (row.kind === 'child') depths.set(row.childRun, row.depth);
    if (depths.size === 0) return '';
    const out: string[] = [];
    for (const [run, depth] of depths) {
      const lines = [depth];
      let parent = store.run(state, run)?.parent_run_id;
      for (let i = 0; parent && i < 16; i++) {
        const at = depths.get(parent);
        if (at !== undefined) lines.push(at);
        parent = store.run(state, parent)?.parent_run_id;
      }
      out.push(`${run}=${lines.join(',')}`);
    }
    return out.join('\n');
  }, [rows, state]);
  return useMemo(() => {
    const lines = new Map<string, readonly number[]>();
    if (written === '') return lines;
    for (const line of written.split('\n')) {
      const at = line.lastIndexOf('=');
      lines.set(line.slice(0, at), line.slice(at + 1).split(',').map(Number));
    }
    return lines;
  }, [written]);
}

function Loading() {
  const styles = useStyles();
  return (
    <View testID="agent.loading" accessible accessibilityLabel={WORDS.loading} style={styles.loading}>
      <Pulse>
        <View style={[styles.bar, styles.short]} />
      </Pulse>
      <Pulse>
        <View style={[styles.bar, styles.long]} />
      </Pulse>
      <Pulse>
        <View style={[styles.bar, styles.middle]} />
      </Pulse>
    </View>
  );
}

function Conversation({ runId }: { readonly runId: string }) {
  const styles = useStyles();
  const theme = useTheme();
  const router = useRouter();
  const session = useSession();
  const capabilities = useCapabilities();
  const { random } = capabilities;

  const state = useSessionValue((s) => s.state);
  const outbox = useSessionValue((s) => s.outbox);
  const watch = useSessionValue((s) => s.scope === 'watch');
  const online = useSessionValue((s) => s.connection === 'online');
  const known = useSessionValue((s) => s.stateAt !== null);
  const snapshot = useConversation(runId);
  const talk = snapshot.conversation;

  const run = store.run(state, runId);
  const header = useSame(agents.runHeader(state, runId));
  const workspace = useSame(pick(store.workspace(state, run?.workspace_id)));
  const choices = useSame(choicesFor(run));
  const busy = working(run);

  // Messages written while the agent works wait on the phone; they show as the outbox's do.
  const held = useMemo(() => heldFor(session, capabilities), [session, capabilities]);
  const waiting = useSyncExternalStore(held.subscribe, held.getSnapshot, held.getSnapshot);
  const entries = useMemo<readonly OutboxEntry[]>(() => (waiting.length === 0 ? outbox : [...outbox, ...waiting.filter((w) => !outbox.some((e) => e.requestId === w.requestId))]), [outbox, waiting]);
  const heldIds = useMemo(() => new Set(waiting.map((w) => w.requestId)), [waiting]);

  const [toggled, setToggled] = useState(NOTHING);
  const rows = pending.withPending(talk, entries, toggled);
  const lines = useLines(rows, state);
  const { answers, answer } = useAnswers(runId, outbox);
  const extra = useMemo<ListExtra>(() => ({ toggled, answers, held: heldIds, lines, watch }), [toggled, answers, heldIds, lines, watch]);

  // What was there when the history arrived is drawn in place; what comes after it arrives.
  const loaded = !snapshot.loading;
  const there = useMemo(
    () => (loaded ? new Set(conversation.rowsOf(talk).map((row) => row.key)) : null),
    // Taken once, the moment the history is there.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [loaded],
  );
  const arrivals = useMemo(() => createArrivals(there, Date.now, theme.phone.motion.message.arrive), [there, theme]);
  // Replies are read with the home folder of the conversation they belong to.
  const home = talk.home;
  const anchor = useMemo(() => conversation.create({ rootId: runId, home }), [runId, home]);

  const latest = useRef({ rows, entries });
  useEffect(() => {
    latest.current = { rows, entries };
  }, [rows, entries]);

  const list = useRef<ConversationListRef>(null);
  const openStore = useMemo(() => createOpenStore(), []);
  const [tool, setTool] = useState<string | null>(null);
  const closeTool = useCallback(() => setTool(null), []);
  const toolRow = useMemo(() => {
    if (tool === null) return null;
    const row = rows.find((r) => r.key === tool);
    return row?.kind === 'tool' ? row : null;
  }, [tool, rows]);

  const send = useCallback(
    (message: Omit<FollowUp, 'run_id'>) => {
      held.send(random.uuid(), { run_id: runId, ...message });
      list.current?.toEnd();
    },
    [held, random, runId],
  );
  const stop = useCallback(() => {
    session.request('run.interrupt', { run_id: runId }).catch(() => undefined);
  }, [session, runId]);

  const actions = useMemo<RowActions>(
    () => ({
      rootId: runId,
      toggle: (key) =>
        setToggled((before) => {
          const next = new Set(before);
          if (!next.delete(key)) next.add(key);
          return next;
        }),
      openTool: setTool,
      openFile: (of, path) => router.push(routes.file(of, path, { hunk: EDITED_HUNK })),
      answer,
      retry: (requestId) => {
        const entry = latest.current.entries.find((e) => e.requestId === requestId);
        const params = entry?.params as FollowUp | undefined;
        if (entry?.method !== 'run.follow_up' || !params) return;
        session.dismiss(requestId);
        held.send(random.uuid(), params);
      },
      remove: (requestId) => session.dismiss(requestId),
      cancel: (requestId) => held.cancel(requestId),
      signIn: () => router.push(routes.accounts),
      markdownOf: (row) => conversation.markdownOf(row, anchor),
      arriving: (key) => arrivals.arriving(key),
    }),
    [runId, router, session, held, random, answer, anchor, arrivals],
  );

  const signal = useMemo(() => changesSignal(talk), [talk]);
  const changes = useChanges(workspace?.id, signal, online);

  const retryLoad = useCallback(() => {
    // The session loads a history when a conversation opens and offers no way to load it again
    // (see the screen's report). Until it does, what a screen can ask for is the state.
    const again = (session as unknown as { reloadConversation?: (runId: string) => Promise<void> }).reloadConversation;
    if (typeof again === 'function') again.call(session, runId).catch(() => undefined);
    else session.reload().catch(() => undefined);
  }, [session, runId]);

  const notices = useMemo(() => {
    const gone = snapshot.truncated || talk.banner !== null;
    if (!gone && snapshot.error === null) return null;
    return (
      <View>
        {gone ? (
          <View style={styles.notice}>
            <Txt testID="agent.truncated" kind="small" tone="muted" style={styles.words}>
              {WORDS.olderGone}
            </Txt>
          </View>
        ) : null}
        {snapshot.error !== null ? (
          <View testID="agent.error" style={styles.notice}>
            <Txt kind="small" tone="muted" style={styles.words}>
              {WORDS.notLoaded}
            </Txt>
            <Button testID="agent.error.retry" label={WORDS.tryAgain} kind="quiet" haptic="selection" onPress={retryLoad} />
          </View>
        ) : null}
      </View>
    );
  }, [snapshot.truncated, snapshot.error, talk.banner, styles, retryLoad]);

  const empty = rows.length === 0;
  const body =
    empty && snapshot.loading ? (
      <Loading />
    ) : empty && !header && known && snapshot.error === null ? (
      <Empty testID="agent.missing" text={WORDS.notOnTheMac} />
    ) : empty ? (
      <View style={styles.fill}>{notices}</View>
    ) : (
      <ConversationList ref={list} rows={rows} extra={extra} working={talk.working} header={notices} />
    );

  return (
    <Screen
      id="agent"
      title={header?.title ?? text.TEXT.chat.agent}
      {...(header ? { subtitle: [header.statusText, header.accountShort].filter(Boolean).join(' · ') } : {})}
      actions={<HeaderActions runId={runId} header={header} taskId={run?.task_id} workspace={workspace} changes={header?.child ? null : changes} watch={watch} onStop={stop} />}
      footer={watch ? <WatchOnlyLine /> : header ? <Composer runId={runId} header={header} model={header.model} choices={choices} busy={busy} onSend={send} onStop={stop} /> : undefined}
    >
      <OpenProvider value={openStore}>
        <ActionsProvider value={actions}>
          {body}
          <ToolSheet row={toolRow} onClose={closeTool} />
        </ActionsProvider>
      </OpenProvider>
    </Screen>
  );
}

/** What the header's controls need of a workspace. */
function pick(workspace: ReturnType<typeof store.workspace>): { id: string; kind: 'worktree' | 'current'; branch?: string | null; removed_ms?: number | null } | undefined {
  return workspace ? { id: workspace.id, kind: workspace.kind, branch: workspace.branch ?? null, removed_ms: workspace.removed_ms ?? null } : undefined;
}
