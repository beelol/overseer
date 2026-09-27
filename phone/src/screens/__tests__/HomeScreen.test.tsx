import { act, render, screen } from '@testing-library/react-native';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { CAPABILITY_NAMES, PlatformProvider, unsupported } from '@/platform';
import {
  FAKE_ANDROID_EMULATOR,
  FAKE_IPHONE,
  createFakePlatform,
  type FakeOptions,
} from '@/platform/fake';
import { HomeScreen } from '@/screens/HomeScreen';
import { palettes } from '@/theme/tokens.generated';

const frame = { x: 0, y: 0, width: 402, height: 874 };
const insets = { top: 0, left: 0, right: 0, bottom: 0 };

async function open(options?: FakeOptions) {
  const platform = createFakePlatform(options);
  await render(
    <SafeAreaProvider initialMetrics={{ frame, insets }}>
      <PlatformProvider capabilities={platform.capabilities}>
        <HomeScreen />
      </PlatformProvider>
    </SafeAreaProvider>,
  );
  return platform;
}

describe('the home screen, against the fakes', () => {
  test('shows the app, the theme and what the iOS simulator adds', async () => {
    await open({ appearance: 'dark' });
    expect(screen.getByText('Overseer')).toBeTruthy();
    expect(screen.getByLabelText('Theme: Overseer Dark')).toBeTruthy();
    expect(screen.getByLabelText('Platform: ios 26.5')).toBeTruthy();
    expect(screen.getByLabelText('Simulator: yes')).toBeTruthy();
    expect(screen.getByLabelText('Host addresses: 127.0.0.1')).toBeTruthy();
    expect(screen.getByLabelText('Addresses to try: 127.0.0.1:47810')).toBeTruthy();
    expect(screen.getByLabelText('Engine: hermes')).toBeTruthy();
    expect(screen.getByLabelText('New Architecture: on')).toBeTruthy();
  });

  test('shows what the Android emulator adds', async () => {
    await open({ launch: FAKE_ANDROID_EMULATOR });
    expect(screen.getByLabelText('Platform: android 15')).toBeTruthy();
    expect(screen.getByLabelText('Host addresses: 10.0.2.2')).toBeTruthy();
    expect(screen.getByLabelText('Addresses to try: 10.0.2.2:47810')).toBeTruthy();
  });

  test('a real phone adds no address of its own', async () => {
    await open({ launch: FAKE_IPHONE });
    expect(screen.getByLabelText('Simulator: no')).toBeTruthy();
    expect(screen.getByLabelText('Host addresses: none')).toBeTruthy();
    expect(screen.getByLabelText('Addresses to try: none')).toBeTruthy();
  });

  test('shows the result of every support check, with the reason for each gap', async () => {
    await open({ support: { push: unsupported('No push in this test.') } });
    for (const name of CAPABILITY_NAMES) {
      expect(screen.getByTestId(`support-${name}`)).not.toHaveTextContent('checking');
    }
    expect(screen.getByTestId('support-secretStore')).toHaveTextContent('supported');
    expect(screen.getByTestId('support-push')).toHaveTextContent(
      'unsupported: No push in this test.',
    );
    // The fake simulator is as honest as the real ones about what it lacks.
    expect(screen.getByTestId('support-camera')).toHaveTextContent(/^unsupported: /);
    expect(screen.getByTestId('support-haptics')).toHaveTextContent(/^unsupported: /);
    expect(screen.getByTestId('support-discovery')).toHaveTextContent(/^unsupported: /);
  });

  test('changes theme while it is open', async () => {
    const platform = await open({ appearance: 'light' });
    expect(screen.getByText('Overseer')).toHaveStyle({ color: palettes.light.text });
    await act(() => platform.fakes.appearance.set('dark'));
    expect(screen.getByLabelText('Theme: Overseer Dark')).toBeTruthy();
    expect(screen.getByText('Overseer')).toHaveStyle({ color: palettes.dark.text });
  });
});
