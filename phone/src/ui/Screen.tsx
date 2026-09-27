import { useRouter } from 'expo-router';
import type { ReactNode } from 'react';
import { KeyboardAvoidingView, View } from 'react-native';
import { SafeAreaView, type Edge } from 'react-native-safe-area-context';

import { useCapabilities } from '@/platform';

import { IconButton } from './Button';
import { ConnectionLine } from './ConnectionLine';
import { makeStyles } from './styles';
import { Txt } from './Txt';

export interface ScreenProps {
  /** The start of this screen's test ids (`agents`, `agent`, `settings`). */
  readonly id: string;
  readonly title: string;
  /** A quiet second line under the title (an agent's status). */
  readonly subtitle?: string;
  /** False on the first screen: nothing to go back to. */
  readonly back?: boolean;
  /** Controls at the right of the header. */
  readonly actions?: ReactNode;
  /** False on screens with no live data (pairing). */
  readonly connection?: boolean;
  /** Held at the bottom, above the keyboard and the home indicator (a composer, a main button). */
  readonly footer?: ReactNode;
  readonly children: ReactNode;
}

const TOP: readonly Edge[] = ['top', 'left', 'right'];
const BOTTOM: readonly Edge[] = ['bottom', 'left', 'right'];

const useStyles = makeStyles((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.bg },
  header: { minHeight: theme.phone.size.header, flexDirection: 'row', alignItems: 'center', paddingHorizontal: theme.space[2], gap: theme.space[1], backgroundColor: theme.colors.chrome, borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
  titles: { flex: 1, paddingHorizontal: theme.space[2], paddingVertical: theme.space[1] },
  actions: { flexDirection: 'row', alignItems: 'center' },
  top: { backgroundColor: theme.colors.chrome },
  body: { flex: 1 },
  footer: { backgroundColor: theme.colors.chrome, borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
}));

/**
 * The frame of every screen: the header with its title, the connection line under it, the
 * content, and what is held at the bottom within reach of a thumb.
 */
export function Screen({ id, title, subtitle, back = true, actions, connection = true, footer, children }: ScreenProps) {
  const styles = useStyles();
  const router = useRouter();
  const { launch } = useCapabilities();
  return (
    <View style={styles.screen} testID={`${id}.screen`}>
      <SafeAreaView edges={TOP} style={styles.top}>
        <View style={styles.header}>
          {back ? <IconButton testID={`${id}.back`} accessibilityLabel="Back" icon="chevron-left" onPress={() => (router.canGoBack() ? router.back() : router.replace('/agents'))} /> : null}
          <View style={styles.titles}>
            <Txt testID={`${id}.title`} kind="strong" numberOfLines={1} accessibilityRole="header">
              {title}
            </Txt>
            {subtitle ? (
              <Txt testID={`${id}.subtitle`} kind="small" tone="muted" numberOfLines={1}>
                {subtitle}
              </Txt>
            ) : null}
          </View>
          <View style={styles.actions}>{actions}</View>
        </View>
      </SafeAreaView>
      {connection ? <ConnectionLine /> : null}
      <KeyboardAvoidingView style={styles.body} behavior={launch.info().conventions.keyboard}>
        <View style={styles.body}>{children}</View>
        {footer ? (
          <SafeAreaView edges={BOTTOM} style={styles.footer}>
            {footer}
          </SafeAreaView>
        ) : (
          <SafeAreaView edges={BOTTOM} />
        )}
      </KeyboardAvoidingView>
    </View>
  );
}
