import { useRouter } from 'expo-router';
import { useCallback, useState } from 'react';
import { ScrollView, View } from 'react-native';

import { review, text } from '@/model';
import { routes } from '@/routes';
import { useSessionValue } from '@/session';
import { Button, Confirm, Empty, makeStyles, Row, Screen, Section, Txt, WatchOnlyLine } from '@/ui';

import { useActivity, useRefreshOn } from './review/activity';
import { keepChosen, keyOf, useReviewStore } from './review/comparison';
import { useMergeBack, type MergeAction } from './review/merge';
import { Notice } from './review/Notice';
import type { MergeReady } from './review/plans';
import { useReviewParams, useRunPlace } from './review/run';
import { WORDS } from './review/words';
import { UNLOCK_FAILED, useUnlockBeforeChanges } from './settings/safety';

const useStyles = makeStyles((theme) => ({
  body: { paddingBottom: theme.space[8] },
  footer: { padding: theme.space[3], gap: theme.space[2] },
  buttons: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.space[2] },
  grow: { flexGrow: 1 },
}));

/** As many files as VS Code names before it asks. */
const NAMED = 20;

/** What the main button will do, said before it does it. */
function next(plan: MergeReady): { readonly action: MergeAction; readonly label: string; readonly says: string } | null {
  switch (plan.state) {
    case 'idle':
      return { action: 'prepare', label: WORDS.merge.prepare, says: WORDS.merge.mergeTarget(plan.target, plan.branch) };
    case 'resolving':
    case 'resolved':
      return { action: 'resume', label: WORDS.merge.resume, says: WORDS.merge.finishResolving };
    case 'ready':
      return plan.canComplete && plan.blockers.length === 0 ? { action: 'complete', label: WORDS.merge.finish, says: WORDS.merge.complete(plan.branch, plan.target, text.basename(plan.repo)) } : null;
  }
}

/**
 * Merge back (phone/docs/app-spec.md): the Mac's plan, the conflicts, and the steps as the Mac
 * has them. Each step says what it will do; what cannot be taken back asks once.
 */
export function MergeScreen() {
  const styles = useStyles();
  const router = useRouter();
  const kept = useReviewStore();
  const { run: runId } = useReviewParams();
  const { run, root, known } = useRunPlace(runId);
  const rootId = root?.id ?? runId;
  const merge = useMergeBack(rootId, root?.workspace_id);
  const activity = useActivity(rootId);
  const watching = useSessionValue((s) => s.scope === 'watch');
  const [asking, setAsking] = useState<'complete' | 'abort' | null>(null);

  // The agent resolved the conflicts, or a step was taken on the Mac.
  useRefreshOn(`${activity.ends}:${activity.merges}:${activity.status ?? ''}`, activity.ready, merge.reload);

  const { plan, landing, done, busy } = merge;
  const step = plan?.ok ? next(plan) : null;
  const merging = plan?.ok === true && (plan.state === 'resolving' || plan.state === 'resolved');

  const openFile = useCallback((path: string, comparison?: string) => router.push(routes.file(rootId, path, comparison ? { comparison } : {})), [router, rootId]);
  const target = plan?.ok ? plan.target : null;
  const reviewLanding = useCallback(() => {
    if (!target) return;
    // As VS Code does before it asks: the review shows what lands.
    keepChosen(kept, rootId, { mode: 'branch_merge_base', branch: target });
    router.push(routes.changes(rootId));
  }, [kept, rootId, target, router]);

  // Completing and aborting cannot be undone: the device's unlock first, when the owner asked for it.
  const unlockFirst = useUnlockBeforeChanges();
  const [refused, setRefused] = useState(false);
  const finish = useCallback(
    async (action: MergeAction) => {
      setRefused(false);
      const unlocked = await unlockFirst(action === 'abort' ? WORDS.merge.confirmAbort : WORDS.merge.confirmComplete);
      if (!unlocked.ok) {
        if (unlocked.cause !== 'cancelled') setRefused(true);
        return;
      }
      await merge.run(action);
    },
    [merge, unlockFirst],
  );

  const press = useCallback(
    (action: MergeAction) => {
      if (action === 'complete' || action === 'abort') setAsking(action);
      else void merge.run(action);
    },
    [merge],
  );

  const gone = known && !run;
  const empty = gone ? WORDS.unknownAgent : !plan ? (merge.loading ? WORDS.merge.working : (merge.outcome?.text ?? WORDS.whenReached)) : '';
  const landingKey = target ? keyOf({ mode: 'branch_merge_base', branch: target }) : undefined;

  return (
    <Screen
      id="merge"
      title={WORDS.merge.title}
      {...(run ? { subtitle: run.title } : {})}
      footer={
        watching ? (
          <WatchOnlyLine />
        ) : plan?.ok && !done && (step || merging) ? (
          <View style={styles.footer}>
            {step ? (
              <Txt testID="merge.next" kind="small" tone="muted">
                {step.says}
              </Txt>
            ) : null}
            <View style={styles.buttons}>
              {merging ? <Button testID="merge.abort" label={WORDS.merge.abort} kind="danger" haptic="warning" disabled={busy !== null} onPress={() => press('abort')} /> : null}
              {step ? (
                <View style={styles.grow}>
                  <Button
                    testID={`merge.${step.action === 'resume' ? 'continue' : step.action}`}
                    label={busy === step.action ? WORDS.merge.working : step.label}
                    kind="primary"
                    wide
                    haptic="confirm"
                    disabled={busy !== null}
                    onPress={() => press(step.action)}
                  />
                </View>
              ) : null}
            </View>
          </View>
        ) : undefined
      }
    >
      {refused ? <Notice testID="merge.unlock" text={UNLOCK_FAILED} tone="red" /> : null}
      {merge.outcome && plan ? <Notice testID="merge.outcome" text={merge.outcome.text} tone={merge.outcome.tone} /> : null}
      {empty ? (
        <Empty testID="merge.empty" text={empty} />
      ) : done ? (
        <Empty testID="merge.done" icon="check" text={WORDS.merge.merged(done.branch, done.target, done.commit)} />
      ) : plan && !plan.ok ? (
        <Empty testID="merge.unavailable" icon="circle-slash" text={WORDS.merge.unavailable(plan.reason)} />
      ) : plan?.ok ? (
        <ScrollView contentContainerStyle={styles.body}>
          <Section title={WORDS.merge.plan}>
            <Row testID="merge.plan.merge" label={WORDS.merge.merge} value={WORDS.merge.into(plan.branch, plan.target)} icon="git-merge" />
            <Row testID="merge.plan.repository" label={WORDS.merge.repository} value={text.basename(plan.repo)} icon="repo" divided />
            <Row testID="merge.plan.state" label={WORDS.merge.state} value={WORDS.merge.states[plan.state] ?? plan.state} icon="info" divided />
          </Section>

          {plan.state === 'idle' ? (
            <Section title={WORDS.merge.steps}>
              <Row testID="merge.step.1" label={plan.uncommitted.length > 0 ? WORDS.merge.commitFirst(plan.uncommitted.length, plan.branch) : WORDS.merge.nothingToCommit} {...(plan.uncommitted.length > 0 ? { detail: named(plan.uncommitted) } : {})} />
              <Row testID="merge.step.2" label={WORDS.merge.mergeTarget(plan.target, plan.branch)} divided />
              <Row testID="merge.step.3" label={WORDS.merge.thenReview(plan.target)} divided />
            </Section>
          ) : null}

          {plan.conflicts.length > 0 ? (
            <Section title={`${WORDS.merge.conflicts} · ${text.TEXT.chat.files(plan.conflicts.length)}`}>
              {plan.conflicts.map((path, index) => (
                <Row key={path} testID={`merge.conflict.${path}`} label={text.basename(path)} detail={path} icon="warning" tone="red" divided={index > 0} onPress={() => openFile(path)} />
              ))}
            </Section>
          ) : null}

          {plan.blockers.length > 0 ? (
            <Section title={WORDS.merge.blocked}>
              {plan.blockers.map((why, index) => (
                <Row key={why} testID={`merge.blocker.${index}`} label={why} icon="circle-slash" divided={index > 0} />
              ))}
            </Section>
          ) : null}

          {plan.state === 'ready' ? (
            <Section title={WORDS.merge.lands} {...(landing && landing.changes.length > NAMED ? { note: `… ${text.TEXT.chat.files(landing.changes.length - NAMED)}` } : {})}>
              <Row
                testID="merge.lands"
                label={landing ? text.TEXT.chat.files(landing.changes.length) : WORDS.merge.landsNothing}
                detail={WORDS.merge.into(plan.branch, plan.target)}
                icon="git-compare"
                {...(landing ? { onPress: reviewLanding } : {})}
              />
              {(landing?.changes ?? []).slice(0, NAMED).map((change) => (
                <Row key={change.path} testID={`merge.lands.${change.path}`} label={text.basename(change.path)} detail={change.path} value={review.statusLetter(change.status)} divided onPress={() => openFile(change.path, landingKey)} />
              ))}
            </Section>
          ) : null}
        </ScrollView>
      ) : null}

      <Confirm
        testID="merge.ask"
        open={asking !== null && plan?.ok === true}
        onClose={() => setAsking(null)}
        question={plan?.ok ? (asking === 'abort' ? WORDS.merge.askAbort : WORDS.merge.askComplete(plan.branch, plan.target, text.basename(plan.repo))) : ''}
        detail={asking === 'abort' ? WORDS.merge.askAbortDetail : WORDS.merge.askCompleteDetail(landing?.changes.length ?? 0, plan?.ok ? plan.target : '')}
        confirm={asking === 'abort' ? WORDS.merge.confirmAbort : WORDS.merge.confirmComplete}
        danger={asking === 'abort'}
        onConfirm={() => {
          if (asking) void finish(asking);
        }}
      />
    </Screen>
  );
}

/** The first few names, and how many more. */
function named(paths: readonly string[]): string {
  const few = paths.slice(0, 5).map((path) => text.basename(path));
  return few.join(', ') + (paths.length > few.length ? ', …' : '');
}
