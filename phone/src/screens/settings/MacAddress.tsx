import { useCallback, useState } from 'react';
import { View } from 'react-native';

import { formatAddress, useCapabilities, useLive, type GatewayAddress } from '@/platform';
import { useSession } from '@/session';
import { Button, IconButton, makeStyles, Row, Section } from '@/ui';

import { Field } from '../pair/Field';
import { Note } from './Note';

const WORDS = {
  title: "The Mac's address",
  note: 'Overseer finds your Mac by itself. If it cannot, type the address of the Mac here. It is tried next to the ones Overseer knows.',
  label: 'Address',
  example: '192.168.1.20:47810',
  add: 'Add',
  remove: (address: string): string => `Remove ${address}`,
} as const;

const useStyles = makeStyles((theme) => ({
  form: { padding: theme.space[4], gap: theme.space[3] },
  divided: { borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
}));

/**
 * An address typed by the owner (AC-120): it always works, with no browsing of the network and
 * no pairing again, because the Mac is known by its key and not by where it is.
 */
export function MacAddress() {
  const styles = useStyles();
  const session = useSession();
  const { discovery } = useCapabilities();
  const manual = useLive(discovery.manual);
  const [typed, setTyped] = useState('');
  const [refused, setRefused] = useState<string | null>(null);

  const add = useCallback(() => {
    const result = discovery.addManual(typed);
    if (!result.ok) {
      setRefused(result.reason);
      return;
    }
    setRefused(null);
    setTyped('');
    session.wake();
  }, [discovery, typed, session]);

  const remove = useCallback((address: GatewayAddress) => discovery.removeManual(address), [discovery]);

  return (
    <Section title={WORDS.title} note={WORDS.note} noteTestID="settings.mac.address.note">
      {manual.map((address, index) => {
        const shown = formatAddress(address);
        return (
          <Row
            key={shown}
            testID={`settings.mac.address.${index}`}
            label={shown}
            divided={index > 0}
            right={<IconButton testID={`settings.mac.address.${index}.remove`} accessibilityLabel={WORDS.remove(shown)} icon="close" tone="muted" onPress={() => remove(address)} />}
          />
        );
      })}
      <View style={[styles.form, manual.length > 0 ? styles.divided : null]}>
        <Field testID="settings.mac.address" label={WORDS.label} value={typed} onChange={setTyped} placeholder={WORDS.example} autoCapitalize="none" autoCorrect={false} keyboardType="numbers-and-punctuation" returnKeyType="done" onSubmitEditing={add} />
        {refused ? <Note testID="settings.mac.address.refused" text={refused} tone="red" alert /> : null}
        <Button testID="settings.mac.address.add" label={WORDS.add} kind="secondary" disabled={typed.trim() === ''} onPress={add} />
      </View>
    </Section>
  );
}
