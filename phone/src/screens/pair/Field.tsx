import { TextInput, View, type TextInputProps } from 'react-native';

import { lineHeight, useTheme } from '@/theme';
import { makeStyles, MAX_TEXT_SCALE, Txt } from '@/ui';

export interface FieldProps extends Omit<
  TextInputProps,
  'style' | 'testID' | 'accessibilityLabel' | 'onChangeText' | 'onChange' | 'value'
> {
  readonly testID: string;
  /** Shown above the field and said by VoiceOver and TalkBack. */
  readonly label: string;
  readonly value: string;
  readonly onChange: (text: string) => void;
}

const useStyles = makeStyles((theme) => ({
  field: { gap: theme.space[1] },
  input: {
    minHeight: theme.phone.size.touch,
    paddingHorizontal: theme.space[3],
    paddingVertical: theme.space[2],
    borderRadius: theme.radius.control,
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.borderStrong,
    backgroundColor: theme.colors.raised,
    color: theme.colors.text,
    fontSize: theme.font.xl,
    lineHeight: lineHeight(theme.font.xl, theme.line.tight),
  },
}));

/** A labelled text field. It follows the system's text size like the app's text does. */
export function Field({ testID, label, value, onChange, ...rest }: FieldProps) {
  const styles = useStyles();
  const theme = useTheme();
  return (
    <View style={styles.field}>
      <Txt kind="small" tone="muted">
        {label}
      </Txt>
      <TextInput
        {...rest}
        testID={testID}
        accessibilityLabel={label}
        value={value}
        onChangeText={onChange}
        maxFontSizeMultiplier={MAX_TEXT_SCALE}
        placeholderTextColor={theme.colors.muted}
        selectionColor={theme.colors.accent}
        keyboardAppearance={theme.scheme}
        style={styles.input}
      />
    </View>
  );
}
