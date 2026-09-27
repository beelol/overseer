import { Arrive } from '@/motion';
import { makeStyles, Txt, type TxtTone } from '@/ui';

const useStyles = makeStyles((theme) => ({
  line: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[2], backgroundColor: theme.colors.raised, borderBottomWidth: theme.phone.size.hairline, borderBottomColor: theme.colors.border },
}));

/** One calm sentence across the screen: what the Mac refused, or how old what is shown is. */
export function Notice({ testID, text, tone = 'muted' }: { readonly testID: string; readonly text: string; readonly tone?: TxtTone }) {
  const styles = useStyles();
  return (
    <Arrive from="above" style={styles.line}>
      <Txt testID={testID} kind="label" tone={tone} accessibilityRole="alert" accessibilityLiveRegion="polite">
        {text}
      </Txt>
    </Arrive>
  );
}
