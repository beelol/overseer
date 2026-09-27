import { Text, type TextProps, type TextStyle } from 'react-native';

import { lineHeight, type Theme } from '@/theme';

import { makeStyles, weight } from './styles';

export type TxtKind = 'title' | 'heading' | 'body' | 'strong' | 'label' | 'small' | 'mono';
export type TxtTone = 'text' | 'muted' | 'faint' | 'accent' | 'link' | 'red' | 'green' | 'amber' | 'onAccent';

export interface TxtProps extends TextProps {
  readonly kind?: TxtKind;
  readonly tone?: TxtTone;
}

/** The largest the system's text size may make the app's text: the largest standard size. */
export const MAX_TEXT_SCALE = 1.6;

const kinds = (theme: Theme): Record<TxtKind, TextStyle> => ({
  title: { fontSize: theme.phone.font.title, lineHeight: lineHeight(theme.phone.font.title, theme.line.tight), fontWeight: weight(theme.weight.semibold) },
  heading: { fontSize: theme.font.xxl, lineHeight: lineHeight(theme.font.xxl, theme.line.tight), fontWeight: weight(theme.weight.semibold) },
  body: { fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.body), fontWeight: weight(theme.weight.regular) },
  strong: { fontSize: theme.font.xl, lineHeight: lineHeight(theme.font.xl, theme.line.tight), fontWeight: weight(theme.weight.semibold) },
  label: { fontSize: theme.font.lg, lineHeight: lineHeight(theme.font.lg, theme.line.tight), fontWeight: weight(theme.weight.medium) },
  small: { fontSize: theme.font.sm, lineHeight: lineHeight(theme.font.sm, theme.line.tight), fontWeight: weight(theme.weight.regular) },
  mono: { fontSize: theme.phone.font.mono, lineHeight: lineHeight(theme.phone.font.mono, theme.line.body), fontFamily: 'Menlo' },
});

const useStyles = makeStyles((theme) => ({
  ...kinds(theme),
  'tone.text': { color: theme.colors.text },
  'tone.muted': { color: theme.colors.muted },
  'tone.faint': { color: theme.colors.faint },
  'tone.accent': { color: theme.colors.accent },
  'tone.link': { color: theme.colors.link },
  'tone.red': { color: theme.colors.red },
  'tone.green': { color: theme.colors.green },
  'tone.amber': { color: theme.colors.amber },
  'tone.onAccent': { color: theme.colors.onAccent },
}));

/** The app's text. It follows the system's text size up to the largest standard size. */
export function Txt({ kind = 'body', tone = 'text', style, ...rest }: TxtProps) {
  const styles = useStyles();
  return <Text maxFontSizeMultiplier={MAX_TEXT_SCALE} {...rest} style={[styles[kind], styles[`tone.${tone}`], style]} />;
}
