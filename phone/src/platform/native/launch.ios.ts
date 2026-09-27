import type { LaunchCapability } from '../capabilities/launch';
import { createLaunchFor } from './launchShared';

/** The iOS simulator shares the Mac's network, so the Mac's loopback is its own. */
export function createLaunch(): LaunchCapability {
  return createLaunchFor({ platform: 'ios', simulatorHostAddress: '127.0.0.1' });
}
