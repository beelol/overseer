import { Redirect } from 'expo-router';

import { routes } from '@/routes';
import { useSessionValue } from '@/session';

/**
 * Where the app opens: on Agents, drawn from what the phone has stored, or on pairing the first
 * time. Until the stored pairing has been read (a few milliseconds) the door is all there is.
 */
export default function Start() {
  const ready = useSessionValue((s) => s.ready);
  const paired = useSessionValue((s) => s.paired);
  if (!ready) return null;
  return <Redirect href={paired ? routes.agents : routes.pair} />;
}
