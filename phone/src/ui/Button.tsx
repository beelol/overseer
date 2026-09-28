import type { ReactNode } from 'react';
import { View } from 'react-native';

import { Tap, type TapProps } from '@/motion';

import { Icon, type IconName } from './Icon';
import { makeStyles } from './styles';
import { Txt, type TxtTone } from './Txt';

export type ButtonKind = 'primary' | 'secondary' | 'danger' | 'quiet';

export interface ButtonProps extends Omit<TapProps, 'children' | 'accessibilityLabel' | 'style'> {
  readonly label: string;
  /** Said by VoiceOver and TalkBack when the label alone is not enough. */
  readonly accessibilityLabel?: string;
  readonly kind?: ButtonKind;
  readonly icon?: IconName;
  /** True stretches the button across its row. */
  readonly wide?: boolean;
}

const useStyles = makeStyles((theme) => ({
  base: {
    minHeight: theme.phone.size.touch,
    paddingHorizontal: theme.space[4],
    borderRadius: theme.radius.card,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: theme.space[2],
  },
  wide: { alignSelf: 'stretch' },
  primary: { backgroundColor: theme.colors.accentStrong },
  secondary: { backgroundColor: theme.colors.raised2, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong },
  danger: { backgroundColor: theme.colors.removedBg, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.red },
  quiet: { backgroundColor: 'transparent' },
}));

const TONE: Record<ButtonKind, TxtTone> = { primary: 'onAccent', secondary: 'text', danger: 'red', quiet: 'link' };

export function Button({ label, accessibilityLabel, kind = 'secondary', icon, wide, ...rest }: ButtonProps) {
  const styles = useStyles();
  return (
    <Tap {...rest} accessibilityLabel={accessibilityLabel ?? label} style={[styles.base, styles[kind], wide ? styles.wide : null]}>
      {icon ? <Icon name={icon} size="md" tone={TONE[kind]} /> : null}
      <Txt kind="label" tone={TONE[kind]} numberOfLines={1}>
        {label}
      </Txt>
    </Tap>
  );
}

export interface IconButtonProps extends Omit<TapProps, 'children' | 'style'> {
  readonly icon: IconName;
  readonly tone?: TxtTone;
  /** A small count beside the icon (changed files). */
  readonly badge?: string;
}

const useIconStyles = makeStyles((theme) => ({
  base: { minWidth: theme.phone.size.touch, minHeight: theme.phone.size.touch, alignItems: 'center', justifyContent: 'center', flexDirection: 'row', gap: theme.space[1] },
}));

/** A control that is an icon only. Its label is what is said aloud. */
export function IconButton({ icon, tone = 'text', badge, ...rest }: IconButtonProps) {
  const styles = useIconStyles();
  return (
    <Tap {...rest} style={styles.base}>
      <Icon name={icon} size="lg" tone={tone} />
      {badge ? (
        <Txt kind="small" tone="muted">
          {badge}
        </Txt>
      ) : null}
    </Tap>
  );
}

export interface ChipProps extends Omit<TapProps, 'children' | 'style' | 'accessibilityLabel'> {
  readonly label: string;
  readonly accessibilityLabel?: string;
  readonly selected?: boolean;
  readonly icon?: IconName;
  readonly count?: number;
}

const useChipStyles = makeStyles((theme) => ({
  base: {
    minHeight: theme.space[8],
    paddingHorizontal: theme.space[3],
    borderRadius: theme.radius.pill,
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.space[1],
    borderWidth: theme.phone.size.hairline,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.raised,
  },
  selected: { backgroundColor: theme.colors.selected, borderColor: theme.colors.focus },
}));

/** A small choice: a filter, a model, an effort. */
export function Chip({ label, accessibilityLabel, selected, icon, count, ...rest }: ChipProps) {
  const styles = useChipStyles();
  return (
    <Tap {...rest} accessibilityLabel={accessibilityLabel ?? (count === undefined ? label : `${label}, ${count}`)} accessibilityState={{ selected: Boolean(selected) }} style={[styles.base, selected ? styles.selected : null]}>
      {icon ? <Icon name={icon} size="sm" tone={selected ? 'text' : 'muted'} /> : null}
      <Txt kind="label" tone={selected ? 'text' : 'muted'} numberOfLines={1}>
        {label}
      </Txt>
      {count !== undefined && count > 0 ? (
        <Txt kind="small" tone={selected ? 'text' : 'accent'}>
          {count}
        </Txt>
      ) : null}
    </Tap>
  );
}

/** A row of controls with even gaps. */
export function Actions({ children }: { readonly children: ReactNode }) {
  const styles = useActionStyles();
  return <View style={styles.row}>{children}</View>;
}

const useActionStyles = makeStyles((theme) => ({ row: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.space[2], alignItems: 'center' } }));
