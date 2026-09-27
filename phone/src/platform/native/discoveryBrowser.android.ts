import { CapabilityUnsupportedError, unsupported } from '../capability';
import type { LaunchCapability } from '../capabilities/launch';
import type { DiscoveryBrowser } from '../discoveryCore';

/**
 * Network service discovery on Android. The emulator sits behind its own router and cannot see
 * the services of the local network at all; on a real phone browsing is not built yet.
 */
export function createDiscoveryBrowser(launch: LaunchCapability): DiscoveryBrowser {
  const support = unsupported(
    launch.info().isSimulator
      ? 'The Android emulator cannot see services on the local network. It reaches the Mac at 10.0.2.2; a manual address works too.'
      : 'Browsing with network service discovery is not in this build yet. Add the address of the Mac by hand.',
  );
  return {
    support: () => support,
    browse() {
      throw new CapabilityUnsupportedError('discovery', support.reason);
    },
  };
}
