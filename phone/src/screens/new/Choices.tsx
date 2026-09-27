import { useState, type ReactNode } from 'react';
import { ScrollView, TextInput, useWindowDimensions, View } from 'react-native';

import { Tap } from '@/motion';
import { lineHeight, useTheme } from '@/theme';
import { Button, Icon, Logo, makeStyles, MAX_TEXT_SCALE, Sheet, Txt, type IconName } from '@/ui';

export interface Choice {
  /** The end of the row's test id: `<sheet>.<id>`. */
  readonly id: string;
  readonly label: string;
  /** A quiet second line. */
  readonly detail?: string;
  /** A provider's logo by the harness's name, where the choice is an agent. */
  readonly logo?: string;
  readonly icon?: IconName;
  readonly selected: boolean;
  readonly onChoose: () => void;
}

export interface ChoicesProps {
  /** The start of the rows' test ids. */
  readonly testID: string;
  readonly open: boolean;
  readonly onClose: () => void;
  readonly title: string;
  /** Said when there is nothing to choose. */
  readonly empty?: string;
  readonly choices: readonly Choice[];
  /** A value that can be typed where the list does not hold it (another model). */
  readonly other?: { readonly label: string; readonly value: string; readonly action: string; readonly onChoose: (value: string) => void };
  readonly children?: ReactNode;
}

const useStyles = makeStyles((theme) => ({
  row: { minHeight: theme.phone.size.touch, paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], flexDirection: 'row', alignItems: 'center', gap: theme.space[3] },
  divided: { borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
  texts: { flex: 1, gap: theme.space[1] },
  empty: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[4] },
  other: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[3], gap: theme.space[2], borderTopWidth: theme.phone.size.hairline, borderTopColor: theme.colors.border },
  typed: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2] },
  input: { flex: 1, minHeight: theme.phone.size.touch, paddingHorizontal: theme.space[3], paddingVertical: theme.space[2], borderRadius: theme.radius.control, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong, backgroundColor: theme.colors.bg, color: theme.colors.text, fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.tight) },
}));

/** The most of the screen's height a list of choices takes before it scrolls. */
const MOST = 0.6;

/**
 * A sheet of choices with the chosen one marked. Choosing closes it. A long list scrolls
 * inside the sheet, so the last choice is reached on a small phone too.
 */
export function Choices({ testID, open, onClose, title, empty, choices, other, children }: ChoicesProps) {
  const styles = useStyles();
  const { height } = useWindowDimensions();
  return (
    <Sheet testID={testID} open={open} onClose={onClose} title={title}>
      <ScrollView style={{ maxHeight: height * MOST }} keyboardShouldPersistTaps="handled">
        {choices.length === 0 && empty ? (
          <Txt testID={`${testID}.empty`} kind="label" tone="muted" style={styles.empty}>
            {empty}
          </Txt>
        ) : null}
        {choices.map((choice, at) => (
          <Tap
            key={choice.id}
            testID={`${testID}.${choice.id}`}
            accessibilityLabel={[choice.label, choice.detail].filter(Boolean).join(', ')}
            accessibilityState={{ selected: choice.selected }}
            haptic="selection"
            scales={false}
            onPress={() => {
              onClose();
              choice.onChoose();
            }}
            style={[styles.row, at > 0 ? styles.divided : null]}
          >
            {choice.logo ? <Logo name={choice.logo} size="header" /> : choice.icon ? <Icon name={choice.icon} size="lg" tone="muted" /> : null}
            <View style={styles.texts}>
              <Txt kind="body">{choice.label}</Txt>
              {choice.detail ? (
                <Txt kind="small" tone="muted">
                  {choice.detail}
                </Txt>
              ) : null}
            </View>
            {choice.selected ? <Icon name="check" size="md" tone="accent" /> : null}
          </Tap>
        ))}
        {other ? <Other key={String(open)} testID={testID} other={other} onClose={onClose} /> : null}
        {children}
      </ScrollView>
    </Sheet>
  );
}

function Other({ testID, other, onClose }: { readonly testID: string; readonly other: NonNullable<ChoicesProps['other']>; readonly onClose: () => void }) {
  const styles = useStyles();
  const theme = useTheme();
  const [typed, setTyped] = useState(other.value);
  const choose = (): void => {
    const value = typed.trim();
    if (!value) return;
    onClose();
    other.onChoose(value);
  };
  return (
    <View style={styles.other}>
      <Txt kind="small" tone="muted">
        {other.label}
      </Txt>
      <View style={styles.typed}>
        <TextInput
          testID={`${testID}.other`}
          accessibilityLabel={other.label}
          value={typed}
          onChangeText={setTyped}
          onSubmitEditing={choose}
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="done"
          maxFontSizeMultiplier={MAX_TEXT_SCALE}
          placeholderTextColor={theme.colors.faint}
          selectionColor={theme.colors.accent}
          keyboardAppearance={theme.scheme}
          style={styles.input}
        />
        <Button testID={`${testID}.other.use`} label={other.action} accessibilityLabel={`${other.action}, ${other.label}`} disabled={!typed.trim()} onPress={choose} />
      </View>
    </View>
  );
}
