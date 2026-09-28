import { act, fireEvent, screen } from '@testing-library/react-native';
import { useState } from 'react';

import { createTestApp } from '@/testing';
import { phone } from '@/theme/tokens.generated';

import { Menu } from '../Sheet';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

const ran: string[] = [];
function Shown() {
  const [open, setOpen] = useState(true);
  return (
    <Menu
      testID="menu"
      open={open}
      onClose={() => setOpen(false)}
      items={[
        { id: 'now', label: 'Now', onPress: () => ran.push('now') },
        { id: 'picker', label: 'Picker', afterClose: true, onPress: () => ran.push('picker') },
      ]}
    />
  );
}

describe('a menu', () => {
  beforeEach(() => ran.splice(0));

  test('an item runs as it is chosen, and the sheet closes', async () => {
    const app = await createTestApp();
    await app.render(<Shown />);
    await fireEvent.press(screen.getByTestId('menu.now'));
    expect(ran).toEqual(['now']);
  });

  test('an item that shows a screen of the system runs only once the sheet is gone', async () => {
    const app = await createTestApp();
    await app.render(<Shown />);
    await fireEvent.press(screen.getByTestId('menu.picker'));
    expect(ran).toEqual([]);
    await act(() => new Promise((resolve) => setTimeout(resolve, phone.motion.sheet.open + 20)));
    expect(ran).toEqual(['picker']);
  });
});
