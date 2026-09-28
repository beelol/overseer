import { setStringAsync } from 'expo-clipboard';
import { openURL } from 'expo-linking';
import { useCallback, useEffect, useState } from 'react';
import { View } from 'react-native';

import { RequestError } from '@/core';
import { Pulse } from '@/motion';
import { useCapabilities, useLive } from '@/platform';
import { useSession } from '@/session';
import { lineHeight } from '@/theme';
import { Button, makeStyles, Sheet, Txt, weight } from '@/ui';

import { readSignIn, readStatus, type Account } from './accounts';
import { ACCOUNTS } from './words';

/** How often the Mac is asked whether the sign-in has finished, while the sheet is open. */
export const SIGN_IN_POLL_MS = 3_000;
const COPIED_MS = 2_000;

type Step =
  | { readonly at: 'asking' }
  /** `since` and `until` are moments of the phone's clock. The code lives here and nowhere else. */
  | {
      readonly at: 'code';
      readonly url: string;
      readonly code: string;
      readonly since: number;
      readonly until: number | null;
    }
  | { readonly at: 'mac' }
  | { readonly at: 'old' }
  | { readonly at: 'failed' };

const useStyles = makeStyles((theme) => ({
  content: {
    paddingHorizontal: theme.space[4],
    paddingBottom: theme.space[4],
    gap: theme.space[3],
  },
  code: {
    fontSize: theme.phone.font.code,
    lineHeight: lineHeight(theme.phone.font.code, theme.line.body),
    fontWeight: weight(theme.weight.semibold),
    textAlign: 'center',
    paddingVertical: theme.space[3],
    paddingHorizontal: theme.space[3],
    borderRadius: theme.radius.card,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.borderStrong,
    backgroundColor: theme.colors.raised2,
    overflow: 'hidden',
  },
  centred: { textAlign: 'center' },
}));

export interface SignInSheetProps {
  /** The account to sign in, or `null` while the sheet is closed. */
  readonly account: Account | null;
  readonly onClose: () => void;
  /** The Mac says the account is signed in. */
  readonly onSignedIn: (id: string) => void;
  readonly pollMs?: number;
}

/**
 * Sign in with a code: the Mac starts the provider's own sign-in and hands over the address and
 * the code; the person finishes in the phone's browser and the sheet closes by itself. No
 * credential passes through the phone, and the code is never stored.
 */
export function SignInSheet({
  account,
  onClose,
  onSignedIn,
  pollMs = SIGN_IN_POLL_MS,
}: SignInSheetProps) {
  const [tries, setTries] = useState(0);
  const again = useCallback(() => setTries((n) => n + 1), []);
  return (
    <Sheet
      testID="accounts.signin"
      open={account !== null}
      onClose={onClose}
      title={account ? `${ACCOUNTS.signIn} · ${account.name}` : ACCOUNTS.signIn}
    >
      {/* Each opening and each new try starts from nothing: what an earlier one held is gone. */}
      {account ? (
        <SignIn
          key={`${account.id}.${tries}`}
          id={account.id}
          onSignedIn={onSignedIn}
          onAgain={again}
          onClose={onClose}
          pollMs={pollMs}
        />
      ) : null}
    </Sheet>
  );
}

interface SignInProps {
  readonly id: string;
  readonly onSignedIn: (id: string) => void;
  readonly onAgain: () => void;
  readonly onClose: () => void;
  readonly pollMs: number;
}

function SignIn({ id, onSignedIn, onAgain, onClose, pollMs }: SignInProps) {
  const styles = useStyles();
  const session = useSession();
  const { random, haptics, appState } = useCapabilities();
  const phase = useLive(appState);
  const [step, setStep] = useState<Step>({ at: 'asking' });
  const [copied, setCopied] = useState(false);

  // Opening asks the Mac for a code, once.
  useEffect(() => {
    let current = true;
    const requestId = random.uuid();
    session
      .request('profile.device_login', { id }, { requestId })
      .then((answer) => {
        if (!current) return;
        const read = readSignIn(answer);
        if (read.kind === 'signedIn') onSignedIn(id);
        else if (read.kind === 'failed') setStep({ at: 'failed' });
        else
          setStep({
            at: 'code',
            url: read.url,
            code: read.code,
            since: Date.now(),
            until: read.validMs === null ? null : Date.now() + read.validMs,
          });
      })
      .catch((error: unknown) => {
        if (!current) return;
        setStep({
          at: error instanceof RequestError && error.code === 'mac_only' ? 'mac' : 'failed',
        });
      })
      // The answer holds the code: it is not left among the answered requests.
      .finally(() => session.dismiss(requestId));
    return () => {
      current = false;
    };
  }, [id, session, random, onSignedIn]);

  // It finishes by itself: the Mac is asked while the sheet is open and the app is in front,
  // and at once when the person comes back from the browser.
  const waiting = step.at === 'code' ? step : null;
  useEffect(() => {
    if (waiting === null || phase !== 'foreground') return undefined;
    let current = true;
    let asking = false;
    const check = async (): Promise<void> => {
      if (asking || !current) return;
      asking = true;
      try {
        if (waiting.until !== null && Date.now() > waiting.until) {
          setStep({ at: 'old' });
          return;
        }
        const status = readStatus(await session.request('profile.status', { id }));
        if (current && status.signedIn) {
          haptics.play('confirm');
          onSignedIn(id);
        }
      } catch {
        // The Mac could not be asked now. The next time will do.
      } finally {
        asking = false;
      }
    };
    const first = Date.now() - waiting.since >= pollMs ? setTimeout(() => void check(), 0) : null;
    const timer = setInterval(() => void check(), pollMs);
    return () => {
      current = false;
      if (first !== null) clearTimeout(first);
      clearInterval(timer);
    };
  }, [id, waiting, phase, pollMs, session, haptics, onSignedIn]);

  useEffect(() => {
    if (!copied) return undefined;
    const timer = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = useCallback(() => {
    if (!waiting) return;
    setStringAsync(waiting.code)
      .then(() => setCopied(true))
      .catch(() => undefined);
  }, [waiting]);

  const open = useCallback(() => {
    if (waiting) openURL(waiting.url).catch(() => undefined);
  }, [waiting]);

  return (
    <View style={styles.content}>
      {step.at === 'asking' ? (
        <Pulse>
          <Txt
            testID="accounts.signin.asking"
            kind="label"
            tone="muted"
            style={styles.centred}
            accessibilityLiveRegion="polite"
          >
            {ACCOUNTS.sheet.asking}
          </Txt>
        </Pulse>
      ) : null}

      {step.at === 'code' ? (
        <>
          <Txt kind="label" tone="muted" style={styles.centred}>
            {ACCOUNTS.sheet.enter}
          </Txt>
          <Txt
            testID="accounts.signin.code"
            selectable
            accessibilityLabel={ACCOUNTS.sheet.code(step.code)}
            style={styles.code}
          >
            {step.code}
          </Txt>
          <Button
            testID="accounts.signin.open"
            label={ACCOUNTS.sheet.open}
            kind="primary"
            icon="link-external"
            wide
            onPress={open}
          />
          <Button
            testID="accounts.signin.copy"
            label={copied ? ACCOUNTS.sheet.copied : ACCOUNTS.sheet.copy}
            icon={copied ? 'check' : 'copy'}
            wide
            haptic="selection"
            onPress={copy}
          />
          <Txt testID="accounts.signin.closes" kind="small" tone="muted" style={styles.centred}>
            {ACCOUNTS.sheet.closes}
          </Txt>
        </>
      ) : null}

      {step.at === 'mac' ? (
        <Txt
          testID="accounts.signin.onmac"
          kind="body"
          style={styles.centred}
          accessibilityRole="alert"
        >
          {ACCOUNTS.onTheMac}
        </Txt>
      ) : null}

      {step.at === 'old' || step.at === 'failed' ? (
        <>
          <Txt
            testID={step.at === 'old' ? 'accounts.signin.old' : 'accounts.signin.failed'}
            kind="body"
            style={styles.centred}
            accessibilityRole="alert"
          >
            {step.at === 'old' ? ACCOUNTS.sheet.old : ACCOUNTS.sheet.failed}
          </Txt>
          <Button
            testID="accounts.signin.again"
            label={step.at === 'old' ? ACCOUNTS.sheet.newCode : ACCOUNTS.sheet.again}
            kind="primary"
            wide
            onPress={onAgain}
          />
        </>
      ) : null}

      {/* Within the sheet, so that it is reached by VoiceOver and TalkBack too. */}
      <Button
        testID="accounts.signin.done"
        label={ACCOUNTS.sheet.close}
        kind="quiet"
        wide
        onPress={onClose}
      />
    </View>
  );
}
