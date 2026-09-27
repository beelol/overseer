import { useEffect, useState } from 'react';

import { text } from '@/model';
import { Arrive } from '@/motion';
import { useSessionValue } from '@/session';

import { makeStyles } from './styles';
import { Txt } from './Txt';

const useStyles = makeStyles((theme) => ({
  line: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[2], backgroundColor: theme.colors.raised, borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
}));

/** What the connection line says in a state of the connection, and its test id. */
export function connectionText(state: string, lastContact: number | null, now: number): { readonly id: string; readonly text: string } | null {
  switch (state) {
    case 'connecting':
    case 'reconnecting':
      return { id: 'connection.reconnecting', text: 'Reconnecting…' };
    case 'unreachable':
      return { id: 'connection.unreachable', text: lastContact === null ? 'Mac unreachable' : `Mac unreachable · last contact ${text.agoInWords(lastContact, now)}` };
    case 'off':
      return { id: 'connection.off', text: 'Phone access is off on the Mac' };
    case 'revoked':
      return { id: 'connection.revoked', text: 'This phone was removed on the Mac' };
    default:
      return null;
  }
}

/** The clock, once a minute: for "2 min ago" that stays true while the screen is open. */
export function useMinute(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

/**
 * One quiet line under the header on every screen that shows live data. Hidden while connected.
 * It never blocks anything: what is stored stays on screen and what is sent is queued.
 */
export function ConnectionLine() {
  const styles = useStyles();
  const state = useSessionValue((s) => s.connection);
  const lastContact = useSessionValue((s) => s.lastContact);
  const now = useMinute();
  const line = connectionText(state, lastContact, now);
  if (!line) return null;
  return (
    <Arrive from="above" style={styles.line}>
      <Txt testID={line.id} kind="label" tone="muted" accessibilityRole="alert" accessibilityLiveRegion="polite">
        {line.text}
      </Txt>
    </Arrive>
  );
}

/** Shown in place of the controls a watch-only phone does not have. */
export function WatchOnlyLine() {
  const styles = useStyles();
  return (
    <Txt testID="watch.line" kind="label" tone="muted" style={styles.line}>
      This phone may watch. Change it on the Mac.
    </Txt>
  );
}
