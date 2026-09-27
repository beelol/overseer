import { useCallback, useState } from 'react';

import { AgentsScreen } from '@/screens/AgentsScreen';
import { PairScreen } from '@/screens/PairScreen';
import { useSessionValue } from '@/session';

/**
 * Where the app opens: on Agents, drawn from what the phone has stored, or on pairing the first
 * time. It is the screen itself and not a redirect to it, so the first screen is in place, with
 * no transition, when the door opens. Until the stored pairing has been read (a few
 * milliseconds) the door is all there is.
 *
 * Pairing stays on the display until it says it is over: after the Mac confirmed, it still asks
 * about notifications, once.
 */
export default function Start() {
  const ready = useSessionValue((s) => s.ready);
  const paired = useSessionValue((s) => s.paired);
  const [pairing, setPairing] = useState(false);
  const done = useCallback(() => setPairing(false), []);
  if (!ready) return null;
  if (!paired && !pairing) setPairing(true);
  return paired && !pairing ? <AgentsScreen /> : <PairScreen onDone={done} />;
}
