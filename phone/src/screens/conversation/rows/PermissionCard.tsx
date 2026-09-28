import { useCallback, useEffect, useState } from 'react';
import { TextInput, View } from 'react-native';

import { conversation, text } from '@/model';
import { Tap } from '@/motion';
import { useCapabilities } from '@/platform';
import { lineHeight, useTheme } from '@/theme';
import { Actions, Button, Icon, makeStyles, MAX_TEXT_SCALE, Txt } from '@/ui';

import { useRowActions } from '../actions';
import type { Answer } from '../answers';
import { iconOf } from '../icons';
import { useOpen } from '../open';
import { answered, WORDS } from '../words';

const useStyles = makeStyles((theme) => ({
  card: { gap: theme.space[3], paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], borderRadius: theme.radius.card, backgroundColor: theme.colors.raised, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.amber },
  head: { flexDirection: 'row', alignItems: 'flex-start', gap: theme.space[2] },
  words: { flex: 1 },
  settled: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2], minHeight: theme.space[6] },
  preview: { paddingHorizontal: theme.space[3], paddingVertical: theme.space[2], borderRadius: theme.radius.control, backgroundColor: theme.colors.raised2 },
  request: { flexDirection: 'row', alignItems: 'center', gap: theme.space[1], minHeight: theme.space[8], alignSelf: 'flex-start' },
  field: {
    minHeight: theme.phone.size.touch,
    paddingHorizontal: theme.space[3],
    paddingVertical: theme.space[2],
    borderRadius: theme.radius.control,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.borderStrong,
    backgroundColor: theme.colors.bg,
    color: theme.colors.text,
    fontSize: theme.font.xl,
    lineHeight: lineHeight(theme.font.xl, theme.line.tight),
  },
}));

export interface PermissionCardProps {
  readonly id: string;
  readonly row: conversation.PermissionRow;
  /** What this phone knows of the answer before the daemon's event says it. */
  readonly answer: Answer | undefined;
  readonly watch: boolean;
  /** True when the request came live: it is felt. */
  readonly arriving: boolean;
}

/** What the agent asks for, as the owner says it: "Create perm.txt". */
function whatOf(row: conversation.PermissionRow): string {
  const d = conversation.describe(row.tool, row.input);
  return `${d.pending || d.verb} ${d.target || ''}`.trim();
}

/**
 * A permission request: what the agent wants to do and on what, Allow and Deny. Once answered
 * it is one quiet line that says by whom, here or on the Mac.
 */
export function PermissionCard({ id, row, answer, watch, arriving }: PermissionCardProps) {
  const styles = useStyles();
  const theme = useTheme();
  const actions = useRowActions();
  const { haptics } = useCapabilities();
  const [denying, , setDenying] = useOpen(`${row.key}.deny`);
  const [requestShown, toggleRequest] = useOpen(`${row.key}.request`);
  const [why, setWhy] = useState('');
  const request = String(row.requestId);

  const waits = row.state === 'pending';
  // Felt once, when the request arrives while the conversation is open.
  useEffect(() => {
    if (arriving && waits) haptics.play('warning');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const allow = useCallback(() => actions.answer(request, true, ''), [actions, request]);
  const deny = useCallback(() => {
    if (!denying) setDenying(true);
    else actions.answer(request, false, why);
  }, [actions, request, denying, setDenying, why]);

  const known = answer !== undefined && answer.state !== 'failed' ? answer : undefined;
  if (row.state === 'allowed' || row.state === 'denied' || known !== undefined) {
    const allowed = row.state === 'allowed' || (row.state !== 'denied' && known?.allow === true);
    const words = `${answered(allowed, row.by ?? known?.by)} · ${whatOf(row)}`;
    const onItsWay = waits && known?.state === 'sending';
    return (
      <View testID={id} accessible accessibilityLabel={words} accessibilityLiveRegion="polite" style={styles.settled}>
        <Icon name={allowed ? 'check' : 'circle-slash'} size="md" tone={allowed ? 'green' : 'muted'} />
        <Txt testID={`${id}.text`} kind="small" tone="muted" style={styles.words}>
          {words}
        </Txt>
        {onItsWay ? (
          <Txt testID={`${id}.mark`} kind="small" tone="muted">
            {text.PHONE_ONLY.sending}
          </Txt>
        ) : null}
      </View>
    );
  }
  if (!waits) {
    return (
      <View testID={id} accessible accessibilityLabel={row.text} style={styles.settled}>
        <Icon name={iconOf(row.icon, 'shield')} size="md" tone="muted" />
        <Txt testID={`${id}.text`} kind="small" tone="muted" style={styles.words}>
          {row.text}
        </Txt>
      </View>
    );
  }
  return (
    <View testID={id} accessibilityRole="alert" style={styles.card}>
      <View style={styles.head}>
        <Icon name={iconOf(row.icon, 'shield')} size="lg" tone="amber" />
        <Txt testID={`${id}.text`} kind="strong" style={styles.words}>
          {row.text}
        </Txt>
      </View>
      {row.full && row.full !== whatOf(row) ? (
        <Txt testID={`${id}.full`} kind="small" tone="muted">
          {row.full}
        </Txt>
      ) : null}
      {row.preview ? (
        <View style={styles.preview}>
          <Txt testID={`${id}.preview`} kind="mono">
            {row.preview.trimEnd()}
          </Txt>
        </View>
      ) : null}
      <Tap testID="agent.permission.request" accessibilityLabel={text.TEXT.conversation.request} accessibilityHint={text.TEXT.conversation.requestHint} accessibilityState={{ expanded: requestShown }} haptic="selection" scales={false} onPress={toggleRequest} style={styles.request}>
        <Txt kind="small" tone="link">
          {text.TEXT.conversation.request}
        </Txt>
        <Icon name={requestShown ? 'chevron-down' : 'chevron-right'} size="sm" tone="faint" />
      </Tap>
      {requestShown ? (
        <View style={styles.preview}>
          <Txt testID="agent.permission.request.text" kind="mono">
            {conversation.requestText(row)}
          </Txt>
        </View>
      ) : null}
      {answer?.state === 'failed' ? (
        <Txt testID={`${id}.failed`} kind="small" tone="red" accessibilityLiveRegion="polite">
          {text.PHONE_ONLY.notSent}
        </Txt>
      ) : null}
      {watch ? null : (
        <>
          {denying ? (
            <TextInput
              testID="agent.permission.message"
              accessibilityLabel={WORDS.whyLabel}
              placeholder={WORDS.why}
              placeholderTextColor={theme.colors.muted}
              selectionColor={theme.colors.accent}
              maxFontSizeMultiplier={MAX_TEXT_SCALE}
              value={why}
              onChangeText={setWhy}
              autoFocus
              returnKeyType="send"
              onSubmitEditing={deny}
              style={styles.field}
            />
          ) : null}
          <Actions>
            {conversation.permissionActions(row).map((action) =>
              action.allow ? (
                <Button key="allow" testID="agent.permission.allow" label={action.label} kind="primary" haptic="confirm" onPress={allow} />
              ) : (
                <Button key="deny" testID="agent.permission.deny" label={action.label} kind={denying ? 'danger' : 'secondary'} haptic={denying ? 'reject' : 'selection'} onPress={deny} />
              ),
            )}
          </Actions>
        </>
      )}
    </View>
  );
}
