/** A fingerprint in groups of four, as a person compares it with the one on the Mac. */
export function inGroupsOfFour(text: string): string {
  return (text.replace(/\s+/g, '').match(/.{1,4}/g) ?? []).join(' ');
}

/** A day in the phone's own way of writing dates: "26 September 2026". */
export function dayOf(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'long',
    year: 'numeric',
  });
}
