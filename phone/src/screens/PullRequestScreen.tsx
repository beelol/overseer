import { useCallback, useRef, useState } from 'react';
import { ScrollView, View } from 'react-native';

import { useCapabilities } from '@/platform';
import { useSessionValue } from '@/session';
import { Button, Empty, makeStyles, Row, Screen, Section, Txt, WatchOnlyLine } from '@/ui';

import { Field } from './review/Field';
import { Notice } from './review/Notice';
import { copy, openInBrowser } from './review/outside';
import type { PullPlan } from './review/plans';
import { usePullRequest } from './review/pull';
import { useReviewParams, useRunPlace } from './review/run';
import { WORDS } from './review/words';

const useStyles = makeStyles((theme) => ({
  body: { paddingBottom: theme.space[8] },
  fields: { paddingHorizontal: theme.space[4], paddingTop: theme.space[5], gap: theme.space[4] },
  footer: { padding: theme.space[3], gap: theme.space[2] },
  buttons: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.space[2] },
  grow: { flexGrow: 1 },
  said: { paddingHorizontal: theme.space[4], paddingTop: theme.space[5], gap: theme.space[1] },
}));

/** As many commits as the plan names. */
const NAMED = 20;

/**
 * Pull request (phone/docs/app-spec.md): the Mac's plan, a title and a description, Open.
 * Once it is open: its address, to open in the browser or to copy.
 *
 * The fields are `pr.field.title` and `pr.field.body`: `pr.title` is the screen's own heading.
 */
export function PullRequestScreen() {
  const styles = useStyles();
  const { haptics } = useCapabilities();
  const { run: runId } = useReviewParams();
  const { run, root, known } = useRunPlace(runId);
  const watching = useSessionValue((s) => s.scope === 'watch');
  const online = useSessionValue((s) => s.connection === 'online');
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [copied, setCopied] = useState(false);
  const typed = useRef(false);

  // The title is the plan's until the owner writes their own.
  const onPlan = useCallback((plan: PullPlan) => {
    if (plan.ok && !typed.current) setTitle(plan.title);
  }, []);
  const pull = usePullRequest(root?.workspace_id, onPlan);
  const { plan, opened } = pull;

  const writeTitle = useCallback((value: string) => {
    typed.current = true;
    setTitle(value);
  }, []);

  const open = useCallback(() => void pull.open(title, body), [pull, title, body]);
  const address = opened?.url ?? '';
  const browse = useCallback(() => void openInBrowser(address), [address]);
  const take = useCallback(async () => {
    if (await copy(address)) {
      haptics.play('confirm');
      setCopied(true);
    }
  }, [address, haptics]);

  const gone = known && !run;
  const empty = gone ? WORDS.unknownAgent : !plan ? (pull.loading ? WORDS.merge.working : (pull.error ?? WORDS.whenReached)) : '';

  return (
    <Screen
      id="pr"
      title={WORDS.pr.title}
      {...(run ? { subtitle: run.title } : {})}
      footer={
        opened ? (
          <View style={styles.footer}>
            <View style={styles.buttons}>
              <Button testID="pr.copy" label={copied ? WORDS.pr.copied : WORDS.pr.copy} icon={copied ? 'check' : 'copy'} haptic="selection" onPress={() => void take()} />
              <View style={styles.grow}>
                <Button testID="pr.browser" label={WORDS.pr.browser} kind="primary" icon="link-external" wide haptic="selection" onPress={browse} />
              </View>
            </View>
          </View>
        ) : watching ? (
          <WatchOnlyLine />
        ) : plan?.ok ? (
          <View style={styles.footer}>
            <Txt testID="pr.next" kind="small" tone="muted">
              {WORDS.pr.whatHappens(plan.branch, plan.remote, plan.repo)}
            </Txt>
            <Button
              testID="pr.open"
              label={pull.opening ? (online ? WORDS.pr.opening : WORDS.queued) : WORDS.pr.open}
              accessibilityLabel={`${WORDS.pr.open}. ${WORDS.pr.whatHappens(plan.branch, plan.remote, plan.repo)}`}
              kind="primary"
              icon="git-pull-request"
              wide
              haptic="confirm"
              disabled={pull.opening || title.trim().length === 0}
              onPress={open}
            />
          </View>
        ) : undefined
      }
    >
      {pull.error && plan ? <Notice testID="pr.error" text={pull.error} tone="red" /> : null}
      {empty ? (
        <Empty testID="pr.empty" text={empty} />
      ) : plan && !plan.ok ? (
        <Empty testID="pr.unavailable" icon="circle-slash" text={plan.reason || WORDS.pr.unavailable} />
      ) : plan?.ok ? (
        <ScrollView contentContainerStyle={styles.body} keyboardShouldPersistTaps="handled">
          {opened ? (
            <>
              <View style={styles.said}>
                <Txt testID="pr.opened" kind="strong" accessibilityRole="header">
                  {opened.reused ? WORDS.pr.wasOpen(opened.number) : WORDS.pr.isOpen(opened.number)}
                </Txt>
                <Txt kind="label" tone="muted">
                  {WORDS.pr.nothingMerged}
                </Txt>
              </View>
              <Section title={WORDS.pr.address}>
                <Row testID="pr.url" label={opened.url} tone="link" icon="link-external" onPress={browse} />
              </Section>
            </>
          ) : null}

          <Section title={WORDS.pr.plan} {...(plan.uncommitted.length > 0 && !opened ? { note: WORDS.pr.commitFirst(plan.uncommitted.length, plan.branch) } : {})}>
            <Row testID="pr.plan.branch" label={WORDS.pr.branch} value={plan.branch} icon="git-branch" />
            <Row testID="pr.plan.base" label={WORDS.pr.base} value={plan.target} icon="git-merge" divided />
            <Row testID="pr.plan.remote" label={WORDS.pr.remote} value={plan.remote} detail={plan.repo} icon="repo" divided />
            <Row testID="pr.plan.commits" label={WORDS.pr.commits} value={plan.commits.length > 0 ? String(plan.commits.length) : WORDS.pr.noCommits} icon="git-commit" divided />
            {plan.commits.slice(0, NAMED).map((subject, index) => (
              <Row key={`${index}:${subject}`} testID={`pr.commit.${index}`} label={subject} divided />
            ))}
            {watching && !opened ? <Row testID="pr.plan.title" label={WORDS.pr.titleField} detail={plan.title} divided /> : null}
          </Section>

          {!opened && !watching ? (
            <View style={styles.fields}>
              <Field testID="pr.field.title" label={WORDS.pr.titleField} titled value={title} onChangeText={writeTitle} />
              <Field testID="pr.field.body" label={WORDS.pr.bodyField} titled multiline value={body} onChangeText={setBody} hint={WORDS.pr.bodyHint} />
            </View>
          ) : null}
        </ScrollView>
      ) : null}
    </Screen>
  );
}
