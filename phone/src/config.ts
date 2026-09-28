import type { DiscoveryConfig } from '@/platform';

/**
 * Where the gateway is looked for. The values are the protocol's
 * (docs/rfcs/phone-remote-protocol.md: Transport, Discovery).
 */
export const GATEWAY_DISCOVERY: DiscoveryConfig = Object.freeze({
  serviceType: '_overseer._tcp',
  defaultPort: 47810,
});
