import { useLocalSearchParams } from 'expo-router';

import { store, type Run, type Workspace } from '@/model';
import { useSessionValue } from '@/session';

const one = (value: string | string[] | undefined): string | undefined => (Array.isArray(value) ? value[0] : value);

export interface ReviewParams {
  readonly run: string;
  readonly path: string | undefined;
  readonly comparison: string | undefined;
  readonly hunk: string | undefined;
}

/** What the route was opened with. */
export function useReviewParams(): ReviewParams {
  const params = useLocalSearchParams<{ run?: string | string[]; path?: string | string[]; comparison?: string | string[]; hunk?: string | string[] }>();
  return { run: one(params.run) ?? '', path: one(params.path) || undefined, comparison: one(params.comparison) || undefined, hunk: one(params.hunk) || undefined };
}

export interface RunPlace {
  readonly run: Run | undefined;
  /** The top-level run: merging back and a pull request are its business, as in VS Code. */
  readonly root: Run | undefined;
  readonly workspace: Workspace | undefined;
  /** False while the phone has no state at all yet: the run may still arrive. */
  readonly known: boolean;
}

/** A run, the top-level run it belongs to and the workspace it works in, live. */
export function useRunPlace(runId: string): RunPlace {
  const run = useSessionValue((s) => store.run(s.state, runId));
  const root = useSessionValue((s) => store.rootOf(s.state, runId) ?? store.run(s.state, runId));
  const workspace = useSessionValue((s) => store.workspace(s.state, (store.rootOf(s.state, runId) ?? store.run(s.state, runId))?.workspace_id));
  const known = useSessionValue((s) => s.stateAt !== null);
  return { run, root, workspace, known };
}
