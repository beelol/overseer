import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function AgentsScreen() {
  return (
    <Screen id="agents" title="Agents" back={false}>
      <Empty testID="agents.empty" text="Not built yet." />
    </Screen>
  );
}
