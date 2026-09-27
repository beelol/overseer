import { memo, useCallback, useMemo } from 'react';
import { View, type AccessibilityActionEvent } from 'react-native';

import { text, type agents } from '@/model';
import { Pulse, Tap } from '@/motion';
import { useTheme } from '@/theme';
import { Icon, Logo, makeStyles, Txt, type IconName } from '@/ui';

import { StatusLine } from './StatusLine';
import { SwipeAction } from './SwipeAction';
import { WORDS } from './words';

type Row = agents.AgentRow;

export interface AgentRowViewProps {
  readonly row: Row;
  /** The stop of this agent was asked for and it has not stopped yet. */
  readonly stopping: boolean;
  /** False on a watch-only phone: nothing that changes something on the Mac. */
  readonly canChange: boolean;
  readonly onOpen: (row: Row) => void;
  readonly onMore: (row: Row) => void;
  readonly onArchive: (row: Row) => void;
}

/** The test id of a row: `agents.row.<run id>`; in the Needs you section `agents.needs.<run id>`. */
export function rowTestID(row: Row): string {
  return `${row.kind === 'needs' ? 'agents.needs' : 'agents.row'}.${row.runId ?? row.id}`;
}

/** True for a row that waits for the owner: it carries the mark. */
export function needsOwner(row: Row): boolean {
  return row.kind === 'needs' || row.status === 'waiting_for_user';
}

const useStyles = makeStyles((theme) => ({
  row: { minHeight: theme.phone.size.touch + theme.space[3], paddingHorizontal: theme.space[4], flexDirection: 'row', alignItems: 'stretch', gap: theme.space[3], backgroundColor: theme.colors.bg },
  indent: { alignItems: 'flex-end' },
  line: { flex: 1, width: theme.phone.size.hairline, marginRight: theme.phone.size.logo.row / 2, backgroundColor: theme.colors.borderStrong },
  picture: { width: theme.phone.size.logo.row, alignItems: 'center', justifyContent: 'center' },
  texts: { flex: 1, justifyContent: 'center', paddingVertical: theme.space[3], gap: theme.space[1] },
  end: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2] },
  mark: { width: theme.space[2], height: theme.space[2], borderRadius: theme.radius.pill },
  orange: { backgroundColor: theme.colors.amber },
  red: { backgroundColor: theme.colors.red },
  green: { backgroundColor: theme.colors.green },
}));

const ICON: Readonly<Record<string, IconName>> = { terminal: 'terminal', hubot: 'hubot' };

const ARCHIVE = 'archive';
const MORE = 'longpress';

/**
 * One agent of the list: the provider's logo, the title on one line and under it the
 * repository, the status in words and the time. A child stands indented under its parent with
 * a line. Tap opens it, a long press offers Pin, Archive and Stop, a swipe archives.
 */
export const AgentRowView = memo(function AgentRowView({ row, stopping, canChange, onOpen, onMore, onArchive }: AgentRowViewProps) {
  const styles = useStyles();
  const theme = useTheme();
  const testID = rowTestID(row);
  const child = row.kind === 'child';
  const archives = canChange && !child && row.taskId !== null;
  const going = stopping && row.active;

  const open = useCallback(() => onOpen(row), [onOpen, row]);
  const more = useCallback(() => onMore(row), [onMore, row]);
  const archive = useCallback(() => onArchive(row), [onArchive, row]);

  const actions = useMemo(() => [{ name: MORE, label: WORDS.menu }, ...(archives ? [{ name: ARCHIVE, label: text.TEXT.agents.archive }] : [])], [archives]);
  const onAction = useCallback(
    (event: AccessibilityActionEvent) => {
      if (event.nativeEvent.actionName === ARCHIVE) onArchive(row);
      else if (event.nativeEvent.actionName === MORE) onMore(row);
    },
    [onArchive, onMore, row],
  );

  const mark = row.badgeTone === 'red' ? styles.red : row.badgeTone === 'green' ? styles.green : styles.orange;
  const words = going ? WORDS.stopping : row.kind === 'needs' ? row.description : row.statusText;
  const detail = row.kind === 'needs' ? (row.tooltip.split('\n')[1] ?? '') : going ? '' : row.description;

  return (
    <SwipeAction id={row.id} enabled={archives} testID={`${testID}.archive`} label={text.TEXT.agents.archive} accessibilityLabel={`${text.TEXT.agents.archive}, ${row.label}`} icon="archive" onAction={archive}>
      <Tap
        testID={testID}
        accessibilityLabel={going ? `${row.accessibilityLabel}, ${WORDS.stopping}` : row.accessibilityLabel}
        accessibilityActions={actions}
        onAccessibilityAction={onAction}
        onPress={open}
        onLongPress={more}
        haptic="selection"
        scales={false}
        style={styles.row}
      >
        {row.depth > 1 ? (
          <View testID={`${testID}.under`} style={[styles.indent, { width: (row.depth - 1) * theme.phone.size.logo.row }]}>
            <View style={styles.line} />
          </View>
        ) : null}
        <View style={styles.picture}>{row.logo ? <Logo name={row.logo} size={child ? 'header' : 'row'} /> : <Icon name={ICON[row.icon ?? ''] ?? 'hubot'} size="lg" tone="muted" />}</View>
        <View style={styles.texts}>
          <Txt testID={`${testID}.title`} kind={child ? 'label' : 'strong'} numberOfLines={1}>
            {row.label}
          </Txt>
          <StatusLine id={row.id} testID={`${testID}.status`} repo={child || !row.repo ? null : text.basename(row.repo)} words={words} tone={going ? 'quiet' : row.badgeTone} detail={detail} />
        </View>
        <View style={styles.end}>
          {row.pinned ? <Icon name="pinned" size="sm" tone="faint" /> : null}
          {needsOwner(row) ? (
            <Pulse>
              <View testID={`${testID}.mark`} style={[styles.mark, mark]} />
            </Pulse>
          ) : null}
        </View>
      </Tap>
    </SwipeAction>
  );
});
