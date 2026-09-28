import { useCapabilities, useLive } from '@/platform';

import { themes, type Theme } from './theme';

/**
 * The theme for the phone's current appearance. It follows the system setting and changes at
 * once when the setting changes, with the app open: the component that calls it draws again.
 */
export function useTheme(): Theme {
  const { appearance } = useCapabilities();
  return themes[useLive(appearance)];
}
