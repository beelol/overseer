/** A stand-in for expo-router in a screen's test. See `@/testing`. */
export const router = {
  pushed: [] as unknown[],
  replaced: [] as unknown[],
  backs: 0,
  /** What `useLocalSearchParams` and `useGlobalSearchParams` return. */
  params: {} as Record<string, string>,
  pathname: '/',
  canGoBack: true,
  reset(): void {
    this.pushed = [];
    this.replaced = [];
    this.backs = 0;
    this.params = {};
    this.pathname = '/';
    this.canGoBack = true;
  },
};

const api = {
  push: (href: unknown) => void router.pushed.push(href),
  replace: (href: unknown) => void router.replaced.push(href),
  navigate: (href: unknown) => void router.pushed.push(href),
  back: () => void router.backs++,
  canGoBack: () => router.canGoBack,
  setParams: (params: Record<string, string>) => void Object.assign(router.params, params),
};

export function mockRouter(): Record<string, unknown> {
  return {
    __esModule: true,
    useRouter: () => api,
    router: api,
    useLocalSearchParams: () => router.params,
    useGlobalSearchParams: () => router.params,
    usePathname: () => router.pathname,
    useFocusEffect: (effect: () => void | (() => void)) => {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const { useEffect } = require('react') as typeof import('react');
      useEffect(effect, [effect]);
    },
    Redirect: () => null,
    Stack: Object.assign(() => null, { Screen: () => null }),
    Link: ({ children }: { children: unknown }) => children,
  };
}
