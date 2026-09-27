import { TextInput } from 'react-native';

import { lineHeight, useTheme } from '@/theme';
import { makeStyles, MAX_TEXT_SCALE } from '@/ui';

import { WORDS } from './words';

export interface TaskFieldProps {
  readonly value: string;
  readonly onChange: (task: string) => void;
  readonly editable: boolean;
}

const useStyles = makeStyles((theme) => ({
  input: {
    minHeight: theme.phone.size.touch * 3,
    paddingHorizontal: theme.space[4],
    paddingVertical: theme.space[3],
    color: theme.colors.text,
    fontSize: theme.font.xl,
    lineHeight: lineHeight(theme.font.xl, theme.line.body),
    textAlignVertical: 'top',
  },
}));

/** What the agent is asked to do: several lines, growing with what is typed. */
export function TaskField({ value, onChange, editable }: TaskFieldProps) {
  const styles = useStyles();
  const theme = useTheme();
  return (
    <TextInput
      testID="new.task"
      accessibilityLabel={WORDS.task}
      value={value}
      onChangeText={onChange}
      editable={editable}
      multiline
      placeholder={WORDS.taskHint}
      maxFontSizeMultiplier={MAX_TEXT_SCALE}
      placeholderTextColor={theme.colors.faint}
      selectionColor={theme.colors.accent}
      keyboardAppearance={theme.scheme}
      style={styles.input}
    />
  );
}
