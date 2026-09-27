import { memo } from 'react';
import { View } from 'react-native';

import { text, type agents } from '@/model';
import { Chip, makeStyles } from '@/ui';

export interface FiltersProps {
  readonly filter: agents.AgentFilter;
  readonly onChange: (filter: agents.AgentFilter) => void;
  /** How many agents need the owner. */
  readonly needs: number;
}

const useStyles = makeStyles((theme) => ({
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.space[2], paddingHorizontal: theme.space[4], paddingVertical: theme.space[2] },
}));

const FILTERS: readonly agents.AgentFilter[] = ['all', 'active', 'needs'];

/** All, Active, Needs you with its count. */
export const Filters = memo(function Filters({ filter, onChange, needs }: FiltersProps) {
  const styles = useStyles();
  return (
    <View style={styles.row} accessibilityRole="tablist">
      {FILTERS.map((name) => (
        <Chip
          key={name}
          testID={`agents.filter.${name}`}
          label={text.PHONE_ONLY.filter[name]}
          selected={filter === name}
          haptic="selection"
          onPress={() => onChange(name)}
          {...(name === 'needs' ? { count: needs, accessibilityLabel: text.TEXT.agents.needsYouCount(needs) } : {})}
        />
      ))}
    </View>
  );
});
