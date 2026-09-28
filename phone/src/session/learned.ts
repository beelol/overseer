/**
 * The storage namespaces the screens fill with what they learn from the Mac: which agents were
 * opened and pinned, the repositories and harnesses it offered, the file being reviewed, drafts
 * and held messages. They are emptied when the Mac is forgotten or revokes the phone (AC-119,
 * AC-130), so a screen that keeps anything of the Mac under a new namespace adds it here. The
 * test in `__tests__/learned.test.ts` finds every namespace a screen uses and checks it is named
 * here or in `PHONE_OWN_SCOPES`.
 */
export const LEARNED_SCOPES: readonly string[] = ['agents', 'new', 'review', 'drafts', 'held'];

/** Namespaces that hold the phone's own settings and measurements, and nothing of the Mac. */
export const PHONE_OWN_SCOPES: readonly string[] = ['settings', 'perf', 'test'];
