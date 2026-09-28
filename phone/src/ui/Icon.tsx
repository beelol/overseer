import { Text, type StyleProp, type TextStyle } from 'react-native';

import { useTheme, type Theme } from '@/theme';

import { ICONS, type IconName } from './icons.generated';
import type { TxtTone } from './Txt';

export type { IconName };

export interface IconProps {
  readonly name: IconName;
  readonly size?: keyof Theme['phone']['size']['icon'];
  /** A colour of words, or `faint`, which is for decoration and never for words. */
  readonly tone?: TxtTone | 'faint';
  readonly style?: StyleProp<TextStyle>;
}

/**
 * An icon of the set VS Code uses (codicons), drawn from the same font file. It is decoration:
 * the control that holds it carries the label.
 */
export function Icon({ name, size = 'lg', tone = 'text', style }: IconProps) {
  const theme = useTheme();
  const points = theme.phone.size.icon[size];
  return (
    <Text
      accessible={false}
      importantForAccessibility="no"
      allowFontScaling={false}
      style={[{ fontFamily: 'codicon', fontSize: points, lineHeight: points, width: points, height: points, color: theme.colors[tone], textAlign: 'center' }, style]}
    >
      {String.fromCodePoint(ICONS[name])}
    </Text>
  );
}
