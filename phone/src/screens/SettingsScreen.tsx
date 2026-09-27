import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function SettingsScreen() {
  return (
    <Screen id="settings" title="Settings" back={true}>
      <Empty testID="settings.empty" text="Not built yet." />
    </Screen>
  );
}
