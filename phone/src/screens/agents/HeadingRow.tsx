import { memo, useCallback } from 'react';
import { View } from 'react-native';

import type { agents } from '@/model';
import { Tap } from '@/motion';
import { Icon, makeStyles, Txt, type IconName } from '@/ui';

type Row = agents.AgentRow;

export interface HeadingRowProps {
  readonly row: Row;
  readonly onFold: (row: Row) => void;
}

/** The test id of a heading: `agents.section.needs`, `agents.repo.<name>`. */
export function headingTestID(row: Row): string {
  if (row.kind === 'repo') return `agents.repo.${row.label}`;
  if (row.kind === 'section') return `agents.section.${row.id.slice(row.id.indexOf(':') + 1)}`;
  return 'agents.notice';
}

const useStyles = makeStyles((theme) => ({
  heading: { minHeight: theme.phone.size.touch, paddingHorizontal: theme.space[4], paddingTop: theme.space[3], paddingBottom: theme.space[1], flexDirection: 'row', alignItems: 'center', gap: theme.space[2], backgroundColor: theme.colors.bg },
  label: { flexShrink: 1 },
  fill: { flex: 1 },
}));

const ICON: Readonly<Record<string, IconName>> = { 'bell-dot': 'bell-dot', repo: 'repo', warning: 'warning' };

/**
 * A quiet heading of the list: "Needs you" with its count, a repository with the number of its
 * agents that are going. Tapping it folds what is under it, and unfolds it again.
 */
export const HeadingRow = memo(function HeadingRow({ row, onFold }: HeadingRowProps) {
  const styles = useStyles();
  const fold = useCallback(() => onFold(row), [onFold, row]);
  const content = (
    <>
      <Icon name={ICON[row.icon ?? ''] ?? 'repo'} size="sm" tone={row.badgeTone === 'orange' ? 'amber' : 'muted'} />
      <Txt kind="small" tone="muted" numberOfLines={1} style={styles.label}>
        {row.kind === 'section' ? row.label.toUpperCase() : row.label}
      </Txt>
      {row.description ? (
        <Txt kind="small" tone={row.badgeTone === 'orange' ? 'amber' : 'muted'}>
          {row.description}
        </Txt>
      ) : null}
      <View style={styles.fill} />
      {row.expandable ? <Icon name={row.expanded ? 'chevron-down' : 'chevron-right'} size="sm" tone="faint" /> : null}
    </>
  );
  if (!row.expandable) {
    return (
      <View testID={headingTestID(row)} accessible accessibilityRole="header" accessibilityLabel={row.accessibilityLabel} style={styles.heading}>
        {content}
      </View>
    );
  }
  return (
    <Tap testID={headingTestID(row)} accessibilityLabel={row.accessibilityLabel} accessibilityState={{ expanded: row.expanded }} onPress={fold} haptic="selection" scales={false} style={styles.heading}>
      {content}
    </Tap>
  );
});
