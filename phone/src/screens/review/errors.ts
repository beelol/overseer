import { text } from '@/model';

/** What went wrong, in the words of the Mac, as one line. */
export function sentence(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error ?? '');
  return text.firstLine(message, 200) || 'It did not work.';
}

/** The code of a refusal (`conflict`, `not_editable`, `mac_setup`), when the Mac gave one. */
export function codeOf(error: unknown): string | null {
  if (error !== null && typeof error === 'object' && 'code' in error) {
    const code = (error as { readonly code: unknown }).code;
    return typeof code === 'string' ? code : null;
  }
  return null;
}

/** True when the Mac refused because the file is not what the hunk was shown against. */
export function changedSince(error: unknown): boolean {
  if (codeOf(error) === 'conflict') return true;
  return error instanceof Error && /changed while you were/.test(error.message);
}

/** True when nothing was asked because there is no connection: not something to say as an error. */
export function notConnected(error: unknown): boolean {
  const code = codeOf(error);
  return code === 'not_connected' || code === 'connection_lost' || code === 'unpaired';
}
