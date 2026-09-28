import { FlashList, type FlashListRef, type ListRenderItemInfo } from '@shopify/flash-list';
import { forwardRef, memo, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactElement } from 'react';
import { View, type NativeScrollEvent, type NativeSyntheticEvent } from 'react-native';

import { text, type conversation } from '@/model';
import { Arrive, Pulse, Tap } from '@/motion';
import { Icon, makeStyles, Txt } from '@/ui';

import type { Answer } from './answers';
import { CrossFade } from './CrossFade';
import { ConversationRow } from './Row';
import { WORDS } from './words';

type Row = conversation.Row;

/** What the rows need besides themselves. The same object until one of its parts changes. */
export interface ListExtra {
  /** Folds of steps that were opened and children that were closed. */
  readonly toggled: ReadonlySet<string>;
  readonly answers: ReadonlyMap<string, Answer>;
  /** The request ids of the messages that wait on the phone. */
  readonly held: ReadonlySet<string>;
  /** By child run: the depths of the children that hold its rows. */
  readonly lines: ReadonlyMap<string, readonly number[]>;
  readonly watch: boolean;
}

export interface ConversationListRef {
  /** Goes to the newest row and stays there. */
  toEnd(): void;
}

export interface ConversationListProps {
  readonly rows: readonly Row[];
  readonly extra: ListExtra;
  /** Said under the rows while the agent works. */
  readonly working: { readonly shown: boolean; readonly label: string };
  /** Quiet lines above the rows: older messages gone, history not loaded. */
  readonly header: ReactElement | null;
}

/** Within this much of the list's height from the end, the list counts as at the end. */
const NEAR_THE_END = 0.2;

const useStyles = makeStyles((theme) => ({
  fill: { flex: 1 },
  content: { paddingTop: theme.space[3], paddingBottom: theme.space[4] },
  working: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.chat.gutterNarrow, paddingTop: theme.space[3], minHeight: theme.space[8] },
  dot: { width: theme.space[2], height: theme.space[2], borderRadius: theme.radius.pill, backgroundColor: theme.colors.blue },
  label: { flex: 1 },
  latestAt: { position: 'absolute', bottom: theme.space[3], alignSelf: 'center' },
  latest: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.space[2],
    minHeight: theme.space[8],
    paddingHorizontal: theme.space[4],
    borderRadius: theme.radius.pill,
    backgroundColor: theme.colors.raised,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.borderStrong,
  },
}));

const keyOf = (row: Row): string => row.key;
const typeOf = (row: Row): string => row.kind;

function renderRow({ item, extraData }: ListRenderItemInfo<Row>): ReactElement {
  const extra = extraData as ListExtra;
  return (
    <ConversationRow
      key={item.key}
      row={item}
      open={item.kind === 'child' ? !extra.toggled.has(item.key) : extra.toggled.has(item.key)}
      lines={extra.lines.get(item.run)}
      answer={item.kind === 'permission' ? extra.answers.get(String(item.requestId)) : undefined}
      held={item.kind === 'user' && item.requestId !== null && extra.held.has(item.requestId)}
      watch={extra.watch}
    />
  );
}

const Working = memo(function Working({ label }: { readonly label: string }) {
  const styles = useStyles();
  return (
    <View testID="agent.working" accessible accessibilityLabel={label} accessibilityLiveRegion="polite" style={styles.working}>
      <Pulse>
        <View style={styles.dot} />
      </Pulse>
      <CrossFade value={label} style={styles.label}>
        <Txt kind="small" tone="muted" numberOfLines={1}>
          {label}
        </Txt>
      </CrossFade>
    </View>
  );
});

/**
 * The conversation, newest at the bottom. Only the rows on screen are built. It stays at the
 * newest row while rows arrive, until the person scrolls up; then a small button leads back.
 */
export const ConversationList = memo(
  forwardRef<ConversationListRef, ConversationListProps>(function ConversationList({ rows, extra, working, header }, ref) {
    const styles = useStyles();
    const list = useRef<FlashListRef<Row>>(null);
    /** The person reads further up: rows that arrive are announced, not scrolled to. */
    const away = useRef(false);
    /** On its way to the newest row by itself, after Send or the button. */
    const heading = useRef(false);
    const count = useRef(rows.length);
    const [latest, setLatest] = useState<'none' | 'latest' | 'new'>('none');

    const toEnd = useCallback(() => {
      heading.current = true;
      away.current = false;
      setLatest('none');
      list.current?.scrollToEnd({ animated: true });
    }, []);
    useImperativeHandle(ref, () => ({ toEnd }), [toEnd]);

    const onScroll = useCallback((event: NativeSyntheticEvent<NativeScrollEvent>) => {
      const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
      const isAway = contentOffset.y + layoutMeasurement.height < contentSize.height - layoutMeasurement.height * NEAR_THE_END;
      if (heading.current) {
        // What it passes on its way is not where the person reads.
        if (!isAway) heading.current = false;
        return;
      }
      if (isAway === away.current) return;
      away.current = isAway;
      setLatest(isAway ? 'latest' : 'none');
    }, []);
    const onDrag = useCallback(() => {
      heading.current = false;
    }, []);

    useEffect(() => {
      const before = count.current;
      count.current = rows.length;
      if (rows.length <= before) return undefined;
      if (away.current) {
        setLatest('new');
        return undefined;
      }
      if (!heading.current) return undefined;
      // The end moved while the list was on its way to it: what was just sent is the end now.
      const frame = requestAnimationFrame(() => list.current?.scrollToEnd({ animated: true }));
      return () => cancelAnimationFrame(frame);
    }, [rows]);

    const pinned = useMemo(() => ({ autoscrollToBottomThreshold: NEAR_THE_END, startRenderingFromBottom: true }), []);
    const footer = useMemo(() => (working.shown ? <Working label={working.label} /> : null), [working.shown, working.label]);

    return (
      <View style={styles.fill}>
        <FlashList
          ref={list}
          testID="agent.list"
          data={rows}
          extraData={extra}
          renderItem={renderRow}
          keyExtractor={keyOf}
          getItemType={typeOf}
          maintainVisibleContentPosition={pinned}
          onScroll={onScroll}
          onScrollBeginDrag={onDrag}
          scrollEventThrottle={16}
          keyboardDismissMode="on-drag"
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={styles.content}
          ListHeaderComponent={header}
          ListFooterComponent={footer}
        />
        {latest !== 'none' ? (
          <Arrive style={styles.latestAt}>
            <Tap testID="agent.latest" accessibilityLabel={latest === 'new' ? WORDS.newMessages : text.TEXT.chat.jumpToLatest} haptic="selection" onPress={toEnd} style={styles.latest}>
              <Icon name="arrow-down" size="sm" tone="text" />
              <Txt kind="label">{latest === 'new' ? WORDS.newMessages : text.TEXT.chat.latest}</Txt>
            </Tap>
          </Arrive>
        ) : null}
      </View>
    );
  }),
);
