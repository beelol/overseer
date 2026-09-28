import { useCallback } from 'react';
import { View } from 'react-native';

import { pending, text, type conversation } from '@/model';
import { Button, makeStyles, Txt } from '@/ui';

import { useRowActions } from '../actions';
import { CrossFade } from '../CrossFade';
import { WORDS } from '../words';

const useStyles = makeStyles((theme) => ({
  side: { alignItems: 'flex-end', gap: theme.space[1] },
  bubble: { maxWidth: '86%', paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], borderRadius: theme.radius.card, backgroundColor: theme.colors.raised2, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.border },
  waiting: { borderStyle: 'dashed', borderColor: theme.colors.borderStrong },
  failed: { borderColor: theme.colors.red },
  actions: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'flex-end', alignItems: 'center', gap: theme.space[1] },
}));

export interface UserBubbleProps {
  readonly id: string;
  readonly row: conversation.UserRow;
  /** True while the message waits on the phone for the turn to end: it can be taken back. */
  readonly held: boolean;
  readonly watch: boolean;
}

/** Your message. On its way it says so: Queued, Sending, and Not sent with a way to try again. */
export function UserBubble({ id, row, held, watch }: UserBubbleProps) {
  const styles = useStyles();
  const actions = useRowActions();
  const mark = pending.sentLabel(row);
  const request = row.requestId;
  const retry = useCallback(() => request !== null && actions.retry(request), [actions, request]);
  const remove = useCallback(() => request !== null && actions.remove(request), [actions, request]);
  const cancel = useCallback(() => request !== null && actions.cancel(request), [actions, request]);
  const failed = row.sent === 'failed';
  return (
    <View style={styles.side}>
      <View testID={id} accessible accessibilityLabel={[row.label, mark, row.text].filter(Boolean).join(', ')} style={[styles.bubble, row.sent === 'queued' || row.sent === 'sending' ? styles.waiting : null, failed ? styles.failed : null]}>
        <Txt kind="body">{row.text}</Txt>
      </View>
      {mark ? (
        <View style={styles.actions}>
          <CrossFade value={row.sent}>
            <Txt testID={`${id}.mark`} kind="small" tone={failed ? 'red' : 'muted'} accessibilityLiveRegion="polite">
              {mark}
            </Txt>
          </CrossFade>
          {failed && request !== null && !watch ? (
            <>
              <Button testID={`${id}.retry`} label={WORDS.tryAgain} kind="quiet" haptic="selection" onPress={retry} />
              <Button testID={`${id}.remove`} label={WORDS.remove} kind="quiet" haptic="selection" onPress={remove} />
            </>
          ) : null}
          {held && !failed && !watch ? <Button testID={`${id}.cancel`} label={text.TEXT.chat.cancel} kind="quiet" haptic="selection" onPress={cancel} /> : null}
        </View>
      ) : null}
    </View>
  );
}
