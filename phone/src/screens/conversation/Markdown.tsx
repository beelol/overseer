import * as Clipboard from 'expo-clipboard';
import * as Linking from 'expo-linking';
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { ScrollView, Text, View, type TextProps, type TextStyle } from 'react-native';

import type { markdown } from '@/model';
import { Tap } from '@/motion';
import { useCapabilities } from '@/platform';
import { lineHeight, useTheme } from '@/theme';
import { Icon, makeStyles, MAX_TEXT_SCALE, Txt, weight, type TxtTone } from '@/ui';

import { useOpen } from './open';
import { WORDS } from './words';

/** How much of a long code block shows until it is opened. */
export const FOLDED_LINES = 12;
const COPIED_FOR_MS = 1200;
/** The width of a table's column, in characters of its longest cell. */
const NARROWEST = 4;
const WIDEST = 32;
/** A character of the label size is about this much of its height wide. */
const CHARACTER = 0.62;

const useStyles = makeStyles((theme) => {
  const body = lineHeight(theme.font.xl, theme.line.body);
  const hairline = theme.phone.size.hairline;
  return {
    blocks: { gap: theme.space[2] },
    h1: { fontSize: theme.font.xxl, lineHeight: lineHeight(theme.font.xxl, theme.line.tight), fontWeight: weight(theme.weight.semibold), marginTop: theme.space[2] },
    h2: { fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.tight), fontWeight: weight(theme.weight.semibold), marginTop: theme.space[2] },
    h3: { fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.tight), fontWeight: weight(theme.weight.medium), marginTop: theme.space[1] },
    strong: { fontWeight: weight(theme.weight.semibold) },
    emphasis: { fontStyle: 'italic' },
    strike: { textDecorationLine: 'line-through' },
    code: { fontFamily: 'Menlo', fontSize: theme.font.lg, lineHeight: body, backgroundColor: theme.colors.raised2 },
    link: { color: theme.colors.link },
    long: { fontFamily: 'Menlo', fontSize: theme.font.lg, lineHeight: body, backgroundColor: theme.colors.raised2 },
    list: { gap: theme.space[1] },
    item: { flexDirection: 'row', gap: theme.space[2] },
    marker: { minWidth: theme.space[5], textAlign: 'right' },
    itemBody: { flex: 1, gap: theme.space[1] },
    box: { width: theme.phone.size.icon.md, height: theme.phone.size.icon.md, marginTop: theme.space[1], borderRadius: theme.space[1], borderWidth: hairline, borderColor: theme.colors.borderStrong, alignItems: 'center', justifyContent: 'center' },
    boxChecked: { backgroundColor: theme.colors.accentStrong, borderColor: theme.colors.accentStrong },
    quote: { borderLeftWidth: hairline * 2, borderLeftColor: theme.colors.borderStrong, paddingLeft: theme.space[3], gap: theme.space[2] },
    rule: { height: hairline, backgroundColor: theme.colors.border, marginVertical: theme.space[3] },
    table: { alignSelf: 'flex-start', borderWidth: hairline, borderColor: theme.colors.border, borderRadius: theme.radius.control, overflow: 'hidden' },
    tableHead: { flexDirection: 'row', backgroundColor: theme.colors.raised },
    tableRow: { flexDirection: 'row', borderTopWidth: hairline, borderTopColor: theme.colors.border },
    cell: { paddingHorizontal: theme.space[3], paddingVertical: theme.space[2] },
    codeblock: { borderRadius: theme.radius.control, borderWidth: hairline, borderColor: theme.colors.border, backgroundColor: theme.colors.raised, overflow: 'hidden' },
    codeHead: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingLeft: theme.space[3], minHeight: theme.space[10] },
    copy: { minWidth: theme.phone.size.touch, minHeight: theme.space[10], alignItems: 'center', justifyContent: 'center' },
    codeBody: { paddingHorizontal: theme.space[3], paddingBottom: theme.space[3] },
    more: { minHeight: theme.phone.size.touch, alignItems: 'center', justifyContent: 'center', borderTopWidth: hairline, borderTopColor: theme.colors.border },
  };
});

type Styles = ReturnType<typeof useStyles>;

/** Text inside text: it takes the size and colour of what holds it, and follows the system's text size as `Txt` does. */
function Span(props: TextProps) {
  return <Text maxFontSizeMultiplier={MAX_TEXT_SCALE} {...props} />;
}

export interface MarkdownProps {
  /** The start of the test ids of what can be pressed inside: links, long tokens, Copy. */
  readonly id: string;
  readonly blocks: readonly (markdown.Block | markdown.InlineRun)[];
  /** `muted` for a thought and inside a quote. */
  readonly tone?: TxtTone;
}

/**
 * An agent's reply as views: the tree of `markdown.parse`, drawn with the app's own pieces.
 * Nothing of the reply is run or loaded; a link opens only where VS Code would open it.
 */
export const Markdown = memo(function Markdown({ id, blocks, tone = 'text' }: MarkdownProps) {
  const styles = useStyles();
  return (
    <View style={styles.blocks}>
      {blocks.map((block, index) => (
        <BlockView key={index} id={`${id}.${index}`} block={block} tone={tone} styles={styles} />
      ))}
    </View>
  );
});

interface BlockProps {
  readonly id: string;
  readonly block: markdown.Block | markdown.InlineRun;
  readonly tone: TxtTone;
  readonly styles: Styles;
}

function BlockView({ id, block, tone, styles }: BlockProps) {
  switch (block.type) {
    case 'paragraph':
    case 'inline':
      return (
        <Txt kind="body" tone={tone}>
          {inlines(block.children, id, styles)}
        </Txt>
      );
    case 'heading':
      return (
        <Txt kind="body" tone={tone} accessibilityRole="header" style={block.level === 1 ? styles.h1 : block.level === 2 ? styles.h2 : styles.h3}>
          {inlines(block.children, id, styles)}
        </Txt>
      );
    case 'list':
      return (
        <View style={styles.list}>
          {block.items.map((item, index) => (
            <View key={index} style={styles.item}>
              {item.checked === null ? (
                <Txt kind="body" tone="muted" style={styles.marker}>
                  {block.ordered ? `${block.start + index}.` : '•'}
                </Txt>
              ) : (
                <View testID={`${id}.${index}.task`} accessible accessibilityRole="checkbox" accessibilityLabel={item.checked ? WORDS.done : WORDS.notDone} accessibilityState={{ checked: item.checked, disabled: true }} style={[styles.box, item.checked ? styles.boxChecked : null]}>
                  {item.checked ? <Icon name="check" size="sm" tone="onAccent" /> : null}
                </View>
              )}
              <View style={styles.itemBody}>
                {item.children.map((child, at) => (
                  <BlockView key={at} id={`${id}.${index}.${at}`} block={child} tone={tone} styles={styles} />
                ))}
              </View>
            </View>
          ))}
        </View>
      );
    case 'quote':
      return (
        <View testID={`${id}.quote`} style={styles.quote}>
          {block.children.map((child, index) => (
            <BlockView key={index} id={`${id}.${index}`} block={child} tone="muted" styles={styles} />
          ))}
        </View>
      );
    case 'rule':
      return <View testID={`${id}.rule`} accessibilityRole="none" style={styles.rule} />;
    case 'table':
      return <TableView id={id} block={block} tone={tone} styles={styles} />;
    case 'code':
      return <CodeView id={id} block={block} styles={styles} />;
  }
}

function inlines(nodes: readonly markdown.Inline[], id: string, styles: Styles): ReactNode {
  return nodes.map((node, index) => {
    const at = `${id}.${index}`;
    switch (node.type) {
      case 'text':
        return node.text;
      case 'break':
        return '\n';
      case 'strong':
        return (
          <Span key={at} style={styles.strong}>
            {inlines(node.children, at, styles)}
          </Span>
        );
      case 'emphasis':
        return (
          <Span key={at} style={styles.emphasis}>
            {inlines(node.children, at, styles)}
          </Span>
        );
      case 'strike':
        return (
          <Span key={at} style={styles.strike}>
            {inlines(node.children, at, styles)}
          </Span>
        );
      case 'code':
        return (
          <Span key={at} style={styles.code}>
            {inlines(node.parts, at, styles)}
          </Span>
        );
      case 'link':
        return (
          <LinkSpan key={at} id={at} link={node} style={styles.link}>
            {inlines(node.children, at, styles)}
          </LinkSpan>
        );
      case 'long':
        return <LongSpan key={at} id={at} long={node} style={styles.long} />;
    }
  });
}

/** The words of inline content, for a label and for the width of a column. */
function words(nodes: readonly markdown.Inline[]): string {
  return nodes.map((node) => (node.type === 'text' || node.type === 'code' || node.type === 'long' ? node.text : node.type === 'break' ? ' ' : words(node.children))).join('');
}

interface LinkProps {
  readonly id: string;
  readonly link: markdown.Link;
  readonly style: TextStyle;
  readonly children: ReactNode;
}

/** A link. It opens in the phone's browser when its address is http or https, and is plain words otherwise. */
function LinkSpan({ id, link, style, children }: LinkProps) {
  const { haptics } = useCapabilities();
  const href = link.opens ? link.href : null;
  const open = useCallback(() => {
    if (href === null) return;
    haptics.play('selection');
    Linking.openURL(href).catch(() => undefined);
  }, [href, haptics]);
  if (href === null) return <Span testID={id}>{children}</Span>;
  return (
    <Span testID={id} accessibilityRole="link" accessibilityLabel={words(link.children) || href} accessibilityHint={href} onPress={open} style={style}>
      {children}
    </Span>
  );
}

/** A long token, shown short. A tap shows the whole of it, and another the short form again. */
function LongSpan({ id, long, style }: { readonly id: string; readonly long: markdown.Long; readonly style: TextStyle }) {
  const [whole, toggle] = useOpen(id);
  return (
    <Span testID={id} accessibilityRole="button" accessibilityLabel={long.full} accessibilityHint={whole ? undefined : WORDS.showWhole} accessibilityState={{ expanded: whole }} onPress={toggle} style={style}>
      {whole ? long.full : long.text}
    </Span>
  );
}

function TableView({ id, block, tone, styles }: { readonly id: string; readonly block: markdown.Table; readonly tone: TxtTone; readonly styles: Styles }) {
  const theme = useTheme();
  const widths = useMemo(() => {
    const character = theme.font.lg * CHARACTER;
    return block.head.map((cell, column) => {
      let longest = words(cell).length;
      for (const row of block.rows) longest = Math.max(longest, words(row[column] ?? []).length);
      return Math.ceil(Math.min(Math.max(longest, NARROWEST), WIDEST) * character) + theme.space[3] * 2;
    });
  }, [block, theme]);
  const cell = (nodes: readonly markdown.Inline[], column: number, at: string, head: boolean): ReactNode => (
    <View key={column} style={[styles.cell, { width: widths[column] }]}>
      <Txt kind="label" tone={head ? 'muted' : tone} style={[{ textAlign: block.align[column] ?? 'left' }, head ? styles.strong : null]}>
        {inlines(nodes, at, styles)}
      </Txt>
    </View>
  );
  return (
    <ScrollView testID={`${id}.table`} horizontal showsHorizontalScrollIndicator={false}>
      <View style={styles.table}>
        <View style={styles.tableHead}>{block.head.map((nodes, column) => cell(nodes, column, `${id}.h.${column}`, true))}</View>
        {block.rows.map((row, index) => (
          <View key={index} style={styles.tableRow}>
            {row.map((nodes, column) => cell(nodes, column, `${id}.${index}.${column}`, false))}
          </View>
        ))}
      </View>
    </ScrollView>
  );
}

function CodeView({ id, block, styles }: { readonly id: string; readonly block: markdown.CodeBlock; readonly styles: Styles }) {
  const { haptics } = useCapabilities();
  const [open, , setOpen] = useOpen(`${id}.code`);
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    [],
  );
  const folded = block.collapsed && !open;
  const shown = useMemo(() => (folded ? block.text.split('\n').slice(0, FOLDED_LINES).join('\n') : block.text), [block.text, folded]);
  const copy = useCallback(() => {
    Clipboard.setStringAsync(block.text).catch(() => undefined);
    haptics.play('confirm');
    setCopied(true);
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), COPIED_FOR_MS);
  }, [block.text, haptics]);
  const showAll = useCallback(() => setOpen(true), [setOpen]);
  return (
    <View testID={`${id}.code`} style={styles.codeblock}>
      <View style={styles.codeHead}>
        <Txt kind="small" tone="muted">
          {block.label}
        </Txt>
        <Tap testID={`${id}.copy`} accessibilityLabel={copied ? WORDS.copied : WORDS.copyCode} onPress={copy} style={styles.copy}>
          <Icon name={copied ? 'check' : 'copy'} size="md" tone={copied ? 'green' : 'muted'} />
        </Tap>
      </View>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.codeBody}>
        <Txt testID={`${id}.text`} kind="mono">
          {shown}
        </Txt>
      </ScrollView>
      {folded ? (
        <Tap testID={`${id}.more`} accessibilityLabel={WORDS.showAll(block.lines)} haptic="selection" scales={false} onPress={showAll} style={styles.more}>
          <Txt kind="label" tone="link">
            {WORDS.showAll(block.lines)}
          </Txt>
        </Tap>
      ) : null}
    </View>
  );
}
