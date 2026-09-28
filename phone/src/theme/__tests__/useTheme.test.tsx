import { act, render, renderHook, screen } from '@testing-library/react-native';
import { useEffect, type ReactNode } from 'react';
import { Text } from 'react-native';

import { PlatformProvider, type Capabilities } from '@/platform';
import { createFakePlatform, type FakePlatform } from '@/platform/fake';
import { lineHeight, themes, useTheme } from '@/theme';
import { motion, palettes, scale } from '@/theme/tokens.generated';

function providerFor(platform: FakePlatform) {
  return function Provider({ children }: { children: ReactNode }) {
    return <PlatformProvider capabilities={platform.capabilities}>{children}</PlatformProvider>;
  };
}

describe('useTheme', () => {
  test('is Overseer Light when the phone is light', async () => {
    const platform = createFakePlatform({ appearance: 'light' });
    const { result } = await renderHook(() => useTheme(), { wrapper: providerFor(platform) });
    expect(result.current.name).toBe('Overseer Light');
    expect(result.current.scheme).toBe('light');
    expect(result.current.colors).toBe(palettes.light);
  });

  test('is Overseer Dark when the phone is dark', async () => {
    const platform = createFakePlatform({ appearance: 'dark' });
    const { result } = await renderHook(() => useTheme(), { wrapper: providerFor(platform) });
    expect(result.current.name).toBe('Overseer Dark');
    expect(result.current.colors).toBe(palettes.dark);
  });

  test('switches live when the setting changes, without mounting anything again', async () => {
    const platform = createFakePlatform({ appearance: 'light' });
    let mounts = 0;
    function Screen() {
      const theme = useTheme();
      useEffect(() => {
        mounts += 1;
      }, []);
      return <Text style={{ color: theme.colors.text }}>{theme.name}</Text>;
    }

    await render(<Screen />, { wrapper: providerFor(platform) });
    expect(screen.getByText('Overseer Light')).toHaveStyle({ color: palettes.light.text });

    await act(() => platform.fakes.appearance.set('dark'));
    expect(screen.getByText('Overseer Dark')).toHaveStyle({ color: palettes.dark.text });

    await act(() => platform.fakes.appearance.set('light'));
    expect(screen.getByText('Overseer Light')).toHaveStyle({ color: palettes.light.text });

    expect(mounts).toBe(1);
  });

  test('stops listening when the screen goes away', async () => {
    const platform = createFakePlatform();
    const real = platform.capabilities.appearance;
    let listening = 0;
    const appearance: Capabilities['appearance'] = {
      ...real,
      subscribe(listener) {
        listening += 1;
        const stop = real.subscribe(listener);
        return () => {
          listening -= 1;
          stop();
        };
      },
    };
    const capabilities: Capabilities = { ...platform.capabilities, appearance };

    const view = await renderHook(() => useTheme(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <PlatformProvider capabilities={capabilities}>{children}</PlatformProvider>
      ),
    });
    expect(listening).toBe(1);
    await view.unmount();
    expect(listening).toBe(0);
  });

  test('needs the platform layer above it', async () => {
    const quiet = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    await expect(renderHook(() => useTheme())).rejects.toThrow('needs a <PlatformProvider>');
    quiet.mockRestore();
  });
});

describe('the themes', () => {
  test('carry the generated tokens and nothing of their own', () => {
    for (const scheme of ['light', 'dark'] as const) {
      const theme = themes[scheme];
      expect(theme.colors).toBe(palettes[scheme]);
      expect(theme.colors.type).toBe(scheme);
      expect(theme.space).toBe(scale.space);
      expect(theme.radius).toBe(scale.radius);
      expect(theme.font).toBe(scale.font);
      expect(theme.weight).toBe(scale.weight);
      expect(theme.line).toBe(scale.line);
      expect(theme.chat).toBe(scale.chat);
      expect(theme.motion).toBe(motion);
    }
  });

  test('line heights are the size times the line token, in whole points', () => {
    expect(lineHeight(scale.font.lg, scale.line.body)).toBe(
      Math.round(scale.font.lg * scale.line.body),
    );
    expect(Number.isInteger(lineHeight(scale.font.md, scale.line.tight))).toBe(true);
  });
});
