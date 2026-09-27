import { useCallback, useState } from 'react';
import { ScrollView, View } from 'react-native';

import { text } from '@/model';
import { useSession, useSessionValue } from '@/session';
import { Confirm, makeStyles, Row, Screen, Section, useMinute } from '@/ui';

import { dayOf, inGroupsOfFour } from './settings/format';
import { MacAddress } from './settings/MacAddress';
import { Note } from './settings/Note';
import { NotificationsSection } from './settings/NotificationsSection';
import { useSafety } from './settings/safety';
import { SafetySection } from './settings/SafetySection';
import { SETTINGS } from './settings/words';

const useStyles = makeStyles((theme) => ({
  content: { paddingBottom: theme.space[8] },
}));

/** Settings: notifications, this phone, the Mac, appearance, safety, and forgetting the Mac. */
export function SettingsScreen() {
  const styles = useStyles();
  const session = useSession();
  const gateway = useSessionValue((s) => s.gateway);
  const scope = useSessionValue((s) => s.scope);
  const lastContact = useSessionValue((s) => s.lastContact);
  const now = useMinute();
  const safety = useSafety();
  const [asking, setAsking] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

  const watch = (scope ?? gateway?.scope) === 'watch';

  const forget = useCallback(() => {
    setFailed(null);
    (async () => {
      const unlocked = await safety.confirm(SETTINGS.forget.action);
      if (!unlocked.ok) {
        if (unlocked.cause !== 'cancelled') setFailed(SETTINGS.safety.failed);
        return;
      }
      // The app's root sees that the pairing is gone and opens pairing.
      await session.forget();
    })().catch(() => setFailed(SETTINGS.forget.failed));
  }, [safety, session]);

  return (
    <Screen id="settings" title={SETTINGS.title}>
      <ScrollView contentContainerStyle={styles.content}>
        <NotificationsSection />

        <Section title={SETTINGS.phone.title}>
          <Row
            testID="settings.phone.name"
            label={SETTINGS.phone.name}
            value={gateway?.deviceName ?? ''}
          />
          <Row
            testID="settings.phone.scope"
            label={SETTINGS.phone.access}
            value={watch ? SETTINGS.phone.watch : SETTINGS.phone.full}
            {...(watch ? { detail: SETTINGS.phone.changeOnTheMac } : {})}
            divided
          />
          <Row
            testID="settings.phone.paired"
            label={SETTINGS.phone.paired}
            value={gateway ? dayOf(gateway.pairedAt) : ''}
            divided
          />
        </Section>

        <Section title={SETTINGS.mac.title}>
          <Row
            testID="settings.mac.name"
            label={SETTINGS.mac.name}
            value={gateway?.gatewayName ?? ''}
          />
          <Row
            testID="settings.mac.contact"
            label={SETTINGS.mac.contact}
            value={text.agoInWords(lastContact, now) || SETTINGS.mac.never}
            divided
          />
          <Row
            testID="settings.mac.fingerprint"
            label={SETTINGS.mac.fingerprint}
            value={inGroupsOfFour(gateway?.gatewayFingerprint ?? '')}
            divided
          />
        </Section>

        <MacAddress />

        <Section title={SETTINGS.appearance.title}>
          <Row testID="settings.appearance" label={SETTINGS.appearance.follows} />
        </Section>

        <SafetySection safety={safety} />

        <Section>
          <Row
            testID="settings.forget"
            label={SETTINGS.forget.action}
            tone="red"
            right={<View />}
            onPress={() => setAsking(true)}
          />
        </Section>
        {failed ? <Note testID="settings.forget.failed" text={failed} tone="red" alert /> : null}
      </ScrollView>

      <Confirm
        testID="settings.forget.ask"
        open={asking}
        onClose={() => setAsking(false)}
        question={SETTINGS.forget.question}
        detail={SETTINGS.forget.detail}
        confirm={SETTINGS.forget.action}
        onConfirm={forget}
      />
    </Screen>
  );
}
