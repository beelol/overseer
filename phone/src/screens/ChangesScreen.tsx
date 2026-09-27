import { FlashList, type ListRenderItem } from '@shopify/flash-list';
import { useRouter } from 'expo-router';
import { useCallback, useMemo, useState } from 'react';
import { View } from 'react-native';

import { review, text, type Hunk } from '@/model';
import { Tap } from '@/motion';
import { useScrollFrames } from '@/perf';
import { routes } from '@/routes';
import { useSessionValue } from '@/session';
import { Empty, Icon, makeStyles, Screen, Txt, useMinute } from '@/ui';

import { useActivity, useRefreshOn } from './review/activity';
import { useChangedFiles, useRowHunks } from './review/changes';
import { keyOf } from './review/comparison';
import { ComparisonMenu } from './review/ComparisonMenu';
import { Field } from './review/Field';
import { FileRow, FolderRow } from './review/FileRows';
import { useMarks } from './review/marks';
import { Notice } from './review/Notice';
import { useReviewParams, useRunPlace } from './review/run';
import { WORDS } from './review/words';

const useStyles = makeStyles((theme) => ({
  summary: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.space[4], paddingVertical: theme.space[2], borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
  list: { flex: 1 },
  footer: { padding: theme.space[3], gap: theme.space[2] },
  comparison: { minHeight: theme.phone.size.touch, flexDirection: 'row', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.space[3], borderRadius: theme.radius.control, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong, backgroundColor: theme.colors.raised2 },
  comparisonText: { flex: 1 },
}));

const keyOfRow = (row: review.FileRow): string => `${row.kind}:${row.key}`;
const typeOfRow = (row: review.FileRow): string => row.kind;

/**
 * The files an agent changed, against a comparison (phone/docs/app-spec.md, Changes). The
 * list is the model's (`review.changedFiles`); it is asked again while the agent edits.
 */
export function ChangesScreen() {
  // Every scroll of the list is timed on the UI thread (AC-126: a large repository scrolls without dropped frames).
  const scrollFrames = useScrollFrames('changes');
  const styles = useStyles();
  const router = useRouter();
  const { run: runId } = useReviewParams();
  const { run, known } = useRunPlace(runId);
  const files = useChangedFiles(runId);
  const marks = useMarks(runId);
  const activity = useActivity(runId);
  const online = useSessionValue((s) => s.connection === 'online');
  const now = useMinute();
  const [query, setQuery] = useState('');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [choosing, setChoosing] = useState(false);

  // While the agent edits, when a turn starts or ends, when a hunk was put back or a merge moved on.
  useRefreshOn(`${activity.starts}:${activity.edits}:${activity.ends}:${activity.rejects}:${activity.merges}:${activity.status ?? ''}`, activity.ready, files.refresh);

  const changes = files.changes;
  const paths = useMemo(() => (changes ?? []).map((change) => change.path), [changes]);
  const rowHunks = useRowHunks(runId, files.workspaceId, files.current?.base ?? null, files.at, paths);

  // A hunk is reviewed when its key is marked now, whatever it was when its file was asked for.
  const hunks = useMemo(() => {
    if (!marks.loaded) return rowHunks.hunks;
    const out: Record<string, readonly Hunk[]> = {};
    for (const [path, list] of Object.entries(rowHunks.hunks)) out[path] = list.map((hunk) => (hunk.reviewed === marks.keys.has(hunk.key) ? hunk : { ...hunk, reviewed: marks.keys.has(hunk.key) }));
    return out;
  }, [rowHunks.hunks, marks.loaded, marks.keys]);

  const rows = useMemo(() => review.changedFiles(changes ?? [], { query: query.trim(), collapsed, hunks, conflicted: files.conflicted }), [changes, query, collapsed, hunks, files.conflicted]);

  const summary = useMemo(() => {
    if (!changes) return null;
    const every = changes.length > 0 && changes.every((change) => hunks[change.path] !== undefined || rowHunks.notShown[change.path] !== undefined);
    const lines = (side: 'modified_lines' | 'base_lines'): number => Object.values(hunks).reduce((n, list) => n + list.reduce((m, hunk) => m + hunk[side].length, 0), 0);
    return review.changesSummary({ files: changes.length, names: paths, ...(every ? { added: lines('modified_lines'), removed: lines('base_lines') } : {}) });
  }, [changes, paths, hunks, rowHunks.notShown]);

  const toggle = useCallback((key: string) => {
    setCollapsed((before) => {
      const next = new Set(before);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }, []);

  const comparison = files.current;
  const open = useCallback((path: string) => router.push(routes.file(runId, path, comparison ? { comparison: keyOf(comparison) } : {})), [router, runId, comparison]);

  const need = rowHunks.need;
  const notShown = rowHunks.notShown;
  const renderItem = useCallback<ListRenderItem<review.FileRow>>(
    ({ item }) => (item.kind === 'folder' ? <FolderRow row={item} onToggle={toggle} /> : <FileRow row={item} notShown={notShown[item.path]} onOpen={open} need={need} />),
    [toggle, open, need, notShown],
  );

  const gone = known && !run;
  const empty = gone
    ? WORDS.unknownAgent
    : files.error && rows.length === 0
      ? files.error
      : changes === null
        ? files.loading || online
          ? text.TEXT.review.checking
          : WORDS.whenReached
        : changes.length === 0
          ? text.TEXT.review.noChanges
          : rows.length === 0
            ? WORDS.changes.noMatch
            : '';

  return (
    <Screen
      id="changes"
      title={WORDS.changes.title}
      {...(run ? { subtitle: run.title } : {})}
      footer={
        <View style={styles.footer}>
          <Field testID="changes.filter" label={WORDS.changes.filter} placeholder={WORDS.changes.filter} icon="search" value={query} onChangeText={setQuery} clear={WORDS.changes.clearFilter} />
          <Tap
            testID="changes.comparison"
            accessibilityLabel={`${text.TEXT.review.comparison}: ${comparison?.label ?? text.TEXT.review.unavailable}`}
            haptic="selection"
            onPress={() => setChoosing(true)}
            style={styles.comparison}
          >
            <Icon name="git-compare" size="md" tone="muted" />
            <Txt testID="changes.comparison.label" kind="label" numberOfLines={1} style={styles.comparisonText}>
              {comparison?.label ?? text.TEXT.review.comparison}
            </Txt>
            <Icon name="chevron-down" size="md" tone="muted" />
          </Tap>
        </View>
      }
    >
      {files.error && rows.length > 0 ? <Notice testID="changes.error" text={files.error} tone="red" /> : null}
      {!online && files.at !== null && changes !== null ? <Notice testID="changes.age" text={WORDS.changes.asOf(text.agoInWords(files.at, now))} /> : null}
      {summary ? (
        <View testID="changes.summary" style={styles.summary} accessible accessibilityLabel={[summary.text, summary.added, summary.removed].filter(Boolean).join(', ')}>
          <Txt kind="label">{summary.text}</Txt>
          {summary.added ? (
            <Txt testID="changes.summary.added" kind="label" tone="green">
              {summary.added}
            </Txt>
          ) : null}
          {summary.removed ? (
            <Txt testID="changes.summary.removed" kind="label" tone="red">
              {summary.removed}
            </Txt>
          ) : null}
        </View>
      ) : null}
      <View style={styles.list}>
        <FlashList
          testID="changes.list"
          {...scrollFrames}
          data={rows}
          renderItem={renderItem}
          keyExtractor={keyOfRow}
          getItemType={typeOfRow}
          extraData={need}
          onRefresh={files.pull}
          refreshing={files.refreshing}
          keyboardShouldPersistTaps="handled"
          ListEmptyComponent={empty ? <Empty testID="changes.empty" text={empty} /> : null}
        />
      </View>
      <ComparisonMenu testID="changes.comparison.menu" open={choosing} onClose={() => setChoosing(false)} choices={files.choices} branches={files.branches} onChoose={files.choose} />
    </Screen>
  );
}
