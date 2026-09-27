import { AccessibilityInfo } from 'react-native';

import { SUPPORTED, defineCapability } from '../capability';
import type { ReduceMotionCapability } from '../capabilities/reduceMotion';
import { createLive } from '../live';

/** Reduce Motion on iOS, Remove animations on Android, from React Native's AccessibilityInfo. */
export function createReduceMotion(): ReduceMotionCapability {
  const reduced = createLive(false);
  async function refresh(): Promise<boolean> {
    reduced.set(await AccessibilityInfo.isReduceMotionEnabled());
    return reduced.get();
  }
  AccessibilityInfo.addEventListener('reduceMotionChanged', (enabled) => reduced.set(enabled));
  refresh().catch(() => undefined);
  return defineCapability<ReduceMotionCapability>('reduceMotion', async () => SUPPORTED, {
    get: reduced.get,
    subscribe: reduced.subscribe,
    refresh,
  });
}
