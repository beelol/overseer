import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function ConversationScreen() {
  return (
    <Screen id="agent" title="Agent" back={true}>
      <Empty testID="agent.empty" text="Not built yet." />
    </Screen>
  );
}
