import { ScrollView, View } from 'react-native';

import type { ModBinding, TurnModSnapshot } from '@/protocol';
import { Button, makeStyles, Screen, Txt } from '@/ui';

import { useMods } from './mods/useMods';
import { Notice } from './review/Notice';
import { useReviewParams, useRunPlace } from './review/run';

const useStyles = makeStyles((theme) => ({
  content: { padding: theme.space[4], gap: theme.space[4] },
  section: { gap: theme.space[2] },
  row: {
    gap: theme.space[1],
    paddingVertical: theme.space[2],
    borderBottomWidth: theme.phone.size.hairline,
    borderBottomColor: theme.colors.border,
  },
  actions: { padding: theme.space[2] },
}));
const object = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
const string = (value: unknown, fallback = ''): string =>
  typeof value === 'string' ? value : fallback;
const scalar = (value: unknown): string =>
  Array.isArray(value)
    ? value.filter((v): v is string => typeof v === 'string').join(', ')
    : typeof value === 'boolean' || typeof value === 'number'
      ? String(value)
      : string(value, 'Unknown');
const name = (entry: unknown): string => {
  const item = object(entry);
  return string(object(item.manifest).name, string(item.id, 'Unknown mod'));
};
const scopes = {
  all_agents: 'All agents (excluding Overseer)',
  repository: 'Repository',
  watchers: 'Watchers',
  agent: 'Agent',
  overseer: 'Overseer session',
};
const scope = (binding: ModBinding): string =>
  `${scopes[binding.scope.kind]}${binding.scope.run_id ? `: ${binding.scope.run_id}` : binding.scope.repo_key ? `: ${binding.scope.repo_key}` : ''}`;
const outcomes: Record<TurnModSnapshot['outcome'], string> = {
  prepared: 'Prepared; delivery is not confirmed',
  transport_accepted: 'Transport accepted',
  failed_before_effect: 'Failed before delivery',
  uncertain_after_effect: 'Delivery outcome is uncertain',
};

/** Phone inspection only. The Mac owns installation, bindings, permission and delivery truth. */
export function ModsScreen() {
  const styles = useStyles();
  const { run: runId } = useReviewParams();
  const { run } = useRunPlace(runId);
  const data = useMods(runId);
  const library = data.library;
  const applied = data.applied;
  const last = applied?.last_turn;
  const retained = Boolean(library || applied);
  const stale = retained && (!data.online || Boolean(data.libraryError || data.appliedError));
  const supported = object(applied?.support ?? library?.support);
  return (
    <Screen
      id="mods"
      title="Mods"
      {...(run ? { subtitle: run.title } : {})}
      actions={
        <Button
          testID="mods.refresh"
          label="Refresh"
          accessibilityLabel="Refresh Mods"
          kind="quiet"
          disabled={!data.online || !data.paired || !runId || data.loading}
          onPress={() => void data.refresh()}
        />
      }
    >
      {stale ? (
        <Notice
          testID="mods.stale"
          text={
            !data.online
              ? 'Offline. These are previously confirmed facts from the Mac.'
              : 'Some facts could not be refreshed. The previous answer remains visible.'
          }
        />
      ) : null}
      {data.loading ? <Notice testID="mods.loading" text="Checking Mods on the Mac…" /> : null}
      <ScrollView testID="mods.list" contentContainerStyle={styles.content}>
        <Txt testID="mods.readonly" kind="small" tone="muted">
          Inspection only. Change Mods on the Mac. Installing never enables them.
        </Txt>
        {!runId ? (
          <Txt testID="mods.empty">Choose an agent to inspect its Mods.</Txt>
        ) : !data.online && !retained ? (
          <Txt testID="mods.empty">Shown when the Mac is reached.</Txt>
        ) : null}
        <View testID="mods.installed" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Installed library
          </Txt>
          {data.libraryError ? (
            <Txt testID="mods.library.error" tone="red" accessibilityRole="alert">
              {data.libraryError}
            </Txt>
          ) : null}
          {library ? (
            library.installed.length === 0 ? (
              <Txt tone="muted">No Mods are installed.</Txt>
            ) : (
              library.installed.map((entry, index) => {
                const item = object(entry);
                return (
                  <View key={`${string(item.fingerprint)}:${index}`} style={styles.row}>
                    <Txt kind="label">
                      {name(entry)} · {string(item.version, 'Unknown version')}
                    </Txt>
                    <Txt kind="small" selectable>
                      {string(item.source)}
                    </Txt>
                    <Txt kind="mono" selectable>
                      {string(item.fingerprint, 'Fingerprint unavailable')}
                    </Txt>
                    {Array.isArray(item.files)
                      ? item.files.map((file, i) => {
                          const info = object(file);
                          return (
                            <Txt key={i} kind="small" selectable>
                              {string(info.path)} · {scalar(info.bytes)} original bytes ·{' '}
                              {string(info.sha256)}
                            </Txt>
                          );
                        })
                      : null}
                  </View>
                );
              })
            )
          ) : (
            <Txt tone="muted">Library has not been loaded.</Txt>
          )}
          {library ? (
            <Txt kind="small" tone="muted">
              Library revision {library.revision}
            </Txt>
          ) : null}
        </View>
        <View testID="mods.bindings" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Owner bindings
          </Txt>
          {library?.bindings.map((binding) => (
            <View key={binding.id} style={styles.row}>
              <Txt kind="label">
                {binding.mod_id} · {scope(binding)}
              </Txt>
              <Txt kind="small">
                {binding.enabled ? 'Enabled' : 'Explicitly disabled'}
                {binding.locked ? ' · Locked' : ''}
                {binding.required ? ' · Required' : ''}
              </Txt>
              <Txt kind="mono" selectable>
                {binding.fingerprint}
              </Txt>
              {Object.entries(binding.filters)
                .filter(([, values]) => Array.isArray(values) && values.length > 0)
                .map(([key, values]) => (
                  <Txt key={key} kind="small" selectable>
                    {key}: {scalar(values)}
                  </Txt>
                ))}
            </View>
          ))}
          {library?.bindings.length === 0 ? <Txt tone="muted">No owner bindings.</Txt> : null}
        </View>
        <View testID="mods.desired" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Desired for this agent
          </Txt>
          {data.appliedError ? (
            <Txt testID="mods.applied.error" tone="red" accessibilityRole="alert">
              {data.appliedError}
            </Txt>
          ) : null}
          {applied ? (
            <>
              <Txt testID="mods.pending" kind="label">
                {applied.pending ? 'Pending a qualified turn' : 'No pending change reported'}
              </Txt>
              <Txt kind="small" selectable>
                {applied.context.run_id} · {applied.context.role} · {applied.context.harness}
                {applied.context.model ? ` · ${applied.context.model}` : ''}
              </Txt>
              {applied.desired.decisions.map((decision) => (
                <View key={decision.binding_id} style={styles.row}>
                  <Txt kind="label">
                    {decision.mod_id} · {decision.status.replaceAll('_', ' ')}
                  </Txt>
                  <Txt>{decision.reason}</Txt>
                  <Txt kind="small">
                    {decision.delivery} · {decision.activation} · children: {decision.children}
                    {decision.required ? ' · Required' : ''}
                  </Txt>
                  <Txt kind="mono" selectable>
                    {decision.fingerprint}
                  </Txt>
                </View>
              ))}
              {applied.desired.decisions.length === 0 ? (
                <Txt tone="muted">No bindings apply to this agent.</Txt>
              ) : null}
              {applied.desired.rules_text ? (
                <Txt selectable>{applied.desired.rules_text}</Txt>
              ) : null}
              {applied.desired.style_text ? (
                <Txt selectable>{applied.desired.style_text}</Txt>
              ) : null}
            </>
          ) : (
            <Txt tone="muted">The plan for this agent has not been loaded.</Txt>
          )}
        </View>
        <View testID="mods.last" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Last recorded turn
          </Txt>
          {last ? (
            <>
              <Txt kind="label">{outcomes[last.outcome]}</Txt>
              <Txt kind="small" selectable>
                {last.turn_id} · {last.transport} · {last.delivery}
              </Txt>
              {last.outcome_detail ? <Txt>{last.outcome_detail}</Txt> : null}
              <Txt kind="small" selectable>
                Planned: {last.planned_fingerprints.join(', ') || 'None'}
              </Txt>
              <Txt kind="small" selectable>
                Applied: {last.applied_fingerprints.join(', ') || 'None'}
              </Txt>
              <Txt kind="mono" selectable>
                {last.digest}
              </Txt>
              <Txt kind="small">{last.added_bytes} original bytes</Txt>
              {last.text_redacted ? (
                <Txt kind="small" tone="muted">
                  Public text is redacted. The digest and byte count identify the private original.
                </Txt>
              ) : null}
              {last.text ? <Txt selectable>{last.text}</Txt> : null}
            </>
          ) : (
            <Txt tone="muted">No recorded turn delivery{applied ? '.' : ' loaded.'}</Txt>
          )}
        </View>
        <View testID="mods.support" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Delivery support
          </Txt>
          {Object.entries(supported).map(([key, value]) => (
            <Txt key={key} kind="small">
              {key}: {scalar(value)}
            </Txt>
          ))}
          {applied ? (
            <Txt testID="mods.notice" kind="small" tone="muted">
              {applied.notice}
            </Txt>
          ) : null}
        </View>
        <View testID="mods.planned" style={styles.section}>
          <Txt kind="strong" accessibilityRole="header">
            Available on the Mac
          </Txt>
          {library?.available_bundled.map((entry, i) => (
            <Txt key={i} kind="small">
              {name(entry)} · {string(object(entry).version)} (installation does not enable it)
            </Txt>
          ))}
          {library?.unavailable.map((entry, i) => (
            <Txt key={i} kind="small">
              {string(object(entry).id)}: {string(object(entry).reason, 'Unavailable')}
            </Txt>
          ))}
        </View>
      </ScrollView>
    </Screen>
  );
}
