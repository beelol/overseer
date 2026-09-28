import { View } from 'react-native';
import { SvgXml } from 'react-native-svg';

import { useTheme, type Theme } from '@/theme';

import { LOGOS, MARK, type LogoFile } from './logos.generated';

/** The logo VS Code shows for a harness or a provider, by the names the daemon uses. */
const FILES: Readonly<Record<string, string>> = {
  claude: 'claude',
  'claude-code': 'claudecode',
  claudecode: 'claudecode',
  anthropic: 'anthropic',
  codex: 'codex',
  openai: 'openai',
  opencode: 'opencode',
  github: 'github',
};

export function logoFile(name: string, scheme: Theme['scheme']): LogoFile | null {
  const base = FILES[name.toLowerCase()];
  if (!base) return null;
  const file = `${base}-${scheme}`;
  return file in LOGOS ? (file as LogoFile) : null;
}

export interface LogoProps {
  /** A harness (`claude`, `codex`, `opencode`) or a provider (`anthropic`, `openai`, `github`). */
  readonly name: string;
  readonly size?: keyof Theme['phone']['size']['logo'];
}

/** A provider's logo: the same file VS Code shows, in the current theme. */
export function Logo({ name, size = 'row' }: LogoProps) {
  const theme = useTheme();
  const points = theme.phone.size.logo[size];
  const file = logoFile(name, theme.scheme);
  if (!file) return <Mark size={points} color={theme.colors.muted} />;
  return (
    <View accessible={false} importantForAccessibility="no-hide-descendants" style={{ width: points, height: points }}>
      <SvgXml xml={LOGOS[file]} width={points} height={points} />
    </View>
  );
}

/** Overseer's mark in one colour. */
export function Mark({ size, color }: { readonly size: number; readonly color: string }) {
  return (
    <View accessible={false} importantForAccessibility="no-hide-descendants" style={{ width: size, height: size }}>
      <SvgXml xml={MARK} width={size} height={size} color={color} />
    </View>
  );
}
