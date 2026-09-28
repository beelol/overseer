/**
 * The sentences only this screen has. What VS Code also says comes from the model's `text.TEXT`;
 * these are the brief's words (phone/docs/app-spec.md) and the few the phone adds to them.
 */
export const WORDS = {
  changes: 'Changes',
  changesWith: (files: string): string => `Changes, ${files}`,
  more: 'More',
  mergeBack: 'Merge back',
  pullRequest: 'Pull request',
  cleanUp: 'Clean up',
  cleanUpQuestion: (branch: string): string => (branch ? `Remove the worktree ${branch}?` : 'Remove this worktree?'),
  cleanUpLoses: (n: number, names: string): string => `${n} uncommitted file${n === 1 ? '' : 's'} will be lost: ${names}`,
  cleanUpLosesNothing: 'Nothing uncommitted will be lost.',
  cleanUpNotNow: 'This worktree cannot be removed now',
  cleanUpUnknown: 'Could not ask the Mac what would be removed.',
  close: 'Close',
  newMessages: 'New messages',
  loading: 'Loading the conversation',
  notLoaded: 'Could not load the conversation.',
  tryAgain: 'Try again',
  remove: 'Remove',
  olderGone: 'Older messages are no longer kept on the Mac.',
  notOnTheMac: 'This agent is not on the Mac any more.',
  noAgent: 'No agent was chosen.',
  message: 'Message to this agent',
  attach: 'Attach a photo or an image',
  takePhoto: 'Take a photo',
  takePhotoDetail: 'The phone asks for the camera the first time.',
  chooseImage: 'Choose an image',
  removeImage: (name: string): string => `Remove ${name}`,
  image: 'image',
  imageTooLarge: 'This image is too large to send.',
  imageNoRoom: 'There is no room for another image in this message.',
  imageFailed: 'The image could not be read.',
  cameraRefused: 'The camera is not allowed. Allow it in the phone’s settings.',
  tooLong: 'This message is too long to send.',
  model: 'Model',
  effort: 'Effort',
  effortOf: (effort: string): string => `${effort} effort`,
  permissions: 'Permissions',
  standard: 'Default',
  chosen: 'chosen',
  copyCode: 'Copy code',
  copied: 'Copied',
  showAll: (lines: number): string => `Show all ${lines} lines`,
  showWhole: 'Show all of it',
  done: 'Done',
  notDone: 'Not done',
  why: 'Why, for the agent (optional)',
  whyLabel: 'What to tell the agent',
  onTheMac: 'on the Mac',
  from: (who: string): string => `from ${who}`,
  allowed: 'Allowed',
  denied: 'Denied',
  open: 'open',
  closed: 'closed',
  details: 'Details',
} as const;

/** "on the Mac", "from Bilal's iPhone", "by someone": who answered, as the card says it. */
export function byWhom(by: string | null | undefined): string {
  const who = String(by ?? '').trim();
  if (!who) return '';
  if (/^the mac$/i.test(who) || /^mac$/i.test(who)) return WORDS.onTheMac;
  if (who.startsWith('phone:')) return WORDS.from(who.slice('phone:'.length).trim() || 'a phone');
  return `by ${who}`;
}

/** "Allowed on the Mac", "Denied from Phone", or the bare word while nobody is named. */
export function answered(allow: boolean, by: string | null | undefined): string {
  return [allow ? WORDS.allowed : WORDS.denied, byWhom(by)].filter(Boolean).join(' ');
}
