/**
 * What leaves the app: an address opened in the browser, a text put on the clipboard.
 *
 * Both packages are in the native build and the platform layer has no capability for either
 * yet. They are used from this one file so that they can move behind a capability (reported).
 */
import * as Clipboard from 'expo-clipboard';
import * as Linking from 'expo-linking';

import { markdown } from '@/model';

/** Opens a web address in the browser. Anything that is not `http` or `https` is not opened. */
export async function openInBrowser(address: string): Promise<boolean> {
  const safe = markdown.safeHref(address);
  if (safe === null || !markdown.opensExternally(safe)) return false;
  try {
    await Linking.openURL(safe);
    return true;
  } catch {
    return false;
  }
}

export async function copy(words: string): Promise<boolean> {
  try {
    await Clipboard.setStringAsync(words);
    return true;
  } catch {
    return false;
  }
}
