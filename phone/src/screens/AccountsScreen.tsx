import { useCallback, useState } from 'react';
import { RefreshControl, ScrollView } from 'react-native';

import { useSessionValue } from '@/session';
import { useTheme } from '@/theme';
import { Empty, makeStyles, Screen, Section, useMinute, WatchOnlyLine } from '@/ui';

import { AccountRow } from './accounts/AccountRow';
import type { Account } from './accounts/accounts';
import { SIGN_IN_POLL_MS, SignInSheet } from './accounts/SignInSheet';
import { useAccounts } from './accounts/useAccounts';
import { ACCOUNTS } from './accounts/words';

const useStyles = makeStyles((theme) => ({
  content: { paddingBottom: theme.space[8] },
}));

export interface AccountsScreenProps {
  /** How often the Mac is asked whether a sign-in has finished. Tests make it short. */
  readonly pollMs?: number;
}

/**
 * Accounts, by provider: signed in or not, the plan, the usage with the time it resets, and
 * signing in with a code. A phone that may only watch sees all of it and signs nothing in.
 */
export function AccountsScreen({ pollMs = SIGN_IN_POLL_MS }: AccountsScreenProps) {
  const styles = useStyles();
  const theme = useTheme();
  const watch = useSessionValue((s) => s.scope === 'watch');
  const { groups, status, usage, checking, refresh, refreshOne } = useAccounts();
  const now = useMinute();
  const [signing, setSigning] = useState<Account | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const close = useCallback(() => setSigning(null), []);
  const signedIn = useCallback(
    (id: string) => {
      setSigning(null);
      void refreshOne(id);
    },
    [refreshOne],
  );
  const again = useCallback(() => {
    setRefreshing(true);
    refresh().finally(() => setRefreshing(false));
  }, [refresh]);

  return (
    <Screen id="accounts" title={ACCOUNTS.title} {...(watch ? { footer: <WatchOnlyLine /> } : {})}>
      {groups.length === 0 ? (
        <Empty testID="accounts.empty" icon="account" text={ACCOUNTS.empty} />
      ) : (
        <ScrollView
          testID="accounts.list"
          contentContainerStyle={styles.content}
          refreshControl={
            <RefreshControl
              refreshing={refreshing}
              onRefresh={again}
              tintColor={theme.colors.muted}
            />
          }
        >
          {groups.map((group) => (
            <Section key={group.id} title={group.name}>
              {group.accounts.map((account, index) => (
                <AccountRow
                  key={account.id}
                  account={account}
                  logo={group.logo}
                  status={status.get(account.id)}
                  usage={usage.get(account.id)}
                  checking={checking}
                  watch={watch}
                  now={now}
                  divided={index > 0}
                  onSignIn={setSigning}
                />
              ))}
            </Section>
          ))}
        </ScrollView>
      )}
      {watch ? null : (
        <SignInSheet account={signing} onClose={close} onSignedIn={signedIn} pollMs={pollMs} />
      )}
    </Screen>
  );
}
