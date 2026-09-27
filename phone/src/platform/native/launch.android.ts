import type { LaunchCapability } from '../capabilities/launch';
import { createLaunchFor } from './launchShared';

/** The Android emulator runs behind its own router; 10.0.2.2 is its alias for the host's loopback. */
export function createLaunch(): LaunchCapability {
  return createLaunchFor({ platform: 'android', simulatorHostAddress: '10.0.2.2' });
}
