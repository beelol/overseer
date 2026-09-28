import { CapabilityUnsupportedError, defineCapability, unsupported } from '../capability';
import type { PushCapability } from '../capabilities/push';

const gap = unsupported(
  'Push on Android needs a Firebase project and comes with the relay. Until then the app shows what needs you while it is open.',
);

function refuse(): Promise<never> {
  return Promise.reject(new CapabilityUnsupportedError('push', gap.reason));
}

/** Android in this gate: no push. Every call says so instead of pretending. */
export function createPush(): PushCapability {
  return defineCapability<PushCapability>('push', async () => gap, {
    permission: refuse,
    requestPermission: refuse,
    deviceToken: refuse,
    setCategories: refuse,
    setForegroundPresentation() {
      throw new CapabilityUnsupportedError('push', gap.reason);
    },
    onReceived() {
      throw new CapabilityUnsupportedError('push', gap.reason);
    },
    onResponse() {
      throw new CapabilityUnsupportedError('push', gap.reason);
    },
    launchResponse: refuse,
  });
}
