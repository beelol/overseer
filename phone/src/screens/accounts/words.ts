/** The words of Accounts, as the brief gives them (phone/docs/app-spec.md). */
export const ACCOUNTS = {
  title: 'Accounts',
  empty: 'No accounts yet.',
  signedIn: 'Signed in',
  signedOut: 'Signed out',
  checking: 'Checking…',
  notChecked: 'Not checked',
  notInstalled: 'Not installed on the Mac',
  apiKey: 'This account uses an API key. Overseer needs an account sign-in.',
  signIn: 'Sign in',
  onTheMac: 'Sign in on the Mac.',
  used: (percent: number): string => `${percent}% used`,
  resets: (when: string): string => `resets ${when}`,
  limit: 'Usage limit reached',
  notReported: 'Usage not reported',
  sheet: {
    asking: 'Getting a code from the Mac…',
    enter: 'Enter this code on the sign-in page.',
    open: 'Open the sign-in page',
    copy: 'Copy the code',
    copied: 'Copied',
    closes: 'This closes when you have signed in.',
    failed: 'The sign-in did not start.',
    again: 'Try again',
    old: 'This code is too old.',
    newCode: 'Get a new code',
    close: 'Close',
    code: (code: string): string => `Code ${code.split('').join(' ')}`,
  },
} as const;

/** The providers by the short names VS Code lists them under (extension/src/views.js). */
export const PROVIDER_NAME: Readonly<Record<string, string>> = {
  openai: 'ChatGPT',
  anthropic: 'Claude',
  local: 'OpenCode',
};
