import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function NewAgentScreen() {
  return (
    <Screen id="new" title="New agent" back={true}>
      <Empty testID="new.empty" text="Not built yet." />
    </Screen>
  );
}
