import { faded } from '@/theme';
import { memo, useMemo } from 'react';
import { Animated, View } from 'react-native';

import { review, text } from '@/model';
import { Actions, Button, Icon, makeStyles, Txt } from '@/ui';

import { cut, tokenize, type Language, type Token } from './syntax';
import { WORDS } from './words';

const useStyles = makeStyles((theme) => ({
  heading: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2], paddingHorizontal: theme.space[4], paddingVertical: theme.space[2], backgroundColor: theme.colors.raised, borderTopWidth: theme.phone.size.hairline, borderBottomWidth: theme.phone.size.hairline, borderColor: theme.colors.border },
  where: { flexGrow: 1, flexShrink: 1, gap: theme.space[1] },
  counts: { flexDirection: 'row', alignItems: 'center', gap: theme.space[2] },
  line: { flexDirection: 'row', alignItems: 'stretch' },
  // The tint of a changed line is the theme's, drawn at half strength: on a phone the code sits
  // on it in small type, and every syntax colour keeps its contrast (the contrast test).
  removed: { backgroundColor: faded(theme.colors.removedBg, theme.phone.opacity.diffTint) },
  added: { backgroundColor: faded(theme.colors.addedBg, theme.phone.opacity.diffTint) },
  gutter: { flexDirection: 'row', justifyContent: 'flex-end', paddingHorizontal: theme.space[2] },
  gutterRemoved: { backgroundColor: theme.colors.removedLine },
  gutterAdded: { backgroundColor: theme.colors.addedLine },
  // On the gutter's stronger tint the number takes the text's colour.
  number: { color: theme.colors.text, textAlign: 'right' },
  sign: { paddingHorizontal: theme.space[1], color: theme.colors.muted },
  code: { paddingRight: theme.space[3] },
  wrapped: { flex: 1 },
  plain: { color: theme.colors.syntax.variable },
  comment: { color: theme.colors.syntax.comment },
  string: { color: theme.colors.syntax.string },
  // Named apart from `number`, the line number's style.
  numeral: { color: theme.colors.syntax.number },
  keyword: { color: theme.colors.syntax.keyword },
}));

/** "Lines 12 to 14", or where a hunk that only removes lines sits. */
export function whereOf(hunk: review.HunkView): string {
  return hunk.added > 0 ? WORDS.file.lines(hunk.modifiedStart, hunk.modifiedStart + hunk.added - 1) : WORDS.file.deletionAfter(hunk.modifiedStart);
}

export interface HunkHeadingProps {
  readonly hunk: review.HunkView;
  /** False on a phone that may only watch: the heading has no controls. */
  readonly controls: boolean;
  readonly onAccept: (hunk: review.HunkView) => void;
  readonly onReject: (hunk: review.HunkView) => void;
  /** Given while long lines scroll sideways: the heading stays in sight, this wide. */
  readonly held?: { readonly shift: Animated.Value; readonly width: number };
}

/** The head of a hunk: where it is, how much it changes, Accept and Reject. */
export const HunkHeading = memo(function HunkHeading({ hunk, controls, onAccept, onReject, held }: HunkHeadingProps) {
  const styles = useStyles();
  return (
    <Animated.View testID={`file.hunk.${hunk.key}`} style={[styles.heading, held ? { width: held.width, transform: [{ translateX: held.shift }] } : null]}>
      <View style={styles.where}>
        <Txt testID={`file.hunk.${hunk.key}.where`} kind="label" accessibilityRole="header" accessibilityLabel={hunk.label}>
          {whereOf(hunk)}
        </Txt>
        <View style={styles.counts}>
          <Txt kind="small" tone="green">
            {text.TEXT.conversation.added(hunk.added)}
          </Txt>
          <Txt kind="small" tone="red">
            {text.TEXT.conversation.removed(hunk.removed)}
          </Txt>
          {hunk.reviewed && !controls ? (
            <>
              <Icon name="check" size="sm" tone="green" />
              <Txt testID={`file.hunk.${hunk.key}.reviewed`} kind="small" tone="green">
                {text.TEXT.review.reviewed}
              </Txt>
            </>
          ) : null}
        </View>
      </View>
      {controls ? (
        <Actions>
          <Button
            testID="file.hunk.accept"
            label={hunk.reviewed ? text.TEXT.review.reviewed : WORDS.file.accept}
            accessibilityLabel={hunk.accept.label}
            accessibilityState={{ selected: hunk.reviewed }}
            kind={hunk.reviewed ? 'secondary' : 'primary'}
            {...(hunk.reviewed ? { icon: 'check' as const } : {})}
            haptic="confirm"
            onPress={() => onAccept(hunk)}
          />
          <Button testID="file.hunk.reject" label={WORDS.file.reject} accessibilityLabel={hunk.reject.label} kind="danger" haptic="warning" onPress={() => onReject(hunk)} />
        </Actions>
      ) : null}
    </Animated.View>
  );
});

export interface DiffLineProps {
  readonly row: review.DiffRow;
  /** The line as shown: tabs as spaces, cut when it is very long. */
  readonly text: string;
  /** How many characters were cut from its end. */
  readonly more: number;
  readonly language: Language | null;
  /** How many characters fit across when long lines wrap; `null` while they scroll sideways. */
  readonly columns: number | null;
  /** How wide the line numbers are. */
  readonly gutter: number;
}

function Pieces({ tokens }: { readonly tokens: readonly Token[] }) {
  const styles = useStyles();
  if (tokens.length === 0) return ' ';
  return tokens.map((token, index) => (
    <Txt key={index} kind="mono" style={styles[token.kind === 'number' ? 'numeral' : token.kind]}>
      {token.text}
    </Txt>
  ));
}

/** One removed or added line: its number, its sign and its text in the colours of the theme. */
export const DiffLine = memo(function DiffLine({ row, text: line, more, language, columns, gutter }: DiffLineProps) {
  const styles = useStyles();
  const removed = row.kind === 'removed';
  const pieces = useMemo(() => {
    const tokens = tokenize(line, language);
    return columns === null ? [tokens] : cut(tokens, review.splitLine(line, columns));
  }, [line, language, columns]);
  const number = removed ? row.baseLine : row.modifiedLine;
  return (
    <View testID={`file.line.${row.key}`} accessible accessibilityLabel={removed ? WORDS.file.removedLine(number, line) : WORDS.file.addedLine(number, line)} style={[styles.line, removed ? styles.removed : styles.added]}>
      <View style={[styles.gutter, removed ? styles.gutterRemoved : styles.gutterAdded, { width: gutter }]}>
        <Txt kind="mono" style={styles.number} numberOfLines={1}>
          {number ?? ''}
        </Txt>
      </View>
      <Txt kind="mono" style={styles.sign}>
        {removed ? '−' : '+'}
      </Txt>
      <View style={[styles.code, columns === null ? null : styles.wrapped]}>
        {pieces.map((tokens, index) => (
          <Txt key={index} testID={index === 0 ? `file.line.${row.key}.text` : undefined} kind="mono" style={styles.plain}>
            <Pieces tokens={tokens} />
          </Txt>
        ))}
        {more > 0 ? (
          <Txt kind="small" tone="muted">
            {WORDS.file.more(more)}
          </Txt>
        ) : null}
      </View>
    </View>
  );
});
