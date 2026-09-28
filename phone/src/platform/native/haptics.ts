import { SUPPORTED, defineCapability, unsupported } from '../capability';
import type { HapticsCapability } from '../capabilities/haptics';
import type { LaunchCapability } from '../capabilities/launch';
import { playMoment } from './hapticsMoments';

/** One set of moments, played by each platform in its own style, through expo-haptics. */
export function createHaptics(launch: LaunchCapability): HapticsCapability {
  const { isSimulator } = launch.info();
  return defineCapability<HapticsCapability>(
    'haptics',
    async () => (isSimulator ? unsupported('Simulators have nothing that can buzz.') : SUPPORTED),
    {
      play(moment) {
        if (isSimulator) return;
        // A failed buzz is never worth an error: the moment has passed.
        playMoment(moment).catch(() => undefined);
      },
    },
  );
}
