import { FlashList, type ListRenderItemInfo } from '@shopify/flash-list';
import { useRouter } from 'expo-router';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { RefreshControl, ScrollView, View } from 'react-native';

import { text, type agents } from '@/model';
import { perf } from '@/perf';
import { routes } from '@/routes';
import { useSession, useSessionValue } from '@/session';
import { useTheme } from '@/theme';
import { Button, Confirm, Empty, IconButton, makeStyles, Menu, Screen, Txt, useMinute, WatchOnlyLine, type MenuItem } from '@/ui';

import { AgentRowView } from './agents/AgentRowView';
import { Filters } from './agents/Filters';
import { HeadingRow } from './agents/HeadingRow';
import { SearchField } from './agents/SearchField';
import { useAgentActions } from './agents/useActions';
import { useAgentsList } from './agents/useAgentsList';
import { WORDS } from './agents/words';

type Row = agents.AgentRow;

const useStyles = makeStyles((theme) => ({
  fill: { flex: 1 },
  grow: { flexGrow: 1 },
  content: { paddingBottom: theme.space[6] },
  quiet: { paddingHorizontal: theme.space[4], paddingBottom: theme.space[1] },
  footer: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[3] },
}));

const keyOf = (row: Row): string => row.id;
const typeOf = (row: Row): string => row.kind;
const isHeading = (row: Row): boolean => row.kind === 'rollup' || row.kind === 'section' || row.kind === 'repo' || row.kind === 'notice';
/** The list has drawn every row it shows: its first layout is done. */
const listLoaded = (): void => perf.mark('screen.agents.list');

/**
 * Agents, the app's first screen: every agent the Mac runs, those that need the owner first,
 * children under their parents. It opens on what the phone stored and says how old that is
 * until the Mac confirms it.
 */
export function AgentsScreen() {
  const styles = useStyles();
  const theme = useTheme();
  const router = useRouter();
  const session = useSession();
  const canChange = useSessionValue((s) => s.scope !== 'watch');
  const stale = useSessionValue((s) => s.stateAt !== null && (s.fromCache || s.connection !== 'online'));
  const stateAt = useSessionValue((s) => s.stateAt);
  const now = useMinute();

  const actions = useAgentActions();
  const list = useAgentsList(actions.archiving);
  const { opened, pin, fold, setQuery } = list;
  const { archive, stop, stopAll, activeAgents, stopping } = actions;

  const [searching, setSearching] = useState(false);
  const [menu, setMenu] = useState(false);
  const [more, setMore] = useState<{ readonly open: boolean; readonly row: Row | null }>({ open: false, row: null });
  const [stopCount, setStopCount] = useState<number | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const later = useRef<ReturnType<typeof setTimeout> | null>(null);
  const shown = useRef(true);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
      if (later.current !== null) clearTimeout(later.current);
    };
  }, []);

  const open = useCallback(
    (row: Row) => {
      if (row.runId === null) return;
      opened(row.runId);
      router.push(routes.agent(row.runId));
    },
    [opened, router],
  );
  const archiveRow = useCallback((row: Row) => (row.taskId === null ? undefined : archive(row.taskId)), [archive]);
  const foldRow = useCallback((row: Row) => fold(row.id, row.expanded), [fold]);
  const showMore = useCallback((row: Row) => setMore({ open: true, row }), []);
  const closeMore = useCallback(() => setMore((was) => ({ ...was, open: false })), []);
  const newAgent = useCallback(() => router.push(routes.newAgent), [router]);

  const closeSearch = useCallback(() => {
    setQuery('');
    setSearching(false);
  }, [setQuery]);

  const reload = useCallback(() => {
    setRefreshing(true);
    session.wake();
    session
      .reload()
      .catch(() => undefined)
      .finally(() => {
        if (shown.current) setRefreshing(false);
      });
  }, [session]);

  const askStopAll = useCallback(() => {
    const agentsGoing = activeAgents().length;
    // One sheet at a time: the question comes once the menu has gone.
    later.current = setTimeout(() => setStopCount(agentsGoing), theme.motion.duration.fast);
  }, [activeAgents, theme]);

  const menuItems = useMemo<readonly MenuItem[]>(
    () => [
      ...(canChange ? [{ id: 'new', label: WORDS.newAgent, icon: 'add', onPress: newAgent } as const] : []),
      { id: 'accounts', label: WORDS.accounts, icon: 'account', onPress: () => router.push(routes.accounts) },
      { id: 'settings', label: WORDS.settings, icon: 'settings-gear', onPress: () => router.push(routes.settings) },
      ...(canChange && list.counts.active > 0 ? [{ id: 'stop', label: WORDS.stopAll, icon: 'debug-stop', danger: true, onPress: askStopAll } as const] : []),
    ],
    [canChange, list.counts.active, newAgent, router, askStopAll],
  );

  const row = more.row;
  const moreItems = useMemo<readonly MenuItem[]>(() => {
    if (row === null || row.runId === null || row.kind === 'child') return [];
    const runId = row.runId;
    const taskId = row.taskId;
    return [
      { id: 'pin', label: row.pinned ? WORDS.unpin : WORDS.pin, icon: row.pinned ? 'pinned' : 'pin', onPress: () => pin(runId, !row.pinned) },
      ...(canChange && taskId !== null ? [{ id: 'archive', label: text.TEXT.agents.archive, icon: 'archive', onPress: () => archive(taskId) } as const] : []),
      ...(canChange && row.active && !stopping.has(runId) ? [{ id: 'stop', label: text.TEXT.agents.stop, icon: 'debug-stop', danger: true, onPress: () => stop(runId) } as const] : []),
    ];
  }, [row, canChange, stopping, pin, archive, stop]);

  const renderItem = useCallback(
    ({ item }: ListRenderItemInfo<Row>) =>
      isHeading(item) ? (
        <HeadingRow row={item} onFold={foldRow} />
      ) : (
        <AgentRowView row={item} stopping={item.runId !== null && stopping.has(item.runId)} canChange={canChange} onOpen={open} onMore={showMore} onArchive={archiveRow} />
      ),
    [foldRow, stopping, canChange, open, showMore, archiveRow],
  );
  const extra = useMemo(() => ({ stopping, canChange }), [stopping, canChange]);

  const refresh = <RefreshControl testID="agents.refresh" refreshing={refreshing} onRefresh={reload} tintColor={theme.colors.muted} colors={[theme.colors.accent]} progressBackgroundColor={theme.colors.raised} />;

  return (
    <Screen
      id="agents"
      title={text.TEXT.agents.title}
      back={false}
      actions={
        <>
          <IconButton testID="agents.search" accessibilityLabel={text.TEXT.agents.search} accessibilityState={{ expanded: searching }} icon="search" onPress={searching ? closeSearch : () => setSearching(true)} />
          <IconButton testID="agents.menu" accessibilityLabel={WORDS.menu} icon="ellipsis" onPress={() => setMenu(true)} />
        </>
      }
      footer={
        canChange ? (
          <View style={styles.footer}>
            <Button testID="agents.new" label={WORDS.newAgent} kind="primary" icon="add" wide haptic="selection" onPress={newAgent} />
          </View>
        ) : (
          <WatchOnlyLine />
        )
      }
    >
      {searching ? <SearchField query={list.query} onChange={setQuery} onClose={closeSearch} matches={list.matches} /> : null}
      <Filters filter={list.filter} onChange={list.setFilter} needs={list.counts.needs} />
      {stale ? (
        <Txt testID="agents.age" kind="small" tone="muted" style={styles.quiet}>
          {WORDS.asOf(text.agoInWords(stateAt, Math.max(now, stateAt ?? 0)))}
        </Txt>
      ) : null}
      {actions.error ? (
        <Txt testID="agents.error" kind="small" tone="red" accessibilityRole="alert" style={styles.quiet}>
          {actions.error}
        </Txt>
      ) : null}
      {list.rows.length === 0 ? (
        <ScrollView testID="agents.list" style={styles.fill} contentContainerStyle={styles.grow} refreshControl={refresh} keyboardShouldPersistTaps="handled">
          {list.empty !== null ? (
            <Empty testID="agents.empty" text={list.empty} {...(canChange && list.empty === text.TEXT.agents.empty ? { action: { testID: 'agents.empty.new', label: WORDS.newAgent, onPress: newAgent } } : {})} />
          ) : null}
        </ScrollView>
      ) : (
        <FlashList
          testID="agents.list"
          data={list.rows}
          renderItem={renderItem}
          keyExtractor={keyOf}
          getItemType={typeOf}
          onLoad={listLoaded}
          extraData={extra}
          refreshControl={refresh}
          contentContainerStyle={styles.content}
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="on-drag"
        />
      )}

      <Menu testID="agents.menu" open={menu} onClose={() => setMenu(false)} items={menuItems} />
      <Menu testID="agents.actions" open={more.open && moreItems.length > 0} onClose={closeMore} {...(row ? { title: row.label } : {})} items={moreItems} />
      <Confirm
        testID="agents.stop"
        open={stopCount !== null}
        onClose={() => setStopCount(null)}
        question={WORDS.stopQuestion(stopCount ?? 0)}
        detail={WORDS.stopDetail}
        confirm={WORDS.stopAll}
        onConfirm={stopAll}
      />
    </Screen>
  );
}
