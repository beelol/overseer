import { type Listener, type Support, unsupported } from '../capability';
import type {
  DiscoveredGateway,
  DiscoveryCapability,
  DiscoveryConfig,
} from '../capabilities/discovery';
import type { KeyValueCapability } from '../capabilities/keyValue';
import type { LaunchCapability } from '../capabilities/launch';
import { createDiscovery } from '../discoveryCore';
import { createFakeSupport, type FakeSupport } from './support';

export const FAKE_DISCOVERY_CONFIG: DiscoveryConfig = Object.freeze({
  serviceType: '_overseer._tcp',
  defaultPort: 47810,
});

export interface FakeDiscovery {
  readonly capability: DiscoveryCapability;
  /** Browsing support. Unsupported at first on a simulator, as on the real ones. */
  readonly support: FakeSupport;
  /** Pretends the network now shows these gateways; every browser hears of it. */
  announce(gateways: readonly DiscoveredGateway[]): void;
  /** How many browsers are listening. */
  browsers(): number;
}

interface FakeDiscoveryParts {
  readonly launch: LaunchCapability;
  readonly keyValue: KeyValueCapability;
  readonly config?: DiscoveryConfig;
  readonly support?: Support;
}

/**
 * The real discovery logic (manual addresses, the platform's addresses) over the fake stores,
 * with a browser the test drives by hand.
 */
export function createFakeDiscovery(parts: FakeDiscoveryParts): FakeDiscovery {
  const support = createFakeSupport(
    'discovery',
    parts.support ??
      (parts.launch.info().isSimulator
        ? unsupported('The fake simulator has no local network to browse.')
        : undefined),
  );
  const listeners = new Set<Listener<readonly DiscoveredGateway[]>>();
  let visible: readonly DiscoveredGateway[] = [];
  const capability = createDiscovery({
    config: parts.config ?? FAKE_DISCOVERY_CONFIG,
    launch: parts.launch,
    keyValue: parts.keyValue,
    browser: {
      support: support.get,
      browse(_config, listener) {
        const entry: Listener<readonly DiscoveredGateway[]> = (found) => listener(found);
        listeners.add(entry);
        if (visible.length > 0) entry(visible);
        return () => {
          listeners.delete(entry);
        };
      },
    },
  });
  return {
    capability,
    support,
    announce(gateways) {
      visible = Object.freeze([...gateways]);
      for (const listener of [...listeners]) listener(visible);
    },
    browsers: () => listeners.size,
  };
}
