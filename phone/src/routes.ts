import type { Href } from 'expo-router';

/** The app's routes, as the brief names them (phone/docs/app-spec.md). */
export const routes = {
  pair: '/pair' as Href,
  agents: '/agents' as Href,
  agent: (run: string) => `/agent/${encodeURIComponent(run)}` as Href,
  changes: (run: string) => `/agent/${encodeURIComponent(run)}/changes` as Href,
  file: (run: string, path: string, options: { comparison?: string; hunk?: string } = {}) =>
    ({ pathname: `/agent/${encodeURIComponent(run)}/file`, params: { path, ...options } }) as unknown as Href,
  merge: (run: string) => `/agent/${encodeURIComponent(run)}/merge` as Href,
  pr: (run: string) => `/agent/${encodeURIComponent(run)}/pr` as Href,
  newAgent: '/new' as Href,
  accounts: '/accounts' as Href,
  settings: '/settings' as Href,
} as const;
