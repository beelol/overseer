import { Empty, Screen } from '@/ui';

/** A place held for the screen of the brief (phone/docs/app-spec.md). */
export function PullRequestScreen() {
  return (
    <Screen id="pr" title="Pull request" back={true}>
      <Empty testID="pr.empty" text="Not built yet." />
    </Screen>
  );
}
