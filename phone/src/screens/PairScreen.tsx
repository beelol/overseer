import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function PairScreen() {
  return (
    <Screen id="pair" title="Pair with your Mac" back={false}>
      <Empty testID="pair.empty" text="Not built yet." />
    </Screen>
  );
}
