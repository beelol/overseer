import { TextInput, View } from 'react-native';

import { useTheme } from '@/theme';
import { Icon, IconButton, makeStyles, MAX_TEXT_SCALE, Txt, type IconName } from '@/ui';

export interface FieldProps {
  readonly testID: string;
  /** What VoiceOver and TalkBack say, and what stands above the field when `titled`. */
  readonly label: string;
  readonly value: string;
  readonly onChangeText: (value: string) => void;
  readonly placeholder?: string;
  readonly icon?: IconName;
  /** True shows the label above the field. */
  readonly titled?: boolean;
  /** Several lines, growing with the text. */
  readonly multiline?: boolean;
  /** What clearing the field is called; given, a filled field shows a way to empty it. */
  readonly clear?: string;
  /** A quiet sentence under the field. */
  readonly hint?: string;
}

const useStyles = makeStyles((theme) => ({
  wrap: { gap: theme.space[1] },
  box: {
    minHeight: theme.phone.size.touch,
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.space[2],
    paddingLeft: theme.space[3],
    borderRadius: theme.radius.control,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.borderStrong,
    backgroundColor: theme.colors.raised,
  },
  input: { flex: 1, paddingVertical: theme.space[2], paddingRight: theme.space[3], color: theme.colors.text, fontSize: theme.font.xl },
  many: { minHeight: theme.phone.size.touch * 2, textAlignVertical: 'top' },
}));

/**
 * A field to type in. The shared pieces (`src/ui`) have none yet; this one is built from the
 * same tokens and belongs there.
 */
export function Field({ testID, label, value, onChangeText, placeholder, icon, titled, multiline, clear, hint }: FieldProps) {
  const styles = useStyles();
  const theme = useTheme();
  return (
    <View style={styles.wrap}>
      {titled ? (
        <Txt kind="small" tone="muted">
          {label}
        </Txt>
      ) : null}
      <View style={styles.box}>
        {icon ? <Icon name={icon} size="md" tone="muted" /> : null}
        <TextInput
          testID={testID}
          accessibilityLabel={label}
          value={value}
          onChangeText={onChangeText}
          placeholder={placeholder}
          placeholderTextColor={theme.colors.muted}
          selectionColor={theme.colors.accent}
          keyboardAppearance={theme.scheme}
          maxFontSizeMultiplier={MAX_TEXT_SCALE}
          autoCapitalize={multiline ? 'sentences' : 'none'}
          autoCorrect={Boolean(multiline)}
          multiline={multiline}
          style={[styles.input, multiline ? styles.many : null]}
        />
        {clear && value ? <IconButton testID={`${testID}.clear`} accessibilityLabel={clear} icon="close" tone="muted" onPress={() => onChangeText('')} /> : null}
      </View>
      {hint ? (
        <Txt kind="small" tone="muted">
          {hint}
        </Txt>
      ) : null}
    </View>
  );
}
