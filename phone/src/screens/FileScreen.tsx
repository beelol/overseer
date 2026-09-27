import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function FileScreen() {
  return (
    <Screen id="file" title="File" back={true}>
      <Empty testID="file.empty" text="Not built yet." />
    </Screen>
  );
}
