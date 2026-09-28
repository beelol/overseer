import { makeStyles, Txt, type TxtTone } from '@/ui';

const useStyles = makeStyles((theme) => ({
  note: { paddingHorizontal: theme.space[4], paddingTop: theme.space[2] },
}));

export interface NoteProps {
  readonly testID: string;
  readonly text: string;
  readonly tone?: TxtTone;
  /** True for what has just happened: it is said aloud when it appears. */
  readonly alert?: boolean;
}

/** A sentence under a section's card, with a name for tests. */
export function Note({ testID, text, tone = 'muted', alert }: NoteProps) {
  const styles = useStyles();
  return (
    <Txt
      testID={testID}
      kind="small"
      tone={tone}
      style={styles.note}
      {...(alert
        ? { accessibilityRole: 'alert' as const, accessibilityLiveRegion: 'polite' as const }
        : {})}
    >
      {text}
    </Txt>
  );
}
