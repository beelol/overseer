import { memo, useEffect, useState } from 'react';
import { View } from 'react-native';
import Animated, { cancelAnimation, runOnJS, useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';

import type { agents } from '@/model';
import { useMotion } from '@/motion';
import { makeStyles, Txt } from '@/ui';

type Tone = NonNullable<agents.AgentRow['badgeTone']>;

export interface StatusLineProps {
  /** The row this line belongs to. Rows of a list are used again for other agents. */
  readonly id: string;
  readonly testID: string;
  /** The repository's name, where the row shows one. */
  readonly repo: string | null;
  /** The status in words: "working", "needs you", "done". */
  readonly words: string | null;
  readonly tone: Tone | null;
  /** How long ago, or why the agent needs the owner. */
  readonly detail: string;
}

interface Shown {
  readonly id: string;
  readonly repo: string | null;
  readonly words: string | null;
  readonly tone: Tone | null;
  readonly detail: string;
}

const useStyles = makeStyles((theme) => ({
  over: { position: 'absolute', top: 0, left: 0, right: 0 },
  blue: { color: theme.colors.blue },
  yellow: { color: theme.colors.amber },
  orange: { color: theme.colors.amber },
  green: { color: theme.colors.green },
  red: { color: theme.colors.red },
  purple: { color: theme.colors.accent },
  quiet: { color: theme.colors.muted },
}));

const SEPARATOR = ' · ';

function Line({ testID, shown }: { readonly testID?: string; readonly shown: Shown }) {
  const styles = useStyles();
  const before = shown.repo && shown.words ? shown.repo + SEPARATOR : (shown.repo ?? '');
  const after = shown.detail ? (shown.repo || shown.words ? SEPARATOR : '') + shown.detail : '';
  return (
    <Txt testID={testID} kind="small" tone="muted" numberOfLines={2}>
      {before}
      {shown.words ? (
        <Txt kind="small" tone="muted" style={shown.tone ? styles[shown.tone] : null}>
          {shown.words}
        </Txt>
      ) : null}
      {after}
    </Txt>
  );
}

/**
 * The quiet line under an agent's title: repository, status in words, time. When the status
 * changes, the old words and their colour fade out while the new ones fade in; nothing moves,
 * so it is the same with Reduce Motion.
 */
export const StatusLine = memo(function StatusLine({ id, testID, repo, words, tone, detail }: StatusLineProps) {
  const styles = useStyles();
  const motion = useMotion();
  const [last, setLast] = useState<Shown>({ id, repo, words, tone, detail });
  const [leaving, setLeaving] = useState<Shown | null>(null);
  const arrived = useSharedValue(1);

  if (last.id !== id || last.words !== words || last.tone !== tone || last.repo !== repo || last.detail !== detail) {
    // Another agent took this row, or only the time moved on: nothing fades.
    const changed = last.id === id && (last.words !== words || last.tone !== tone);
    setLast({ id, repo, words, tone, detail });
    setLeaving(changed ? last : null);
  }

  useEffect(() => {
    if (leaving === null) {
      arrived.set(1);
      return undefined;
    }
    arrived.set(0);
    arrived.set(
      withTiming(1, motion.timing(motion.tokens.status.change), (finished) => {
        'worklet';
        if (finished) runOnJS(setLeaving)(null);
      }),
    );
    return () => cancelAnimation(arrived);
  }, [leaving, arrived, motion]);

  const coming = useAnimatedStyle(() => ({ opacity: arrived.value }));
  const going = useAnimatedStyle(() => ({ opacity: 1 - arrived.value }));

  return (
    <View>
      <Animated.View style={coming}>
        <Line testID={testID} shown={{ id, repo, words, tone, detail }} />
      </Animated.View>
      {leaving ? (
        <Animated.View style={[styles.over, going]} pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
          <Line shown={leaving} />
        </Animated.View>
      ) : null}
    </View>
  );
});
