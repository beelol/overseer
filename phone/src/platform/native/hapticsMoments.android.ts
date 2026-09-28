import * as Haptics from 'expo-haptics';

import type { HapticMoment } from '../capabilities/haptics';

/**
 * Android plays moments with the system's own view haptics, which follow the owner's touch
 * feedback setting and need no vibration permission.
 */
export function playMoment(moment: HapticMoment): Promise<void> {
  switch (moment) {
    case 'selection':
      return Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Segment_Tick);
    case 'confirm':
      return Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Confirm);
    case 'reject':
      return Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Reject);
    case 'warning':
      return Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Long_Press);
    case 'impact':
      return Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Context_Click);
  }
}
