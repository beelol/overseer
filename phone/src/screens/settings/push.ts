import type { Capabilities, PushPermission } from '@/platform';
import type { Session } from '@/session';

/** Why the app asks for notifications: said before the system's own question. */
export const NOTIFICATIONS_REASON = 'Overseer can tell you when an agent needs you.';
export const ALLOW_NOTIFICATIONS = 'Allow notifications';

/** True when the system delivers notifications, loudly or quietly. */
export function delivers(permission: PushPermission | null): boolean {
  return permission === 'granted' || permission === 'provisional';
}

/**
 * Tells the Mac where to send. A simulator has no address at Apple's service: the Mac reaches
 * it by its own tool. The token is handed on and never kept by a screen.
 */
async function tellTheMac(
  { push, launch }: Pick<Capabilities, 'push' | 'launch'>,
  session: Session,
): Promise<void> {
  const simulator = launch.info().isSimulator;
  const token = simulator ? 'booted' : (await push.deviceToken()).token;
  // The owner just said yes: notifications are on for this phone from now, until they turn
  // them off in Settings. The Mac sends nothing to a phone that never said yes.
  await session.setNotifications({ enabled: true });
  await session.request('device.notifications', {
    token,
    environment: simulator ? 'simulator' : 'device',
  });
}

/**
 * Asks the system, which asks the owner once. When the owner allowed it the Mac learns where to
 * send; if the Mac cannot be reached now, it learns at the next connection.
 */
export async function allowNotifications(
  capabilities: Pick<Capabilities, 'push' | 'launch'>,
  session: Session,
): Promise<PushPermission> {
  const permission = await capabilities.push.requestPermission();
  if (delivers(permission)) await tellTheMac(capabilities, session).catch(() => undefined);
  return permission;
}
