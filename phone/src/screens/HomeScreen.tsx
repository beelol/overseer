import { useEffect, useMemo, useState } from 'react';
import { ScrollView, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';

import {
  CAPABILITY_NAMES,
  formatAddress,
  useCapabilities,
  type Capabilities,
  type CapabilityName,
  type Support,
} from '@/platform';
import { lineHeight, useTheme, type Theme } from '@/theme';

// The list runs under the home indicator; its last row clears it with the content's own padding.
const SAFE_EDGES = ['top', 'left', 'right'] as const;

type Checks = Readonly<Partial<Record<CapabilityName, Support>>>;

/** Asks every capability whether it works here. Support checks never throw or prompt. */
function useSupportChecks(capabilities: Capabilities): Checks {
  const [checks, setChecks] = useState<Checks>({});
  useEffect(() => {
    let current = true;
    for (const name of CAPABILITY_NAMES) {
      capabilities[name].support().then((support) => {
        if (current) setChecks((known) => ({ ...known, [name]: support }));
      });
    }
    return () => {
      current = false;
    };
  }, [capabilities]);
  return checks;
}

function describe(support: Support | undefined): string {
  if (support === undefined) return 'checking';
  return support.supported ? 'supported' : `unsupported: ${support.reason}`;
}

/**
 * The first screen of the foundation: temporary scaffolding that proves the theme, the tokens
 * and the platform layer on a device. The agents list replaces it.
 */
export function HomeScreen() {
  const theme = useTheme();
  const styles = useMemo(() => createStyles(theme), [theme]);
  const capabilities = useCapabilities();
  const checks = useSupportChecks(capabilities);
  const launch = capabilities.launch.info();
  const candidates = capabilities.discovery.candidates();

  const facts: readonly (readonly [label: string, value: string])[] = [
    ['Theme', theme.name],
    ['Platform', `${launch.device.platform} ${launch.device.systemVersion}`],
    ['Device', launch.device.model],
    ['Simulator', launch.isSimulator ? 'yes' : 'no'],
    ['Host addresses', launch.hostAddresses.join(', ') || 'none'],
    ['Addresses to try', candidates.map(formatAddress).join(', ') || 'none'],
    ['Engine', launch.runtime.engine],
    ['New Architecture', launch.runtime.newArchitecture ? 'on' : 'off'],
  ];

  return (
    <SafeAreaView edges={SAFE_EDGES} style={styles.screen}>
      <ScrollView contentContainerStyle={styles.content}>
        <Text accessibilityRole="header" style={styles.title}>
          Overseer
        </Text>
        <Text style={styles.subtitle}>Phone foundation</Text>

        <View style={styles.card}>
          <Text accessibilityRole="header" style={styles.heading}>
            Launch
          </Text>
          {facts.map(([label, value]) => (
            <View
              key={label}
              accessible
              accessibilityLabel={`${label}: ${value}`}
              style={styles.row}
            >
              <Text style={styles.label}>{label}</Text>
              <Text style={styles.value}>{value}</Text>
            </View>
          ))}
        </View>

        <View style={styles.card}>
          <Text accessibilityRole="header" style={styles.heading}>
            Capabilities
          </Text>
          {CAPABILITY_NAMES.map((name) => {
            const support = checks[name];
            const text = describe(support);
            return (
              <View
                key={name}
                accessible
                accessibilityLabel={`${name}: ${text}`}
                style={styles.row}
              >
                <Text style={styles.label}>{name}</Text>
                <Text
                  style={[
                    styles.value,
                    support?.supported === true && styles.supported,
                    support?.supported === false && styles.unsupported,
                  ]}
                  testID={`support-${name}`}
                >
                  {text}
                </Text>
              </View>
            );
          })}
        </View>
      </ScrollView>
    </SafeAreaView>
  );
}

function createStyles(theme: Theme) {
  const { colors, font, line, radius, space, weight } = theme;
  return StyleSheet.create({
    screen: { flex: 1, backgroundColor: colors.bg },
    content: { padding: space[4], paddingBottom: space[10], gap: space[4] },
    title: {
      color: colors.text,
      fontSize: font.xxl,
      fontWeight: weight.semibold,
      lineHeight: lineHeight(font.xxl, line.tight),
    },
    subtitle: {
      color: colors.muted,
      fontSize: font.lg,
      lineHeight: lineHeight(font.lg, line.body),
    },
    card: {
      backgroundColor: colors.raised,
      borderColor: colors.border,
      borderRadius: radius.card,
      borderWidth: StyleSheet.hairlineWidth,
      padding: space[4],
      gap: space[2],
    },
    heading: {
      color: colors.accent,
      fontSize: font.xl,
      fontWeight: weight.medium,
      lineHeight: lineHeight(font.xl, line.tight),
    },
    row: { flexDirection: 'row', gap: space[3] },
    label: {
      color: colors.muted,
      flexBasis: '36%',
      fontSize: font.lg,
      lineHeight: lineHeight(font.lg, line.body),
    },
    value: {
      color: colors.text,
      flex: 1,
      fontSize: font.lg,
      lineHeight: lineHeight(font.lg, line.body),
    },
    supported: { color: colors.green },
    unsupported: { color: colors.amber },
  });
}
