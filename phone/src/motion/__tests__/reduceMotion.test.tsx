import { renderHook } from '@testing-library/react-native';
import type { ReactNode } from 'react';
import { ReduceMotion } from 'react-native-reanimated';

import { PlatformProvider } from '@/platform';
import { createFakePlatform } from '@/platform/fake';

import { useMotion } from '../motion';

/** The app's providers, once the fake has reported the system's setting (as the real one does, later). */
async function wrapperFor(reduceMotion: boolean) {
  const platform = createFakePlatform({ reduceMotion });
  await platform.capabilities.reduceMotion.refresh();
  return function Wrapper({ children }: { readonly children: ReactNode }) {
    return <PlatformProvider capabilities={platform.capabilities}>{children}</PlatformProvider>;
  };
}

describe("Reduce Motion is the app's to honour", () => {
  test.each([false, true])(
    'with Reduce Motion %s, timings and springs are never skipped by Reanimated',
    async (reduced) => {
      const { result } = await renderHook(() => useMotion(), {
        wrapper: await wrapperFor(reduced),
      });
      expect(result.current.reduced).toBe(reduced);
      const fade = result.current.tokens.door.fade;
      expect(result.current.timing(fade)).toMatchObject({
        duration: fade,
        reduceMotion: ReduceMotion.Never,
      });
      expect(result.current.spring('snappy')).toMatchObject({ reduceMotion: ReduceMotion.Never });
    },
  );

  test('with Reduce Motion on, nothing travels', async () => {
    const { result } = await renderHook(() => useMotion(), { wrapper: await wrapperFor(true) });
    expect(result.current.travel(result.current.tokens.distance.arrive)).toBe(0);
  });
});
