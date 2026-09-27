import { memo } from 'react';
import { View } from 'react-native';

import { Button, Logo, makeStyles, Txt } from '@/ui';

import {
  hasCode,
  stateText,
  usageText,
  usageTone,
  type Account,
  type Status,
  type Usage,
  type UsageWindow,
} from './accounts';
import { ACCOUNTS } from './words';

const useStyles = makeStyles((theme) => ({
  row: {
    minHeight: theme.phone.size.touch,
    paddingHorizontal: theme.space[4],
    paddingVertical: theme.space[3],
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    gap: theme.space[3],
  },
  divided: { borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
  texts: { flex: 1, flexBasis: theme.space[10] * BASIS, gap: theme.space[1] },
  window: { gap: theme.space[1], paddingTop: theme.space[1] },
  track: {
    height: theme.space[1],
    borderRadius: theme.radius.pill,
    backgroundColor: theme.colors.borderStrong,
    overflow: 'hidden',
  },
  fill: { height: theme.space[1], borderRadius: theme.radius.pill },
  accent: { backgroundColor: theme.colors.accent },
  amber: { backgroundColor: theme.colors.amber },
  red: { backgroundColor: theme.colors.red },
}));
const BASIS = 4;

export interface AccountRowProps {
  readonly account: Account;
  readonly logo: string;
  readonly status: Status | undefined;
  readonly usage: Usage | undefined;
  /** True while the Mac can be asked for what is not known yet. */
  readonly checking: boolean;
  /** True on a phone that may only watch: it sees everything and signs nothing in. */
  readonly watch: boolean;
  readonly now: number;
  readonly divided: boolean;
  readonly onSignIn: (account: Account) => void;
}

function Window({
  id,
  index,
  window,
  limited,
  now,
}: {
  readonly id: string;
  readonly index: number;
  readonly window: UsageWindow;
  readonly limited: boolean;
  readonly now: number;
}) {
  const styles = useStyles();
  const tone = usageTone(window, limited);
  return (
    <View style={styles.window}>
      <Txt
        testID={`accounts.usage.${id}.${index}`}
        kind="small"
        tone={tone === 'accent' ? 'muted' : tone}
      >
        {usageText(window, now)}
      </Txt>
      <View style={styles.track} accessible={false} importantForAccessibility="no-hide-descendants">
        <View style={[styles.fill, styles[tone], { width: `${Math.round(window.used * 100)}%` }]} />
      </View>
    </View>
  );
}

/** One account: its name, signed in or not, its plan, and its usage with the time it resets. */
export const AccountRow = memo(function AccountRow({
  account,
  logo,
  status,
  usage,
  checking,
  watch,
  now,
  divided,
  onSignIn,
}: AccountRowProps) {
  const styles = useStyles();
  const state = stateText(status, usage, checking);
  const signedOut = status !== undefined && status.installed && !status.signedIn;
  const signedIn = status?.signedIn === true;
  const withCode = hasCode(account);
  const windows = signedIn && usage?.reported ? usage.windows : [];
  const said = [account.name, state, ...windows.map((window) => usageText(window, now))];
  if (signedIn && usage?.limited) said.push(ACCOUNTS.limit);
  if (signedOut && !withCode) said.push(ACCOUNTS.onTheMac);

  return (
    <View
      testID={`accounts.row.${account.id}`}
      style={[styles.row, divided ? styles.divided : null]}
    >
      <Logo name={logo} />
      <View style={styles.texts} accessible accessibilityLabel={said.join(', ')}>
        <Txt kind="strong">{account.name}</Txt>
        <Txt
          testID={`accounts.state.${account.id}`}
          kind="small"
          tone={signedOut ? 'amber' : 'muted'}
        >
          {state}
        </Txt>
        {signedOut && status.apiKey ? (
          <Txt testID={`accounts.note.${account.id}`} kind="small" tone="muted">
            {ACCOUNTS.apiKey}
          </Txt>
        ) : null}
        {windows.map((window, index) => (
          <Window
            key={window.label}
            id={account.id}
            index={index}
            window={window}
            limited={usage?.limited === true}
            now={now}
          />
        ))}
        {signedIn && usage?.limited ? (
          <Txt testID={`accounts.limit.${account.id}`} kind="small" tone="red">
            {ACCOUNTS.limit}
          </Txt>
        ) : null}
        {signedIn && usage !== undefined && !usage.reported ? (
          <Txt testID={`accounts.usage.${account.id}.none`} kind="small" tone="muted">
            {ACCOUNTS.notReported}
          </Txt>
        ) : null}
        {signedOut && !withCode ? (
          <Txt testID={`accounts.onmac.${account.id}`} kind="small" tone="muted">
            {ACCOUNTS.onTheMac}
          </Txt>
        ) : null}
      </View>
      {signedOut && withCode && !watch ? (
        <Button
          testID={`accounts.signin.${account.id}`}
          label={ACCOUNTS.signIn}
          accessibilityLabel={`${ACCOUNTS.signIn}, ${account.name}`}
          kind="primary"
          onPress={() => onSignIn(account)}
        />
      ) : null}
    </View>
  );
});
