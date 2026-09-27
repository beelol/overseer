import { useMemo, type ReactNode } from 'react';
import { View } from 'react-native';

import { Arrive } from '@/motion';
import { useTheme } from '@/theme';
import { makeStyles } from '@/ui';

const useStyles = makeStyles((theme) => ({
  frame: { paddingRight: theme.chat.gutterNarrow, paddingVertical: theme.chat.blockGap / 2 },
  turn: { paddingTop: theme.chat.turnGap - theme.chat.blockGap / 2 },
  line: { position: 'absolute', top: 0, bottom: 0, width: theme.phone.size.hairline * 2, backgroundColor: theme.colors.borderStrong },
}));

export interface FrameProps {
  /** The row's test id: the frame is `<id>.frame`, a line `<id>.line.<depth>`. */
  readonly id: string;
  readonly depth: number;
  /** The depths of the children that hold the row, outermost last: a line is drawn for each. */
  readonly lines: readonly number[] | undefined;
  /** True for a row that came live: it arrives. A row of the loaded history is drawn in place. */
  readonly arriving: boolean;
  /** True for the first row of a turn: more room above it. */
  readonly turn?: boolean;
  readonly children: ReactNode;
}

/** Where a row sits: its depth, the lines of the children that hold it, and how it comes in. */
export function Frame({ id, depth, lines, arriving, turn, children }: FrameProps) {
  const styles = useStyles();
  const theme = useTheme();
  const step = theme.space[4];
  const gutter = theme.chat.gutterNarrow;
  const indent = useMemo(() => ({ paddingLeft: gutter + depth * step }), [gutter, depth, step]);
  const drawn = (
    <>
      {lines?.map((at) => (
        <View key={at} testID={`${id}.line.${at}`} pointerEvents="none" style={[styles.line, { left: gutter + at * step - theme.space[2] }]} />
      ))}
      {children}
    </>
  );
  const style = [styles.frame, turn ? styles.turn : null, indent];
  return arriving ? (
    <Arrive style={style}>{drawn}</Arrive>
  ) : (
    <View testID={`${id}.frame`} style={style}>
      {drawn}
    </View>
  );
}
