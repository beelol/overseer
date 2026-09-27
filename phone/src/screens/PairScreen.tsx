import { useRouter } from 'expo-router';
import { useCallback, useEffect, useRef, useState } from 'react';
import { ScrollView, View } from 'react-native';

import { Arrive, Pulse } from '@/motion';
import { useCapabilities } from '@/platform';
import { routes } from '@/routes';
import { useSession } from '@/session';
import { Button, Icon, makeStyles, Screen, Txt } from '@/ui';

import { pairingSentence } from './pair/failure';
import { Field } from './pair/Field';
import { targetOf, useMacName, type Target } from './pair/target';
import { useCamera } from './pair/useCamera';
import { PAIR } from './pair/words';
import { ALLOW_NOTIFICATIONS, allowNotifications, delivers, NOTIFICATIONS_REASON, tellTheMac } from './settings/push';

type Step = 'code' | 'waiting' | 'notifications';

const NAME_LENGTH = 64;
const CAMERA_ROWS = 5;

const useStyles = makeStyles((theme) => ({
  content: { flexGrow: 1, padding: theme.space[4], gap: theme.space[4] },
  camera: {
    flex: 1,
    minHeight: theme.space[10] * CAMERA_ROWS,
    borderRadius: theme.radius.card,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.chrome,
    overflow: 'hidden',
  },
  fill: { flex: 1 },
  centre: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    gap: theme.space[3],
    padding: theme.space[5],
  },
  centred: { textAlign: 'center' },
  steps: { gap: theme.space[2] },
  step: { flexDirection: 'row', gap: theme.space[2] },
  stepText: { flex: 1 },
  footer: { padding: theme.space[4], gap: theme.space[2] },
}));

/**
 * Pair with your Mac: scan the code or type it, name the phone, confirm on the Mac. It is
 * shown the first time, and again only after the Mac removed this phone.
 */
export interface PairScreenProps {
  /**
   * Called when pairing and what follows it (the question about notifications) are over. The
   * first screen keeps pairing on the display until then.
   */
  readonly onDone?: () => void;
}

export function PairScreen({ onDone }: PairScreenProps = {}) {
  const styles = useStyles();
  const router = useRouter();
  const session = useSession();
  const capabilities = useCapabilities();
  const { camera, haptics, launch, push } = capabilities;
  const device = launch.info().device;
  const eye = useCamera();

  const [step, setStep] = useState<Step>('code');
  const [name, setName] = useState(device.model);
  const [code, setCode] = useState('');
  const [typing, setTyping] = useState(false);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const mac = useMacName(step === 'waiting' ? target : null);

  const busy = useRef(false);
  const shown = useRef(true);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);

  const toAgents = useCallback(() => {
    onDone?.();
    router.replace(routes.agents);
  }, [router, onDone]);

  const pair = useCallback(
    async (given: string, to: Target) => {
      busy.current = true;
      setError(null);
      setTarget(to);
      setStep('waiting');
      try {
        await session.pair(given, name.trim() || device.model, device.platform);
      } catch (failure) {
        busy.current = false;
        if (!shown.current) return;
        // Paired already, by an earlier try that was answered late: there is nothing to repeat.
        if (session.getSnapshot().paired) {
          toAgents();
          return;
        }
        haptics.play('reject');
        setError(pairingSentence(failure));
        setPaused(true);
        setStep('code');
        return;
      }
      if (!shown.current) return;
      haptics.play('confirm');
      setCode('');
      // Asked once, after pairing, with the reason first. Where the system delivers nothing, or
      // was answered before, nothing is asked: an owner who said yes before (this phone was
      // paired earlier) has notifications on again, and one who said no is left alone.
      const permission = await push
        .support()
        .then(async (support) => (support.supported ? await push.permission() : null))
        .catch(() => null);
      if (!shown.current) return;
      if (permission === 'undetermined') {
        setStep('notifications');
        return;
      }
      if (delivers(permission)) tellTheMac({ push, launch }, session).catch(() => undefined);
      // Where the system delivers nothing, the app shows what needs the owner itself, while it
      // is open. There is nothing to ask the system; the switches in Settings turn it off.
      if (permission === null) session.setNotifications({ enabled: true }).catch(() => undefined);
      toAgents();
    },
    [session, name, device, haptics, push, launch, toAgents],
  );

  const take = useCallback(
    (text: string, from: 'camera' | 'typed') => {
      if (busy.current) return;
      const given = text.trim();
      // The camera reads every code held in front of it. Only Overseer's are taken.
      if (from === 'camera' && !given.toUpperCase().startsWith('OVSR')) return;
      let to: Target;
      try {
        to = targetOf(given);
      } catch {
        haptics.play('reject');
        setError(PAIR.codeDidNotWork);
        if (from === 'camera') setPaused(true);
        return;
      }
      void pair(given, to);
    },
    [haptics, pair],
  );

  const scanned = useCallback((text: string) => take(text, 'camera'), [take]);

  const allow = useCallback(() => {
    allowNotifications(capabilities, session)
      .catch(() => undefined)
      .then(() => {
        if (shown.current) toAgents();
      });
  }, [capabilities, session, toAgents]);

  if (step === 'notifications') {
    return (
      <Screen
        id="pair"
        title={PAIR.title}
        back={false}
        connection={false}
        footer={
          <View style={styles.footer}>
            <Button
              testID="pair.notifications.allow"
              label={ALLOW_NOTIFICATIONS}
              kind="primary"
              wide
              haptic="confirm"
              onPress={allow}
            />
            <Button
              testID="pair.notifications.later"
              label={PAIR.notNow}
              kind="quiet"
              wide
              onPress={toAgents}
            />
          </View>
        }
      >
        <View style={styles.centre}>
          <Icon name="bell" size="xl" tone="accent" />
          <Txt testID="pair.notifications.reason" kind="body" style={styles.centred}>
            {NOTIFICATIONS_REASON}
          </Txt>
        </View>
      </Screen>
    );
  }

  if (step === 'waiting') {
    return (
      <Screen id="pair" title={PAIR.title} back={false} connection={false}>
        <View style={styles.centre} accessibilityLiveRegion="polite">
          <Pulse>
            <Icon name="device-mobile" size="xl" tone="accent" />
          </Pulse>
          <Txt testID="pair.waiting" kind="heading" style={styles.centred}>
            {PAIR.confirm}
          </Txt>
          {mac ? (
            <Txt testID="pair.mac" kind="body" tone="muted" style={styles.centred}>
              {mac}
            </Txt>
          ) : null}
        </View>
      </Screen>
    );
  }

  const Scanner = camera.CodeScanner;
  // Without a camera the field is there from the start: on simulators, and when it was refused.
  const field = typing || eye.state === 'none' || eye.state === 'off';

  return (
    <Screen
      id="pair"
      title={PAIR.title}
      back={false}
      connection={false}
      footer={
        <View style={styles.footer}>
          {field ? (
            <Button
              testID="pair.submit"
              label={PAIR.pair}
              kind="primary"
              wide
              disabled={code.trim().length === 0}
              onPress={() => take(code, 'typed')}
            />
          ) : (
            <Button
              testID="pair.type"
              label={PAIR.typeTheCode}
              wide
              onPress={() => setTyping(true)}
            />
          )}
        </View>
      }
    >
      <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={styles.content}>
        {eye.state === 'on' || eye.state === 'ask' ? (
          <View style={styles.camera}>
            {eye.state === 'ask' ? (
              <View style={styles.centre}>
                <Icon name="device-camera" size="xl" tone="muted" />
                <Txt testID="pair.camera.reason" kind="label" tone="muted" style={styles.centred}>
                  {PAIR.cameraReason}
                </Txt>
                <Button
                  testID="pair.camera.allow"
                  label={PAIR.useTheCamera}
                  kind="primary"
                  onPress={eye.allow}
                />
              </View>
            ) : paused ? (
              <View style={styles.centre}>
                <Button
                  testID="pair.scan"
                  label={PAIR.scanAgain}
                  icon="device-camera"
                  onPress={() => setPaused(false)}
                />
              </View>
            ) : (
              <Scanner
                active
                onCode={scanned}
                accessibilityLabel={PAIR.camera}
                style={styles.fill}
              />
            )}
          </View>
        ) : null}

        <View style={styles.steps}>
          <View style={styles.step}>
            <Txt kind="body" tone="muted">
              1.
            </Txt>
            <Txt testID="pair.step.mac" kind="body" style={styles.stepText}>
              {PAIR.onTheMac}
              <Txt kind="strong">{PAIR.pairAPhone}</Txt>.
            </Txt>
          </View>
          <View style={styles.step}>
            <Txt kind="body" tone="muted">
              2.
            </Txt>
            <Txt testID="pair.step.scan" kind="body" style={styles.stepText}>
              {PAIR.scan}
            </Txt>
          </View>
        </View>

        {eye.state === 'off' ? (
          <Txt testID="pair.camera.off" kind="label" tone="muted">
            {PAIR.cameraOff}
          </Txt>
        ) : null}

        {error ? (
          <Arrive>
            <Txt
              testID="pair.error"
              kind="label"
              tone="red"
              accessibilityRole="alert"
              accessibilityLiveRegion="polite"
            >
              {error}
            </Txt>
          </Arrive>
        ) : null}

        <Field
          testID="pair.name"
          label={PAIR.name}
          value={name}
          onChange={setName}
          maxLength={NAME_LENGTH}
          autoCapitalize="words"
          autoCorrect={false}
          returnKeyType="done"
        />

        {field ? (
          <Field
            testID="pair.code"
            label={PAIR.code}
            value={code}
            onChange={setCode}
            placeholder={PAIR.codeHint}
            multiline
            submitBehavior="blurAndSubmit"
            autoCapitalize="characters"
            autoCorrect={false}
            autoComplete="off"
            spellCheck={false}
            returnKeyType="go"
            onSubmitEditing={() => take(code, 'typed')}
          />
        ) : null}
      </ScrollView>
    </Screen>
  );
}
