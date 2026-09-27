import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function ChangesScreen() {
  return (
    <Screen id="changes" title="Changes" back={true}>
      <Empty testID="changes.empty" text="Not built yet." />
    </Screen>
  );
}
