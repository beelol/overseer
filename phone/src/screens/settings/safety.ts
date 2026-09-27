import { useCallback, useMemo, useState } from 'react';

import { useCapabilities, type Support, type SyncStore, type UnlockResult } from '@/platform';

import { useSupport } from './useSupport';

/** The safety settings, kept on the phone under `settings`. Both are off until turned on. */
export type SafetySettings = {
  /** Ask for the device's unlock when the app opens. */
  appLock: boolean;
  /** Ask for the device's unlock before a change that cannot be undone. */
  unlockBeforeChanges: boolean;
};

export type SafetyKey = keyof SafetySettings;

function read(store: SyncStore<SafetySettings>): SafetySettings {
  try {
    return {
      appLock: store.get('appLock') === true,
      unlockBeforeChanges: store.get('unlockBeforeChanges') === true,
    };
  } catch {
    return { appLock: false, unlockBeforeChanges: false };
  }
}

export interface Safety {
  readonly values: SafetySettings;
  /** Whether the device can be unlocked at all. `null` until it has answered. */
  readonly support: Support | null;
  /**
   * Turns a setting on or off, after the owner proved it is them: a lock that anyone holding
   * the phone could turn off would protect nothing.
   */
  change(key: SafetyKey, on: boolean, reason: string): Promise<UnlockResult>;
  /** The unlock before a change that cannot be undone. Passes at once when the setting is off. */
  confirm(reason: string): Promise<UnlockResult>;
}

export function useSafety(): Safety {
  const { keyValue, deviceUnlock } = useCapabilities();
  const store = useMemo(() => keyValue.scope<SafetySettings>('settings'), [keyValue]);
  const [values, setValues] = useState(() => read(store));
  const support = useSupport(deviceUnlock);

  const unlock = useCallback(
    async (reason: string): Promise<UnlockResult> => {
      try {
        return await deviceUnlock.unlock({ reason });
      } catch {
        return { ok: false, cause: 'unavailable' };
      }
    },
    [deviceUnlock],
  );

  const change = useCallback(
    async (key: SafetyKey, on: boolean, reason: string): Promise<UnlockResult> => {
      const result = await unlock(reason);
      if (!result.ok) return result;
      try {
        store.set(key, on);
      } catch {
        return { ok: false, cause: 'unavailable' };
      }
      setValues(read(store));
      return result;
    },
    [store, unlock],
  );

  const confirm = useCallback(
    async (reason: string): Promise<UnlockResult> => {
      if (!read(store).unlockBeforeChanges || support?.supported !== true) return { ok: true };
      return unlock(reason);
    },
    [store, support, unlock],
  );

  return { values, support, change, confirm };
}
