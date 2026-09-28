import { CapabilityUnsupportedError, unsupported } from '../capability';
import type { LaunchCapability } from '../capabilities/launch';
import type { DiscoveryBrowser } from '../discoveryCore';

/**
 * Bonjour on iOS. Browsing is built with the real iPhone (AC-120), where the local network
 * permission can be granted and verified; until then it reports the gap.
 */
export function createDiscoveryBrowser(launch: LaunchCapability): DiscoveryBrowser {
  const support = unsupported(
    launch.info().isSimulator
      ? 'Browsing with Bonjour is not in this build yet. The simulator reaches the Mac at 127.0.0.1; a manual address works too.'
      : 'Browsing with Bonjour is not in this build yet. Add the address of the Mac by hand.',
  );
  return {
    support: () => support,
    browse() {
      throw new CapabilityUnsupportedError('discovery', support.reason);
    },
  };
}
