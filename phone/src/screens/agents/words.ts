import { text } from '@/model';

/**
 * The words of the agents list that the brief gives (phone/docs/app-spec.md) and the model does
 * not hold yet. Everything else the list says comes from `text.TEXT` and `text.PHONE_ONLY`.
 */
export const WORDS = {
  newAgent: 'New agent',
  accounts: 'Accounts',
  settings: 'Settings',
  menu: 'Menu',
  stopAll: 'Stop all agents',
  pin: 'Pin',
  unpin: 'Unpin',
  /** A run whose stop was asked for and has not stopped yet, in the list's own lower case. */
  stopping: 'stopping',
  /** How old what is shown is, while the Mac has not confirmed it. */
  asOf: (ago: string): string => `as of ${ago}`,
  stopQuestion: (agents: number): string => `Stop ${agents} agent${agents === 1 ? '' : 's'}?`,
  stopDetail: 'Each one stops where it is. What it changed stays.',
  notDone: (what: string, why: string): string => `${what}: ${text.PHONE_ONLY.notSent.toLowerCase()}. ${why}`,
} as const;
