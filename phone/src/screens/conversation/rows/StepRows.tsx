import { useCallback } from 'react';
import { View } from 'react-native';

import { text, type conversation } from '@/model';
import { Pulse, Tap } from '@/motion';
import { Chip, Icon, makeStyles, Txt } from '@/ui';

import { useRowActions } from '../actions';
import { CrossFade } from '../CrossFade';
import { iconOf } from '../icons';
import { WORDS } from '../words';

const useStyles = makeStyles((theme) => ({
  line: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2], minHeight: theme.space[8] },
  rest: { flex: 1 },
  result: { flexDirection: 'row', alignItems: 'center', gap: theme.space[1] },
  dot: { width: theme.space[2], height: theme.space[2], borderRadius: theme.radius.pill, backgroundColor: theme.colors.blue },
  edits: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2] },
}));

/** Tool calls that follow each other, as one line: "6 steps · Read · Searched". A tap unfolds them. */
export function Steps({ id, row, open }: { readonly id: string; readonly row: conversation.StepsRow; readonly open: boolean }) {
  const styles = useStyles();
  const actions = useRowActions();
  const toggle = useCallback(() => actions.toggle(row.key), [actions, row.key]);
  return (
    <Tap testID={id} accessibilityLabel={[row.label, row.tooltip, row.failedText].filter(Boolean).join(', ')} accessibilityState={{ expanded: open }} haptic="selection" scales={false} onPress={toggle} style={styles.line}>
      <Icon name={iconOf(row.icon, 'tools')} size="md" tone="muted" />
      <Txt kind="label">{row.label}</Txt>
      <Txt testID={`${id}.verbs`} kind="small" tone="muted" numberOfLines={1} style={styles.rest}>
        {row.summary}
      </Txt>
      {row.failedText ? (
        <Txt kind="small" tone="red">
          {row.failedText}
        </Txt>
      ) : null}
      <Icon name={open ? 'chevron-down' : 'chevron-right'} size="sm" tone="faint" />
    </Tap>
  );
}

function resultWords(result: conversation.ToolResult): string {
  switch (result.state) {
    case 'running':
      return text.statusText('running');
    case 'ok':
      return text.TEXT.conversation.done;
    case 'failed':
      return result.text;
    case 'changed':
      return `${result.addedText} ${result.removedText}`;
  }
}

/** A tool call on one line: what it is, what it did and to what. A tap shows all of it. */
export function Tool({ id, row }: { readonly id: string; readonly row: conversation.ToolRow }) {
  const styles = useStyles();
  const actions = useRowActions();
  const open = useCallback(() => actions.openTool(row.key), [actions, row.key]);
  const result = row.result;
  return (
    <Tap testID={id} accessibilityLabel={[`${row.verb} ${row.target}`.trim(), resultWords(result)].join(', ')} accessibilityHint={WORDS.details} haptic="selection" scales={false} onPress={open} style={styles.line}>
      <Icon name={iconOf(row.icon, 'tools')} size="md" tone="muted" />
      <Txt kind="label">{row.verb}</Txt>
      <Txt kind={row.code ? 'mono' : 'small'} tone="muted" numberOfLines={1} style={styles.rest}>
        {row.target}
      </Txt>
      <CrossFade value={result.state}>
        <View testID={`${id}.${result.state}`} style={styles.result}>
          {result.state === 'running' ? (
            <Pulse>
              <View style={styles.dot} />
            </Pulse>
          ) : result.state === 'ok' ? (
            <Icon name="check" size="sm" tone="green" />
          ) : result.state === 'failed' ? (
            <>
              <Icon name="error" size="sm" tone="red" />
              <Txt kind="small" tone="red">
                {result.text}
              </Txt>
            </>
          ) : (
            <>
              <Txt kind="small" tone="green">
                {result.addedText}
              </Txt>
              <Txt kind="small" tone="red">
                {result.removedText}
              </Txt>
            </>
          )}
        </View>
      </CrossFade>
    </Tap>
  );
}

function EditedFile({ id, run, file }: { readonly id: string; readonly run: string; readonly file: conversation.EditRow['files'][number] }) {
  const actions = useRowActions();
  const open = useCallback(() => actions.openFile(run, file.path), [actions, run, file.path]);
  return <Chip testID={id} label={file.name} accessibilityLabel={`${file.name}, ${text.TEXT.conversation.openAtHunk}`} haptic="selection" onPress={open} />;
}

/** The files an edit changed. Each opens the file's changes where the agent edited it. */
export function Edit({ id, row }: { readonly id: string; readonly row: conversation.EditRow }) {
  const styles = useStyles();
  return (
    <View testID={id} style={styles.edits}>
      <Icon name={iconOf(row.icon, 'diff')} size="md" tone="amber" />
      {row.files.map((file, index) => (
        <EditedFile key={file.path} id={`${id}.file.${index}`} run={row.run} file={file} />
      ))}
    </View>
  );
}

/** A native child. What it did follows, one deeper, until it is folded. */
export function Child({ id, row, open }: { readonly id: string; readonly row: conversation.ChildRow; readonly open: boolean }) {
  const styles = useStyles();
  const actions = useRowActions();
  const toggle = useCallback(() => actions.toggle(row.key), [actions, row.key]);
  return (
    <Tap testID={id} accessibilityLabel={`${row.title}, ${row.statusText}`} accessibilityState={{ expanded: open }} haptic="selection" scales={false} onPress={toggle} style={styles.line}>
      <Icon name={iconOf(row.icon, 'type-hierarchy-sub')} size="md" tone="muted" />
      <Txt kind="label" numberOfLines={1} style={styles.rest}>
        {row.title}
      </Txt>
      <CrossFade value={row.status}>
        <Txt testID={`${id}.status`} kind="small" tone={row.status === 'failed' ? 'red' : 'muted'}>
          {row.statusText}
        </Txt>
      </CrossFade>
      <Icon name={open ? 'chevron-down' : 'chevron-right'} size="sm" tone="faint" />
    </Tap>
  );
}
