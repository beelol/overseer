import { defineCapability, type Support, unsupported } from '../capability';
import type { HapticMoment, HapticsCapability } from '../capabilities/haptics';
import type { LaunchCapability } from '../capabilities/launch';
import { createFakeSupport, type FakeSupport } from './support';

export interface FakeHaptics {
  readonly capability: HapticsCapability;
  readonly support: FakeSupport;
  /** The moments that were played, in order. Nothing is recorded while unsupported. */
  played(): readonly HapticMoment[];
}

export function createFakeHaptics(launch: LaunchCapability, initial?: Support): FakeHaptics {
  const support = createFakeSupport(
    'haptics',
    initial ??
      (launch.info().isSimulator
        ? unsupported('The fake simulator has nothing that can buzz.')
        : undefined),
  );
  const played: HapticMoment[] = [];
  const capability = defineCapability<HapticsCapability>('haptics', support.check, {
    play(moment) {
      // Like the real one: no buzz and no error where there are no haptics.
      if (support.get().supported) played.push(moment);
    },
  });
  return { capability, support, played: () => played };
}
