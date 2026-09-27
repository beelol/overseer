import { AgentsScreen } from '@/screens/AgentsScreen';
import { PairScreen } from '@/screens/PairScreen';
import { useSessionValue } from '@/session';

/**
 * Where the app opens: on Agents, drawn from what the phone has stored, or on pairing the first
 * time. It is the screen itself and not a redirect to it, so the first screen is in place, with
 * no transition, when the door opens. Until the stored pairing has been read (a few
 * milliseconds) the door is all there is.
 */
export default function Start() {
  const ready = useSessionValue((s) => s.ready);
  const paired = useSessionValue((s) => s.paired);
  if (!ready) return null;
  return paired ? <AgentsScreen /> : <PairScreen />;
}
