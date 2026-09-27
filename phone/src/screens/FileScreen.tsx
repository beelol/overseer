import { FlashList, type ListRenderItem } from '@shopify/flash-list';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Animated, useWindowDimensions, View } from 'react-native';

import { review, text } from '@/model';
import { useCapabilities } from '@/platform';
import { useSession, useSessionValue } from '@/session';
import { useTheme } from '@/theme';
import { Confirm, Empty, IconButton, makeStyles, MAX_TEXT_SCALE, Screen, Txt, WatchOnlyLine } from '@/ui';

import { useActivity, useRefreshOn } from './review/activity';
import { useReviewStore } from './review/comparison';
import { DiffLine, HunkHeading } from './review/DiffRows';
import { changedSince, sentence } from './review/errors';
import { fileItems, placeOfHunk, useFileChanges, type FileItem } from './review/file';
import { useMarks } from './review/marks';
import { Notice } from './review/Notice';
import { useReviewParams } from './review/run';
import { WORDS } from './review/words';

const useStyles = makeStyles((theme) => ({
  fill: { flex: 1 },
  about: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.space[4], paddingVertical: theme.space[2], borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
}));

/** How wide a character of the mono font is, as a part of its size. */
const ADVANCE = 0.6;

const keyOfItem = (item: FileItem): string => item.key;
const typeOfItem = (item: FileItem): string => item.kind;

/**
 * One file's changes (phone/docs/app-spec.md, A file's changes): its hunks, each with its
 * removed and added lines, Accept and Reject. Every line is a row of the list.
 */
export function FileScreen() {
  const styles = useStyles();
  const theme = useTheme();
  const session = useSession();
  const { haptics } = useCapabilities();
  const kept = useReviewStore();
  const window = useWindowDimensions();
  const { run: runId, path = '', comparison, hunk: named } = useReviewParams();
  const marks = useMarks(runId);
  const file = useFileChanges(runId, path, comparison, marks.learn);
  const activity = useActivity(runId);
  const watching = useSessionValue((s) => s.scope === 'watch');
  const [wrap, setWrap] = useState(() => kept.get('wrap') ?? true);
  const [hidden, setHidden] = useState<ReadonlySet<string>>(() => new Set());
  const [notice, setNotice] = useState<string | null>(null);
  const [asked, setAsked] = useState<review.HunkView | null>(null);
  const [shift] = useState(() => new Animated.Value(0));
  // How tall the place of the list is: sideways, the list needs its height said.
  const [tall, setTall] = useState<number | null>(null);

  // The file is asked again while the agent edits; a new turn may move the comparison.
  const starts = useRef(activity.starts);
  const { reload } = file;
  const refresh = useCallback(() => {
    const moved = starts.current !== activity.starts;
    starts.current = activity.starts;
    void reload({ comparison: moved });
  }, [reload, activity.starts]);
  useRefreshOn(`${activity.starts}:${activity.edits}:${activity.ends}:${activity.rejects}:${activity.merges}`, activity.ready, refresh);

  // What was remembered of this file says which hunks were marked then, until the marks arrive.
  const { result } = file;
  const { learn } = marks;
  useEffect(() => {
    if (result) learn(result.hunks);
  }, [result, learn]);

  const diff = useMemo(() => (result ? review.fileDiff(result, marks.keys) : null), [result, marks.keys]);
  const list = useMemo(() => (diff ? fileItems(diff, hidden) : null), [diff, hidden]);
  const first = useMemo(() => (diff && list ? placeOfHunk(diff, list, named) : 0), [diff, list, named]);

  // The mono font's size follows the system's text size, as far as the app's text does.
  const character = theme.phone.font.mono * ADVANCE * Math.min(window.fontScale, MAX_TEXT_SCALE);
  const gutter = Math.ceil((list?.digits ?? 1) * character) + theme.space[2] * 2;
  const sign = Math.ceil(character) + theme.space[1] * 2;
  const columns = wrap ? Math.max(8, Math.floor((window.width - gutter - sign - theme.space[3]) / character)) : null;
  const across = Math.max(window.width, gutter + sign + theme.space[3] + Math.ceil(((list?.longest ?? 0) + 1) * character));

  const toggleWrap = useCallback(() => {
    setWrap((before) => {
      kept.set('wrap', !before);
      return !before;
    });
  }, [kept]);

  const refused = useCallback(
    (failure: unknown, changed: string) => {
      haptics.play('reject');
      if (changedSince(failure)) {
        setNotice(changed);
        void reload();
      } else setNotice(sentence(failure));
    },
    [haptics, reload],
  );

  const accept = useCallback(
    (hunk: review.HunkView) => {
      setNotice(null);
      (hunk.reviewed ? marks.unaccept(hunk.key) : marks.accept(path, hunk.hunk)).catch((failure: unknown) => refused(failure, WORDS.file.changedSinceAccept));
    },
    [marks, path, refused],
  );

  const { workspaceId, base } = file;
  const { forget } = marks;
  const putBack = useCallback(
    async (hunk: review.HunkView) => {
      if (workspaceId === null || base === null) return;
      setNotice(null);
      // Shown at once: the hunk leaves the list while the Mac puts the lines back.
      setHidden((before) => new Set(before).add(hunk.key));
      try {
        await session.request('review.reject', { workspace_id: workspaceId, path, base, key: hunk.key });
        forget(hunk.key);
        await reload();
      } catch (failure) {
        refused(failure, WORDS.file.changedSince);
      } finally {
        setHidden((before) => {
          const next = new Set(before);
          next.delete(hunk.key);
          return next;
        });
      }
    },
    [session, workspaceId, base, path, forget, reload, refused],
  );

  const controls = !watching;
  const language = list?.language ?? null;
  const held = useMemo(() => (wrap ? undefined : { shift, width: window.width }), [wrap, shift, window.width]);
  const renderItem = useCallback<ListRenderItem<FileItem>>(
    ({ item }) =>
      item.kind === 'hunk' ? (
        <HunkHeading hunk={item.hunk} controls={controls} onAccept={accept} onReject={setAsked} {...(held ? { held } : {})} />
      ) : (
        <DiffLine row={item.row} text={item.text} more={item.more} language={language} columns={columns} gutter={gutter} />
      ),
    [controls, accept, held, language, columns, gutter],
  );

  const name = text.basename(path) || WORDS.changes.title;
  const folder = path.slice(0, Math.max(0, path.length - name.length));
  const empty = !path ? WORDS.file.nothingNamed : file.error && !diff ? file.error : !diff ? (file.loading ? text.TEXT.review.loading : WORDS.whenReached) : !diff.shown ? (diff.note ?? text.PHONE_ONLY.notShown('')) : list && list.items.length === 0 ? WORDS.file.noChanges : '';

  const rows =
    list && list.items.length > 0 ? (
      <FlashList
        testID="file.list"
        data={list.items}
        renderItem={renderItem}
        keyExtractor={keyOfItem}
        getItemType={typeOfItem}
        extraData={renderItem}
        {...(first > 0 ? { initialScrollIndex: first } : {})}
        {...(wrap ? { stickyHeaderIndices: list.headings } : {})}
      />
    ) : null;

  return (
    <Screen
      id="file"
      title={name}
      {...(folder ? { subtitle: folder } : {})}
      actions={<IconButton testID="file.wrap" accessibilityLabel={wrap ? WORDS.file.unwrap : WORDS.file.wrap} accessibilityState={{ selected: wrap }} icon="word-wrap" tone={wrap ? 'accent' : 'muted'} haptic="selection" onPress={toggleWrap} />}
      footer={watching ? <WatchOnlyLine /> : undefined}
    >
      {notice ? <Notice testID="file.notice" text={notice} tone="red" /> : null}
      {file.error && diff ? <Notice testID="file.error" text={file.error} tone="red" /> : null}
      {diff && diff.shown ? (
        <View testID="file.about" style={styles.about}>
          {file.comparison ? (
            <Txt kind="small" tone="muted">
              {file.comparison}
            </Txt>
          ) : null}
          {diff.note ? (
            <Txt testID="file.note" kind="small" tone="muted">
              {diff.note}
            </Txt>
          ) : null}
          <Txt kind="small" tone="green">
            {text.TEXT.conversation.added(diff.added)}
          </Txt>
          <Txt kind="small" tone="red">
            {text.TEXT.conversation.removed(diff.removed)}
          </Txt>
          {diff.hunks.length > 0 ? (
            <Txt testID="file.reviewed" kind="small" tone={diff.reviewed === diff.hunks.length ? 'green' : 'muted'}>
              {WORDS.changes.reviewed(diff.reviewed, diff.hunks.length)}
            </Txt>
          ) : null}
        </View>
      ) : null}
      {empty ? (
        <Empty testID={diff && !diff.shown ? 'file.notshown' : 'file.empty'} text={empty} {...(diff && !diff.shown ? { icon: 'file-binary' as const } : {})} />
      ) : wrap ? (
        <View style={styles.fill}>{rows}</View>
      ) : (
        <View style={styles.fill} onLayout={(event) => setTall(event.nativeEvent.layout.height)}>
          <Animated.ScrollView
            testID="file.sideways"
            horizontal
            style={styles.fill}
            scrollEventThrottle={16}
            onScroll={Animated.event([{ nativeEvent: { contentOffset: { x: shift } } }], { useNativeDriver: true })}
          >
            <View style={[{ width: across }, tall === null ? null : { height: tall }]}>{rows}</View>
          </Animated.ScrollView>
        </View>
      )}
      <Confirm
        testID="file.reject"
        open={asked !== null}
        onClose={() => setAsked(null)}
        question={asked ? (asked.removed > 0 ? WORDS.file.putBack(asked.removed) : WORDS.file.takeOut(asked.added)) : ''}
        {...(asked ? { detail: asked.removed > 0 ? WORDS.file.putBackDetail(asked.added) || whereText(asked, path) : WORDS.file.takeOutDetail } : {})}
        confirm={WORDS.file.putBackConfirm}
        onConfirm={() => {
          if (asked) void putBack(asked);
        }}
      />
    </Screen>
  );
}

/** What stands under the question when nothing else is lost: which lines, of which file. */
function whereText(hunk: review.HunkView, path: string): string {
  return `${text.basename(path)}, ${hunk.where}`;
}
