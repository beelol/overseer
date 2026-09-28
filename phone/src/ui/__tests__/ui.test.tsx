import { fireEvent, screen } from '@testing-library/react-native';

import { FAKE_IPHONE } from '@/platform/fake';
import { createTestApp } from '@/testing';
import { router } from '@/testing/router';
import { Button, connectionText, ICONS_FOR_TEST, Menu, Row, Screen, Section, SwitchRow, Txt } from '@/ui';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

beforeEach(() => router.reset());

describe('the pieces screens are built from', () => {
  test('a screen has its title, a way back and no connection line while connected', async () => {
    const app = await createTestApp();
    await app.render(
      <Screen id="settings" title="Settings">
        <Txt>inside</Txt>
      </Screen>,
    );
    expect(screen.getByTestId('settings.title').props.children).toBe('Settings');
    await fireEvent.press(screen.getByTestId('settings.back'));
    expect(router.backs).toBe(1);
    expect(screen.queryByTestId('connection.reconnecting')).toBeNull();
    expect(screen.queryByTestId('connection.unreachable')).toBeNull();
  });

  test('the connection line says each state in the words of the brief', async () => {
    const now = 10 * 60_000;
    expect(connectionText('online', null, now)).toBeNull();
    expect(connectionText('connecting', null, now)).toEqual({ id: 'connection.reconnecting', text: 'Reconnecting…' });
    expect(connectionText('reconnecting', null, now)?.id).toBe('connection.reconnecting');
    expect(connectionText('unreachable', now - 2 * 60_000, now)).toEqual({ id: 'connection.unreachable', text: 'Mac unreachable · last contact 2m ago' });
    expect(connectionText('off', null, now)).toEqual({ id: 'connection.off', text: 'Phone access is off on the Mac' });
    expect(connectionText('revoked', null, now)).toEqual({ id: 'connection.revoked', text: 'This phone was removed on the Mac' });
  });

  test('the line appears when the Mac cannot be reached, and what is on screen stays', async () => {
    const app = await createTestApp({ connection: 'unreachable' });
    await app.render(
      <Screen id="agents" title="Agents" back={false}>
        <Txt testID="content">stored</Txt>
      </Screen>,
    );
    expect(screen.getByTestId('connection.unreachable')).toBeTruthy();
    expect(screen.getByTestId('content')).toBeTruthy();
    expect(screen.queryByTestId('agents.back')).toBeNull();
  });

  test('controls carry a label and a test id, and play their haptic', async () => {
    const app = await createTestApp({ launch: FAKE_IPHONE });
    const pressed = jest.fn();
    const changed = jest.fn();
    await app.render(
      <Section title="Notifications">
        <Button testID="new.start" label="Start" kind="primary" haptic="confirm" onPress={pressed} />
        <Row testID="settings.mac" label="The Mac" value="Studio" />
        <SwitchRow testID="settings.notifications.all" label="Notifications" value={false} onChange={changed} />
      </Section>,
    );
    await fireEvent.press(screen.getByLabelText('Start'));
    expect(pressed).toHaveBeenCalledTimes(1);
    expect(app.platform.fakes.haptics.played()).toEqual(['confirm']);
    expect(screen.getByLabelText('The Mac, Studio')).toBeTruthy();
    await fireEvent(screen.getByTestId('settings.notifications.all'), 'valueChange', true);
    expect(changed).toHaveBeenCalledWith(true);
  });

  test('a menu closes when one of its actions is chosen', async () => {
    const app = await createTestApp();
    const closed = jest.fn();
    const stop = jest.fn();
    await app.render(<Menu testID="agent.more" open onClose={closed} items={[{ id: 'stop', label: 'Stop', danger: true, onPress: stop }]} />);
    await fireEvent.press(screen.getByTestId('agent.more.stop'));
    expect(closed).toHaveBeenCalled();
    expect(stop).toHaveBeenCalled();
  });

  test('the icons the pieces use exist in the font', () => {
    expect(ICONS_FOR_TEST['chevron-left']).toBeGreaterThan(0);
    expect(ICONS_FOR_TEST['chevron-right']).toBeGreaterThan(0);
  });
});
