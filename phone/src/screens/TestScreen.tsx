import { useLocalSearchParams } from 'expo-router';
import { useMemo } from 'react';
import { View } from 'react-native';

import { SLOW_LIMIT_MS, type TestSettings } from '@/door';
import { useCapabilities } from '@/platform';
import { makeStyles, Screen, Txt } from '@/ui';

const useStyles = makeStyles((theme) => ({ content: { padding: theme.space[4], gap: theme.space[2] } }));

/**
 * Where the scenario run sets what it measures with: `overseer://test?door=off`,
 * `overseer://test?slow=400`, `overseer://test?door=on&slow=0`. What is set holds from the next
 * start of the app. Nothing in the app leads here.
 */
export function TestScreen() {
  const styles = useStyles();
  const params = useLocalSearchParams<{ door?: string; slow?: string }>();
  const { keyValue } = useCapabilities();
  const now = useMemo(() => {
    const settings = keyValue.scope<TestSettings>('test');
    if (params.door === 'off' || params.door === 'on') settings.set('door', params.door);
    const slow = Number(params.slow);
    if (params.slow !== undefined && Number.isFinite(slow) && slow >= 0) settings.set('slow', Math.min(Math.round(slow), SLOW_LIMIT_MS));
    return { door: settings.get('door') ?? 'on', slow: settings.get('slow') ?? 0 };
  }, [keyValue, params.door, params.slow]);
  return (
    <Screen id="test" title="Test settings" connection={false}>
      <View style={styles.content}>
        <Txt testID="test.door">{`door: ${now.door}`}</Txt>
        <Txt testID="test.slow">{`slow: ${now.slow}`}</Txt>
        <Txt kind="small" tone="muted">
          These hold from the next start of the app.
        </Txt>
      </View>
    </Screen>
  );
}
