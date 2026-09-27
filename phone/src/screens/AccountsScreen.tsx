import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function AccountsScreen() {
  return (
    <Screen id="accounts" title="Accounts" back={true}>
      <Empty testID="accounts.empty" text="Not built yet." />
    </Screen>
  );
}
