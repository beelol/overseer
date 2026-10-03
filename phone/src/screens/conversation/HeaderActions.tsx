import { useRouter } from 'expo-router';
import { memo, useCallback, useMemo, useState } from 'react';

import { text, type agents, type review } from '@/model';
import type { GitStatus, Workspace } from '@/protocol';
import { routes } from '@/routes';
import { useSession } from '@/session';
import { Button, Confirm, IconButton, Menu, Sheet, type MenuItem } from '@/ui';

import { UNLOCK_FAILED, useUnlockBeforeChanges } from '../settings/safety';
import { WORDS } from './words';

/** How many of the files that would be lost are named; the rest is a number. */
const NAMED = 6;

export interface HeaderActionsProps {
  readonly runId: string;
  readonly header: agents.RunHeader | undefined;
  readonly taskId: string | undefined;
  readonly workspace: Pick<Workspace, 'id' | 'kind' | 'branch' | 'removed_ms'> | undefined;
  /** The changed files, or `null` when there are none or they are not known yet. */
  readonly changes: review.ChangesSummary | null;
  readonly watch: boolean;
  readonly onStop: () => void;
}

/** The paths a worktree holds that are not committed, each once. */
export function uncommitted(dirty: GitStatus | null | undefined): readonly string[] {
  if (!dirty) return [];
  return [...new Set([...dirty.staged.map((c) => c.path), ...dirty.unstaged.map((c) => c.path), ...dirty.untracked, ...dirty.conflicted])];
}

type Asking = { readonly kind: 'none' } | { readonly kind: 'confirm'; readonly lost: readonly string[] } | { readonly kind: 'no'; readonly why: string };

/**
 * The header's controls: Changes with the count of changed files, and More. More holds what
 * can be done now: Stop while the agent works; Merge back, Pull request, Clean up and Archive
 * once it has stopped. A menu has the id of the control that opens it (`agent.more.stop`).
 */
export const HeaderActions = memo(function HeaderActions({ runId, header, taskId, workspace, changes, watch, onStop }: HeaderActionsProps) {
  const router = useRouter();
  const session = useSession();
  const [more, setMore] = useState(false);
  const [asking, setAsking] = useState<Asking>({ kind: 'none' });
  const workspaceId = workspace?.id;

  const askCleanUp = useCallback(() => {
    if (!workspaceId) return;
    // The daemon lists what is not committed first; the question names it.
    session.request('workspace.cleanup_plan', { workspace_id: workspaceId }).then(
      (plan) => setAsking(plan.removable ? { kind: 'confirm', lost: uncommitted(plan.dirty) } : { kind: 'no', why: plan.reason }),
      () => setAsking({ kind: 'no', why: WORDS.cleanUpUnknown }),
    );
  }, [session, workspaceId]);

  const unlockFirst = useUnlockBeforeChanges();
  const cleanUp = useCallback(async () => {
    if (!workspaceId || asking.kind !== 'confirm') return;
    const lost = asking.lost;
    const unlocked = await unlockFirst(WORDS.cleanUp);
    if (!unlocked.ok) {
      if (unlocked.cause !== 'cancelled') setAsking({ kind: 'no', why: UNLOCK_FAILED });
      return;
    }
    session.request('workspace.cleanup', { workspace_id: workspaceId, discard_dirty: lost.length > 0 }).catch(() => undefined);
  }, [session, workspaceId, asking, unlockFirst]);

  const archived = header?.archived === true;
  const archive = useCallback(() => {
    if (!taskId) return;
    session.request('task.archive', { task_id: taskId, archived: !archived }).catch(() => undefined);
    // An archived agent leaves the list: back to it. A restored one stays open.
    if (archived) return;
    if (router.canGoBack()) router.back();
    else router.replace(routes.agents);
  }, [session, router, taskId, archived]);

  const items = useMemo(() => {
    const list: MenuItem[] = [];
    // Inspection needs only the route target, including while its header is loading.
    if (runId) list.push({ id: 'mods', label: 'Mods', icon: 'list-unordered', onPress: () => router.push(routes.mods(runId)) });
    if (!header) return list;
    const idle = !header.active && !header.child;
    const worktree = workspace?.kind === 'worktree' && !workspace.removed_ms;
    if (!watch && header.canStop) list.push({ id: 'stop', label: text.TEXT.chat.stop, icon: 'debug-stop', danger: true, onPress: onStop });
    if (idle && worktree) {
      list.push({ id: 'merge', label: WORDS.mergeBack, icon: 'git-merge', onPress: () => router.push(routes.merge(runId)) });
      list.push({ id: 'pr', label: WORDS.pullRequest, icon: 'git-pull-request', onPress: () => router.push(routes.pr(runId)) });
      if (!watch) list.push({ id: 'cleanup', label: WORDS.cleanUp, icon: 'trash', danger: true, onPress: askCleanUp });
    }
    if (!watch && idle && taskId) list.push({ id: 'archive', label: archived ? text.TEXT.chat.restore : text.TEXT.chat.archive, icon: archived ? 'discard' : 'archive', onPress: archive });
    return list;
  }, [header, workspace, watch, taskId, archived, runId, router, onStop, askCleanUp, archive]);

  const lost = asking.kind === 'confirm' ? asking.lost : [];
  const named = lost.slice(0, NAMED).join(', ') + (lost.length > NAMED ? ', …' : '');
  const done = useCallback(() => setAsking({ kind: 'none' }), []);

  return (
    <>
      <IconButton
        testID="agent.changes"
        accessibilityLabel={changes ? WORDS.changesWith(changes.text) : WORDS.changes}
        icon="diff-multiple"
        haptic="selection"
        {...(changes ? { badge: String(changes.files) } : {})}
        onPress={() => router.push(routes.changes(runId))}
      />
      {items.length > 0 ? <IconButton testID="agent.more" accessibilityLabel={WORDS.more} icon="ellipsis" haptic="selection" onPress={() => setMore(true)} /> : null}
      <Menu testID="agent.more" open={more} onClose={() => setMore(false)} items={items} />
      <Confirm
        testID="agent.more.cleanup"
        open={asking.kind === 'confirm'}
        onClose={done}
        question={WORDS.cleanUpQuestion(workspace?.branch ?? '')}
        detail={lost.length > 0 ? WORDS.cleanUpLoses(lost.length, named) : WORDS.cleanUpLosesNothing}
        confirm={WORDS.cleanUp}
        onConfirm={cleanUp}
      />
      <Sheet testID="agent.more.cleanup.no" open={asking.kind === 'no'} onClose={done} title={WORDS.cleanUpNotNow} {...(asking.kind === 'no' ? { message: asking.why } : {})}>
        <Button testID="agent.more.cleanup.no.ok" label={WORDS.close} kind="quiet" onPress={done} />
      </Sheet>
    </>
  );
});
