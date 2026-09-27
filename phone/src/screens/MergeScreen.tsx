import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function MergeScreen() {
  return (
    <Screen id="merge" title="Merge back" back={true}>
      <Empty testID="merge.empty" text="Not built yet." />
    </Screen>
  );
}
