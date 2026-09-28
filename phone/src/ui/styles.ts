import { useMemo } from 'react';
import { StyleSheet } from 'react-native';

import { useTheme, type Theme } from '@/theme';

type Named<T> = StyleSheet.NamedStyles<T> | StyleSheet.NamedStyles<unknown>;

/**
 * The styles of a component for the current theme. `factory` is defined outside the component,
 * so the styles are built once per theme and not on every draw.
 *
 * @example
 * const useStyles = makeStyles((theme) => ({ row: { padding: theme.space[4] } }));
 * const styles = useStyles();
 */
export function makeStyles<T extends Named<T>>(factory: (theme: Theme) => T): () => T {
  const built = new Map<Theme, T>();
  return function useStyles(): T {
    const theme = useTheme();
    return useMemo(() => {
      let styles = built.get(theme);
      if (!styles) {
        styles = StyleSheet.create(factory(theme)) as T;
        built.set(theme, styles);
      }
      return styles;
    }, [theme]);
  };
}

/** A font weight token as React Native takes it. */
export function weight(value: number): '400' | '500' | '600' {
  return String(value) as '400' | '500' | '600';
}
