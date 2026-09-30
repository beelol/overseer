import { useMemo } from 'react';
import { View } from 'react-native';

import { text, type conversation } from '@/model';
import { Tap } from '@/motion';
import { Button, Icon, makeStyles, Txt, type TxtTone } from '@/ui';

import { useRowActions } from '../actions';
import { iconOf } from '../icons';
import { Markdown } from '../Markdown';
import { useOpen } from '../open';

const useStyles = makeStyles((theme) => ({
  fold: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2], minHeight: theme.space[8] },
  thought: { paddingLeft: theme.space[6], paddingTop: theme.space[1] },
  quiet: { flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap', columnGap: theme.space[3], rowGap: theme.space[1] },
  mark: { flexDirection: 'row', alignItems: 'center', gap: theme.space[1], flexShrink: 1 },
  words: { flexShrink: 1 },
  error: { alignItems: 'flex-start', gap: theme.space[2], paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], borderRadius: theme.radius.card, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.red, backgroundColor: theme.colors.raised },
  errorHead: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2] },
  output: { paddingHorizontal: theme.space[3], paddingVertical: theme.space[2], borderRadius: theme.radius.control, backgroundColor: theme.colors.raised },
}));

/** What the agent said, as formatted text; what a program printed, as it printed it. */
export function Message({ id, row }: { readonly id: string; readonly row: conversation.MessageRow }) {
  const styles = useStyles();
  const actions = useRowActions();
  const blocks = useMemo(() => (row.markdown ? actions.markdownOf(row) : null), [actions, row]);
  if (blocks !== null) {
    return (
      <View testID={id} accessibilityLabel={row.label}>
        <Markdown id={`${id}.md`} blocks={blocks} />
      </View>
    );
  }
  const printed = row.role === 'stdout' || row.role === 'stderr';
  return (
    <View testID={id} accessibilityLabel={row.label} style={printed ? styles.output : null}>
      <Txt kind={printed ? 'mono' : 'body'} tone={row.role === 'stderr' ? 'red' : 'text'}>
        {row.text}
      </Txt>
    </View>
  );
}

/** A thought or a plan: one line until it is opened. */
export function Thinking({ id, row }: { readonly id: string; readonly row: conversation.ThinkingRow }) {
  const styles = useStyles();
  const actions = useRowActions();
  const [open, toggle] = useOpen(row.key);
  const blocks = useMemo(() => (open ? actions.markdownOf(row) : null), [actions, row, open]);
  return (
    <View>
      <Tap testID={id} accessibilityLabel={row.label} accessibilityState={{ expanded: open }} haptic="selection" scales={false} onPress={toggle} style={styles.fold}>
        <Icon name={iconOf(row.icon, 'lightbulb')} size="md" tone="muted" />
        <Txt kind="label" tone="muted">
          {row.label}
        </Txt>
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size="sm" tone="faint" />
      </Tap>
      {blocks !== null ? (
        <View testID={`${id}.text`} style={styles.thought}>
          <Markdown id={`${id}.md`} blocks={blocks} tone="muted" />
        </View>
      ) : null}
    </View>
  );
}

/** A quiet line: how a run ended outside a turn, what was done from a phone, what Continuity did. */
export function Note({ id, row }: { readonly id: string; readonly row: conversation.NoteRow }) {
  const styles = useStyles();
  const actions = useRowActions();
  const tone: TxtTone = row.status === 'failed' || row.status === 'disconnected' ? 'red' : 'muted';
  const link = row.link;
  const line = (
    <View testID={id} accessible accessibilityLabel={[row.text, row.tooltip].filter(Boolean).join(', ')} style={styles.mark}>
      {row.icon ? <Icon name={iconOf(row.icon)} size="sm" tone={tone} /> : null}
      <Txt kind="small" tone={tone} style={styles.words}>
        {row.text}
      </Txt>
    </View>
  );
  if (!link) return line;
  // A handoff names the other agent: one tap opens it.
  return (
    <View style={styles.quiet}>
      {line}
      <Button testID={`${id}.link`} label={link.label} kind="quiet" haptic="selection" onPress={() => actions.openAgent(link.runId)} />
    </View>
  );
}

/** The end of a turn: how it ended, how long it took, tokens and cost. */
export function Footer({ id, row }: { readonly id: string; readonly row: conversation.FooterRow }) {
  const styles = useStyles();
  if (!row.text && !row.duration && !row.usage) return null;
  const tone: TxtTone = row.state === 'fail' ? 'red' : 'muted';
  return (
    <View testID={id} accessible accessibilityLabel={[row.text, row.duration, row.usageDetail || row.usage].filter(Boolean).join(', ')} style={styles.quiet}>
      {row.text ? (
        <View style={styles.mark}>
          {row.icon ? <Icon name={iconOf(row.icon)} size="sm" tone={row.state === 'ok' ? 'green' : tone} /> : null}
          <Txt testID={`${id}.state`} kind="small" tone={tone} style={styles.words}>
            {row.text}
          </Txt>
        </View>
      ) : null}
      {row.duration ? (
        <Txt testID={`${id}.time`} kind="small" tone="muted">
          {row.duration}
        </Txt>
      ) : null}
      {row.usage ? (
        <Txt testID={`${id}.usage`} kind="small" tone="muted">
          {row.usage}
        </Txt>
      ) : null}
    </View>
  );
}

/** An error, in the agent's words. Signed out: the way to the accounts. */
export function Failure({ id, row }: { readonly id: string; readonly row: conversation.ErrorRow }) {
  const styles = useStyles();
  const actions = useRowActions();
  return (
    <View testID={id} accessibilityRole="alert" style={styles.error}>
      <View style={styles.errorHead}>
        <Icon name={iconOf(row.icon, 'error')} size="md" tone="red" />
        <Txt kind="strong" tone="red">
          {row.title}
        </Txt>
      </View>
      <Txt kind="body">{row.message}</Txt>
      {row.signIn ? <Button testID={`${id}.signin`} label={text.TEXT.conversation.signInAgain} accessibilityLabel={text.TEXT.conversation.signInAgainLabel} haptic="selection" onPress={actions.signIn} /> : null}
    </View>
  );
}
