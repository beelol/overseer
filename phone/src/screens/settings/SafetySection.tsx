import { useCallback, useState } from 'react';

import { Section, SwitchRow } from '@/ui';

import { Note } from './Note';
import type { Safety, SafetyKey } from './safety';
import { SETTINGS } from './words';

/**
 * App lock, and the unlock before changes that cannot be undone. Both are off until the owner
 * turns them on, and both need the device's own unlock: where there is none, it says why.
 */
export function SafetySection({ safety }: { readonly safety: Safety }) {
  const [failed, setFailed] = useState(false);
  const { values, support, change } = safety;
  const usable = support?.supported === true;

  const set = useCallback(
    (key: SafetyKey, on: boolean) => {
      setFailed(false);
      change(key, on, SETTINGS.safety.reason)
        .then((result) => {
          // Turning away from the system's question is a decision, not a failure.
          if (!result.ok && result.cause !== 'cancelled') setFailed(true);
        })
        .catch(() => setFailed(true));
    },
    [change],
  );

  return (
    <>
      <Section title={SETTINGS.safety.title}>
        <SwitchRow
          testID="settings.safety.lock"
          label={SETTINGS.safety.lock}
          value={usable && values.appLock}
          disabled={!usable}
          onChange={(on) => set('appLock', on)}
        />
        <SwitchRow
          testID="settings.safety.unlock"
          label={SETTINGS.safety.unlock}
          value={usable && values.unlockBeforeChanges}
          disabled={!usable}
          divided
          onChange={(on) => set('unlockBeforeChanges', on)}
        />
      </Section>
      {support?.supported === false ? (
        <Note testID="settings.safety.why" text={support.reason} />
      ) : null}
      {failed ? (
        <Note testID="settings.safety.failed" text={SETTINGS.safety.failed} tone="red" alert />
      ) : null}
    </>
  );
}
