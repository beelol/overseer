import { memo, useEffect } from 'react';
import { View } from 'react-native';

import { review, text } from '@/model';
import { Tap } from '@/motion';
import { useTheme } from '@/theme';
import { Icon, makeStyles, Txt } from '@/ui';

import { WORDS } from './words';

const useStyles = makeStyles((theme) => ({
  row: { minHeight: theme.phone.size.touch, paddingRight: theme.space[4], paddingVertical: theme.space[2], flexDirection: 'row', alignItems: 'center', gap: theme.space[2], borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
  letter: { width: theme.phone.size.icon.lg, textAlign: 'center' },
  texts: { flex: 1, gap: theme.space[1] },
  counts: { flexDirection: 'row', gap: theme.space[2], alignItems: 'center' },
  A: { color: theme.colors.green },
  D: { color: theme.colors.red },
  M: { color: theme.colors.amber },
  R: { color: theme.colors.blue },
  U: { color: theme.colors.pink },
}));

/** Rows deeper than this are drawn no further in: a phone is narrow. */
const DEEPEST = 4;

function useIndent(depth: number): { readonly paddingLeft: number } {
  const theme = useTheme();
  return { paddingLeft: theme.space[4] + Math.min(depth, DEEPEST) * theme.space[3] };
}

export interface FolderRowProps {
  readonly row: review.FileRow;
  readonly onToggle: (key: string) => void;
}

/** A folder of the changed files. Pressed, it folds or unfolds. */
export const FolderRow = memo(function FolderRow({ row, onToggle }: FolderRowProps) {
  const styles = useStyles();
  const indent = useIndent(row.depth);
  return (
    <Tap
      testID={`changes.folder.${row.key}`}
      accessibilityLabel={row.expanded ? WORDS.changes.fold(row.name) : WORDS.changes.unfold(row.name)}
      accessibilityState={{ expanded: row.expanded }}
      scales={false}
      haptic="selection"
      onPress={() => onToggle(row.key)}
      style={[styles.row, indent]}
    >
      <Icon name={row.expanded ? 'chevron-down' : 'chevron-right'} size="md" tone="muted" />
      <Icon name={row.expanded ? 'folder-opened' : 'folder'} size="md" tone="muted" />
      <View style={styles.texts}>
        <Txt kind="label" tone="muted">
          {row.name}
        </Txt>
      </View>
    </Tap>
  );
});

export interface FileRowProps {
  readonly row: review.FileRow;
  /** Why the file is not shown as text, when it is not. */
  readonly notShown: string | undefined;
  readonly onOpen: (path: string) => void;
  /** Asks for the file's hunks. A new function asks again. */
  readonly need: (path: string) => void;
}

/** A changed file: its letter, its path with the name strong, its counts and how much is reviewed. */
export const FileRow = memo(function FileRow({ row, notShown, onOpen, need }: FileRowProps) {
  const styles = useStyles();
  const indent = useIndent(row.depth);
  useEffect(() => need(row.path), [need, row.path]);

  const folder = row.path.slice(0, row.path.length - row.name.length);
  const counted = row.added !== null && row.removed !== null;
  const reviewed = row.hunks !== null && row.reviewed !== null && row.hunks > 0 ? WORDS.changes.reviewed(row.reviewed, row.hunks) : null;
  const all = row.hunks !== null && row.hunks > 0 && row.reviewed === row.hunks;
  const added = counted ? text.TEXT.conversation.added(row.added ?? 0) : '';
  const removed = counted ? text.TEXT.conversation.removed(row.removed ?? 0) : '';
  const said = [row.accessibilityLabel, row.statusText, counted ? `${added} ${removed}` : null, reviewed, notShown].filter(Boolean).join(', ');

  return (
    <Tap testID={`changes.row.${row.path}`} accessibilityLabel={said} scales={false} haptic="selection" onPress={() => onOpen(row.path)} style={[styles.row, indent]}>
      <Txt testID={`changes.row.${row.path}.status`} kind="mono" style={[styles.letter, row.status ? styles[row.status] : null]}>
        {row.status ?? ''}
      </Txt>
      <View style={styles.texts}>
        <Txt kind="label" tone="muted">
          {folder}
          <Txt kind="strong">{row.name}</Txt>
        </Txt>
        {row.statusText ? (
          <Txt kind="small" tone={row.conflicted ? 'red' : 'muted'}>
            {row.statusText}
          </Txt>
        ) : null}
        {notShown ? (
          <Txt kind="small" tone="muted">
            {notShown}
          </Txt>
        ) : null}
        {reviewed ? (
          <Txt testID={`changes.row.${row.path}.reviewed`} kind="small" tone={all ? 'green' : 'muted'}>
            {reviewed}
          </Txt>
        ) : null}
      </View>
      {counted ? (
        <View style={styles.counts}>
          <Txt testID={`changes.row.${row.path}.added`} kind="label" tone="green">
            {added}
          </Txt>
          <Txt testID={`changes.row.${row.path}.removed`} kind="label" tone="red">
            {removed}
          </Txt>
        </View>
      ) : null}
      <Icon name="chevron-right" size="md" tone="faint" />
    </Tap>
  );
});
