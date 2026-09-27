import * as Haptics from 'expo-haptics';

import type { HapticMoment } from '../capabilities/haptics';

/** iOS plays moments with the system's three feedback generators. */
export function playMoment(moment: HapticMoment): Promise<void> {
  switch (moment) {
    case 'selection':
      return Haptics.selectionAsync();
    case 'confirm':
      return Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    case 'reject':
      return Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
    case 'warning':
      return Haptics.notificationAsync(Haptics.NotificationFeedbackType.Warning);
    case 'impact':
      return Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
  }
}
