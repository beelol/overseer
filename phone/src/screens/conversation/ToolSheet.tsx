import { useMemo } from 'react';
import { ScrollView, useWindowDimensions, View } from 'react-native';

import { conversation } from '@/model';
import { makeStyles, Sheet, Txt } from '@/ui';

const useStyles = makeStyles((theme) => ({
  body: { paddingHorizontal: theme.space[4], paddingBottom: theme.space[4], gap: theme.space[2] },
  box: { paddingHorizontal: theme.space[3], paddingVertical: theme.space[2], borderRadius: theme.radius.control, backgroundColor: theme.colors.raised2 },
}));

/** A sheet may take this much of the screen's height; what is longer scrolls. */
const TALLEST = 0.6;

export interface ToolSheetProps {
  /** The tool call to show, or `null` while the sheet is closed. */
  readonly row: conversation.ToolRow | null;
  readonly onClose: () => void;
}

/** What a tool call was given and what it returned, as VS Code shows it when a row is opened. */
export function ToolSheet({ row, onClose }: ToolSheetProps) {
  const styles = useStyles();
  const { height } = useWindowDimensions();
  const detail = useMemo(() => (row ? conversation.toolDetail(row) : null), [row]);
  const limit = useMemo(() => ({ maxHeight: Math.round(height * TALLEST) }), [height]);
  return (
    <Sheet testID="agent.tool" open={row !== null} onClose={onClose} {...(row ? { title: `${row.verb} ${row.target}`.trim() } : {})}>
      <ScrollView style={limit} contentContainerStyle={styles.body}>
        {row?.full && row.full !== detail?.input?.text ? (
          <Txt testID="agent.tool.full" kind="small" tone="muted" selectable>
            {row.full}
          </Txt>
        ) : null}
        {detail?.input ? (
          <>
            <Txt kind="small" tone="muted">
              {detail.input.label}
            </Txt>
            <View style={styles.box}>
              <Txt testID="agent.tool.input" kind="mono" selectable>
                {detail.input.text}
              </Txt>
            </View>
          </>
        ) : null}
        {detail?.output ? (
          <>
            <Txt kind="small" tone="muted">
              {detail.output.label}
            </Txt>
            <View style={styles.box}>
              <Txt testID="agent.tool.output" kind="mono" tone={detail.output.error ? 'red' : 'text'} selectable>
                {detail.output.text}
              </Txt>
            </View>
          </>
        ) : null}
        {detail?.note ? (
          <Txt testID="agent.tool.note" kind="small" tone="muted">
            {detail.note}
          </Txt>
        ) : null}
      </ScrollView>
    </Sheet>
  );
}
