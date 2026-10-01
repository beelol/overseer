import { TextInput, View } from 'react-native';

import { text } from '@/model';
import { Arrive } from '@/motion';
import { lineHeight, useTheme } from '@/theme';
import { Icon, IconButton, makeStyles, MAX_TEXT_SCALE, Txt } from '@/ui';

export interface SearchFieldProps {
  readonly query: string;
  readonly onChange: (query: string) => void;
  /** Clears what was typed and puts the field away. */
  readonly onClose: () => void;
  /** How many agents match; `null` while nothing is typed. */
  readonly matches: number | null;
}

const useStyles = makeStyles((theme) => ({
  bar: { paddingHorizontal: theme.space[4], paddingTop: theme.space[2], gap: theme.space[1] },
  field: { minHeight: theme.phone.size.touch, flexDirection: 'row', alignItems: 'center', gap: theme.space[2], paddingLeft: theme.space[3], borderRadius: theme.radius.card, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong, backgroundColor: theme.colors.raised },
  input: { flex: 1, paddingVertical: theme.space[2], color: theme.colors.text, fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.tight) },
}));

/** The search of the agents list. What is typed filters the list at once. */
export function SearchField({ query, onChange, onClose, matches }: SearchFieldProps) {
  const styles = useStyles();
  const theme = useTheme();
  const words = text.TEXT.agents;
  return (
    <Arrive from="above" style={styles.bar}>
      <View style={styles.field}>
        <Icon name="search" size="md" tone="faint" />
        <TextInput
          testID="agents.search.field"
          accessibilityLabel={words.search}
          value={query}
          onChangeText={onChange}
          placeholder={words.searchHint}
          autoFocus
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="search"
          maxFontSizeMultiplier={MAX_TEXT_SCALE}
          placeholderTextColor={theme.colors.muted}
          selectionColor={theme.colors.accent}
          keyboardAppearance={theme.scheme}
          style={styles.input}
        />
        <IconButton testID="agents.search.clear" accessibilityLabel={words.clearSearch} icon="close" tone="muted" onPress={onClose} />
      </View>
      {matches !== null ? (
        <Txt testID="agents.search.matches" kind="small" tone="muted" accessibilityLiveRegion="polite">
          {words.matches(matches)}
        </Txt>
      ) : null}
    </Arrive>
  );
}
