import { openSettings } from 'expo-linking';
import { useCallback, useEffect, useState } from 'react';
import { View } from 'react-native';

import { useCapabilities, useLive, type PushPermission } from '@/platform';
import { useSession, useSessionValue, type NotificationSwitches } from '@/session';
import { Button, makeStyles, Section, SwitchRow, Txt } from '@/ui';

import { Note } from './Note';
import { ALLOW_NOTIFICATIONS, allowNotifications, delivers, NOTIFICATIONS_REASON } from './push';
import { useSupport } from './useSupport';
import { SETTINGS } from './words';

type Kind = keyof NotificationSwitches['kinds'];
const KINDS: readonly Kind[] = ['permission', 'question', 'failure', 'finished'];

const useStyles = makeStyles((theme) => ({
  offer: {
    padding: theme.space[4],
    gap: theme.space[3],
    borderBottomWidth: theme.phone.size.hairline,
    borderBottomColor: theme.colors.border,
  },
}));

/**
 * This phone's notification switches, as the Mac holds them. A change is shown at once and
 * put back if the Mac refuses it. Off means the Mac sends nothing.
 */
export function NotificationsSection() {
  const styles = useStyles();
  const session = useSession();
  const capabilities = useCapabilities();
  const { push, appState } = capabilities;
  const switches = useSessionValue((s) => s.notifications);
  const mac = useSessionValue((s) => s.macNotifications);
  const support = useSupport(push);
  const phase = useLive(appState);
  const [permission, setPermission] = useState<PushPermission | null>(null);
  const [refused, setRefused] = useState(false);

  // Read again when the app comes back to the front: the owner may have been in the system's settings.
  useEffect(() => {
    if (!support?.supported || phase !== 'foreground') return undefined;
    let current = true;
    push
      .permission()
      .then((answer) => {
        if (current) setPermission(answer);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [support, phase, push]);

  const change = useCallback(
    (next: Parameters<typeof session.setNotifications>[0]) => {
      setRefused(false);
      session.setNotifications(next).catch(() => setRefused(true));
    },
    [session],
  );

  const allow = useCallback(() => {
    // The system asks once. After a refusal only its own settings can allow it.
    if (permission === 'denied') {
      openSettings().catch(() => undefined);
      return;
    }
    allowNotifications(capabilities, session)
      .then(setPermission)
      .catch(() => undefined);
  }, [permission, capabilities, session]);

  const offer = mac && support?.supported === true && permission !== null && !delivers(permission);

  return (
    <>
      <Section title={SETTINGS.notifications.title}>
        {offer ? (
          <View style={styles.offer}>
            <Txt testID="settings.notifications.reason" kind="label" tone="muted">
              {permission === 'denied' ? SETTINGS.notifications.notAllowed : NOTIFICATIONS_REASON}
            </Txt>
            <Button
              testID="settings.notifications.allow"
              label={ALLOW_NOTIFICATIONS}
              kind="primary"
              onPress={allow}
            />
          </View>
        ) : null}
        <SwitchRow
          testID="settings.notifications.all"
          label={SETTINGS.notifications.all}
          value={mac && switches.enabled}
          disabled={!mac}
          onChange={(enabled) => change({ enabled })}
        />
        {KINDS.map((kind) => (
          <SwitchRow
            key={kind}
            testID={`settings.notifications.${kind}`}
            label={SETTINGS.notifications[kind]}
            value={mac && switches.enabled && switches.kinds[kind]}
            disabled={!mac || !switches.enabled}
            divided
            onChange={(on) => change({ kinds: { [kind]: on } })}
          />
        ))}
        <SwitchRow
          testID="settings.notifications.text"
          label={SETTINGS.notifications.text}
          value={switches.show_text}
          disabled={!mac || !switches.enabled}
          divided
          onChange={(show_text) => change({ show_text })}
        />
      </Section>
      {!mac ? (
        <Note testID="settings.notifications.mac" text={SETTINGS.notifications.offOnTheMac} />
      ) : null}
      {mac && refused ? (
        <Note
          testID="settings.notifications.refused"
          text={SETTINGS.notifications.refused}
          tone="red"
          alert
        />
      ) : null}
      {mac && support?.supported === false ? (
        <Note testID="settings.notifications.why" text={support.reason} />
      ) : null}
    </>
  );
}
