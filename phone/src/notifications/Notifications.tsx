import { useGlobalSearchParams, usePathname, useRouter } from 'expo-router';
import { useCallback, useEffect, useRef, useState } from 'react';
import { View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { isActive, store, type Run } from '@/model';
import { Arrive, Tap } from '@/motion';
import { useCapabilities, type Support } from '@/platform';
import { routes } from '@/routes';
import { useSession, useSessionValue, type Session } from '@/session';
import { IconButton, Logo, makeStyles, Txt } from '@/ui';

import { actOf, CATEGORIES, SENTENCE, type NotificationAct, type NotificationKind } from './handle';

/** How long a banner for news stays: an agent finished or failed. */
const BANNER_MS = 6_000;
/**
 * Kinds that wait for the owner. Their banner stays until the owner opens the agent or dismisses
 * it, or until the agent no longer waits: on a phone without push it is the only signal.
 */
const NEEDS: ReadonlySet<NotificationKind> = new Set(['permission', 'question']);

/** Sends the owner's answer, or opens the agent. Used by taps on notifications and banners. */
function useAct(session: Session): (act: NotificationAct) => void {
  const router = useRouter();
  return useCallback(
    (act) => {
      if (act.do === 'open') {
        router.push(routes.agent(act.runId));
        return;
      }
      // Queued and sent once, like every change: it reaches the Mac even if the phone was away.
      session.request('run.permission', { run_id: act.runId, request_id: act.requestId, allow: act.allow }).catch(() => undefined);
    },
    [router, session],
  );
}

/**
 * Notifications, from both ends. Where the system delivers them (iOS), this registers the
 * actions, gives the Mac the phone's token, and acts on a tap. Where it does not (Android in
 * this gate), the app shows what needs the owner itself, as a banner, while it is open.
 * The same switches rule both: off means nothing is shown.
 */
export function Notifications() {
  const session = useSession();
  const { push, launch } = useCapabilities();
  const act = useAct(session);
  const paired = useSessionValue((s) => s.paired);
  const online = useSessionValue((s) => s.connection === 'online');
  const [support, setSupport] = useState<Support | null>(null);

  useEffect(() => {
    let current = true;
    push.support().then((s) => current && setSupport(s));
    return () => {
      current = false;
    };
  }, [push]);

  // Taps on notifications, including the one that opened the app.
  useEffect(() => {
    if (!support?.supported) return;
    push.setCategories(CATEGORIES).catch(() => undefined);
    push.setForegroundPresentation('show');
    const off = push.onResponse((response) => {
      const asked = actOf(response);
      if (asked) act(asked);
    });
    push
      .launchResponse()
      .then((response) => {
        const asked = response ? actOf(response) : null;
        if (asked) act(asked);
      })
      .catch(() => undefined);
    return off;
  }, [support, push, act]);

  // The Mac learns where to send: only after the owner allowed notifications.
  useEffect(() => {
    if (!support?.supported || !paired || !online) return;
    let current = true;
    (async () => {
      const permission = await push.permission();
      if (!current || (permission !== 'granted' && permission !== 'provisional')) return;
      const simulator = launch.info().isSimulator;
      // A simulator has no address at Apple's service: the Mac reaches it by its own tool.
      const token = simulator ? 'booted' : (await push.deviceToken()).token;
      if (!current) return;
      await session.request('device.notifications', { token, environment: simulator ? 'simulator' : 'device' });
    })().catch(() => undefined);
    return () => {
      current = false;
    };
  }, [support, paired, online, push, launch, session]);

  if (support === null || support.supported) return null;
  return <Banners onAct={act} />;
}

interface Banner {
  readonly key: string;
  readonly run: Run;
  readonly kind: NotificationKind;
  readonly title: string;
}

function kindOf(run: Run, before: Run | undefined): NotificationKind | null {
  if (run.attention && run.attention.request_id !== before?.attention?.request_id) return run.attention.kind === 'question' ? 'question' : 'permission';
  if (before && isActive(before.status) && !isActive(run.status)) return run.status === 'failed' ? 'failure' : run.status === 'completed' ? 'finished' : null;
  return null;
}

const useStyles = makeStyles((theme) => ({
  place: { position: 'absolute', left: theme.space[3], right: theme.space[3] },
  banner: { flexDirection: 'row', alignItems: 'center', gap: theme.space[3], padding: theme.space[3], borderRadius: theme.radius.card, backgroundColor: theme.colors.raised2, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong },
  texts: { flex: 1 },
}));

/** What needs the owner, shown by the app itself while it is open. */
function Banners({ onAct }: { readonly onAct: (act: NotificationAct) => void }) {
  const styles = useStyles();
  const session = useSession();
  const insets = useSafeAreaInsets();
  const { haptics } = useCapabilities();
  const pathname = usePathname();
  const params = useGlobalSearchParams<{ run?: string }>();
  const looking = pathname.startsWith('/agent/') ? params.run : undefined;
  const lookingRef = useRef(looking);
  useEffect(() => {
    lookingRef.current = looking;
  }, [looking]);
  const [banner, setBanner] = useState<Banner | null>(null);
  const shown = useRef<Banner | null>(null);
  const show = useCallback((next: Banner | null) => {
    shown.current = next;
    setBanner(next);
  }, []);

  useEffect(() => {
    let before = session.getSnapshot();
    return session.subscribe(() => {
      const now = session.getSnapshot();
      const was = before;
      before = now;
      // History is not news: what was stored, or replayed after time away, rings nothing.
      if (now.fromCache || was.fromCache || now.connection !== 'online' || now.state === was.state) return;
      const settings = session.notificationSettings();
      // A banner that waited for the owner goes once the agent no longer waits for that answer.
      const current = shown.current;
      if (current && NEEDS.has(current.kind) && store.run(now.state, current.run.id)?.attention?.request_id !== current.run.attention?.request_id) show(null);
      if (!now.macNotifications || !settings.enabled) return;
      for (const run of store.rows(now.state.runs)) {
        const earlier = store.run(was.state, run.id);
        if (earlier === run) continue;
        const kind = kindOf(run, earlier);
        if (!kind || !settings.kinds[kind] || lookingRef.current === run.id || run.parent_run_id) continue;
        const task = store.task(now.state, run.task_id);
        const repo = task?.repo_root.split('/').filter(Boolean).pop() ?? '';
        // News never covers what waits for the owner.
        if (shown.current && NEEDS.has(shown.current.kind) && !NEEDS.has(kind)) continue;
        haptics.play(kind === 'finished' ? 'confirm' : 'warning');
        show({ key: `${run.id}.${kind}.${run.attention?.request_id ?? run.status}`, run, kind, title: [run.harness, repo].filter(Boolean).join(' · ') });
        return;
      }
    });
  }, [session, haptics, show]);

  useEffect(() => {
    if (!banner || NEEDS.has(banner.kind)) return;
    const timer = setTimeout(() => show(null), BANNER_MS);
    return () => clearTimeout(timer);
  }, [banner, show]);

  if (!banner) return null;
  return (
    <View pointerEvents="box-none" style={[styles.place, { top: insets.top }]}>
      <Arrive key={banner.key} from="above">
        <Tap
          testID="notification.banner"
          accessibilityLabel={`${banner.title}. ${SENTENCE[banner.kind]}`}
          accessibilityRole="alert"
          scales={false}
          style={styles.banner}
          onPress={() => {
            show(null);
            onAct({ do: 'open', runId: banner.run.id });
          }}
        >
          <Logo name={banner.run.harness} />
          <View style={styles.texts}>
            <Txt kind="label" numberOfLines={1}>
              {banner.title}
            </Txt>
            <Txt kind="small" tone="muted" numberOfLines={1}>
              {SENTENCE[banner.kind]}
            </Txt>
          </View>
          {NEEDS.has(banner.kind) ? <IconButton testID="notification.banner.dismiss" accessibilityLabel={`Dismiss: ${banner.title}`} icon="close" haptic="selection" onPress={() => show(null)} /> : null}
        </Tap>
      </Arrive>
    </View>
  );
}
