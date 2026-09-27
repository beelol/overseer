import type { ReactNode } from 'react';
import { Switch, View } from 'react-native';

import { Tap, type TapProps } from '@/motion';
import { useCapabilities } from '@/platform';
import { useTheme } from '@/theme';

import { Button } from './Button';
import { Icon, type IconName } from './Icon';
import { makeStyles } from './styles';
import { Txt } from './Txt';

const useStyles = makeStyles((theme) => ({
  section: { paddingTop: theme.space[5] },
  sectionTitle: { paddingHorizontal: theme.space[4], paddingBottom: theme.space[2] },
  card: { marginHorizontal: theme.space[4], borderRadius: theme.radius.card, backgroundColor: theme.colors.raised, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.border, overflow: 'hidden' },
  note: { paddingHorizontal: theme.space[4], paddingTop: theme.space[2] },
  row: { minHeight: theme.phone.size.touch, paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], flexDirection: 'row', alignItems: 'center', gap: theme.space[3] },
  divided: { borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
  texts: { flex: 1, gap: theme.space[1] },
  value: { flexShrink: 1, alignItems: 'flex-end' },
  empty: { flex: 1, alignItems: 'center', justifyContent: 'center', padding: theme.space[8], gap: theme.space[4] },
}));

export interface SectionProps {
  readonly title?: string;
  /** A sentence under the card. */
  readonly note?: string;
  readonly children: ReactNode;
}

/** A titled group of rows on a raised card. */
export function Section({ title, note, children }: SectionProps) {
  const styles = useStyles();
  return (
    <View style={styles.section}>
      {title ? (
        <Txt kind="small" tone="muted" accessibilityRole="header" style={styles.sectionTitle}>
          {title.toUpperCase()}
        </Txt>
      ) : null}
      <View style={styles.card}>{children}</View>
      {note ? (
        <Txt kind="small" tone="muted" style={styles.note}>
          {note}
        </Txt>
      ) : null}
    </View>
  );
}

export interface RowProps {
  readonly testID: string;
  readonly label: string;
  /** A quiet second line. */
  readonly detail?: string;
  /** Shown at the right: a value, in words. */
  readonly value?: string;
  readonly icon?: IconName;
  /** True for every row of a section but the first: a line above it. */
  readonly divided?: boolean;
  readonly tone?: 'text' | 'red' | 'link';
  /** Given, the row is pressed and shows that it leads somewhere. */
  readonly onPress?: TapProps['onPress'];
  readonly right?: ReactNode;
}

/** A row of a section: a label, and a value or somewhere to go. */
export function Row({ testID, label, detail, value, icon, divided, tone = 'text', onPress, right }: RowProps) {
  const styles = useStyles();
  const content = (
    <>
      {icon ? <Icon name={icon} size="lg" tone={tone === 'text' ? 'muted' : tone} /> : null}
      <View style={styles.texts}>
        <Txt kind="body" tone={tone}>
          {label}
        </Txt>
        {detail ? (
          <Txt kind="small" tone="muted">
            {detail}
          </Txt>
        ) : null}
      </View>
      {value ? (
        <View style={styles.value}>
          <Txt kind="label" tone="muted" selectable>
            {value}
          </Txt>
        </View>
      ) : null}
      {right}
      {onPress && !right ? <Icon name="chevron-right" size="md" tone="faint" /> : null}
    </>
  );
  const label_ = [label, value, detail].filter(Boolean).join(', ');
  if (onPress) {
    return (
      <Tap testID={testID} accessibilityLabel={label_} onPress={onPress} scales={false} haptic="selection" style={[styles.row, divided ? styles.divided : null]}>
        {content}
      </Tap>
    );
  }
  return (
    <View testID={testID} accessible accessibilityLabel={label_} style={[styles.row, divided ? styles.divided : null]}>
      {content}
    </View>
  );
}

export interface SwitchRowProps {
  readonly testID: string;
  readonly label: string;
  readonly detail?: string;
  readonly value: boolean;
  readonly onChange: (value: boolean) => void;
  readonly disabled?: boolean;
  readonly divided?: boolean;
}

/** A row with the system's own switch. */
export function SwitchRow({ testID, label, detail, value, onChange, disabled, divided }: SwitchRowProps) {
  const styles = useStyles();
  const theme = useTheme();
  const { haptics } = useCapabilities();
  return (
    <View style={[styles.row, divided ? styles.divided : null]}>
      <View style={styles.texts}>
        <Txt kind="body">{label}</Txt>
        {detail ? (
          <Txt kind="small" tone="muted">
            {detail}
          </Txt>
        ) : null}
      </View>
      <Switch
        testID={testID}
        accessibilityLabel={label}
        value={value}
        disabled={disabled}
        onValueChange={(next) => {
          haptics.play('selection');
          onChange(next);
        }}
        trackColor={{ false: theme.colors.borderStrong, true: theme.colors.accentStrong }}
        thumbColor={theme.colors.onAccent}
        ios_backgroundColor={theme.colors.borderStrong}
      />
    </View>
  );
}

export interface EmptyProps {
  readonly testID: string;
  readonly text: string;
  readonly icon?: IconName;
  readonly action?: { readonly testID: string; readonly label: string; readonly onPress: () => void };
}

/** A screen with nothing to show yet: one sentence and, where there is one, the next step. */
export function Empty({ testID, text, icon, action }: EmptyProps) {
  const styles = useStyles();
  return (
    <View testID={testID} style={styles.empty}>
      {icon ? <Icon name={icon} size="xl" tone="faint" /> : null}
      <Txt kind="body" tone="muted" style={{ textAlign: 'center' }}>
        {text}
      </Txt>
      {action ? <Button testID={action.testID} label={action.label} kind="primary" onPress={action.onPress} /> : null}
    </View>
  );
}
