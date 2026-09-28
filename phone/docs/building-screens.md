# Building a screen

How a screen of the phone app is built and tested. What each screen holds is in
[app-spec.md](app-spec.md); this file is the how.

## What a screen is made of

| Need | Use | Where |
| --- | --- | --- |
| The daemon's state, live | `useSessionValue((s) => s.state)` and the selectors of `@/model` | `src/session`, `phone/model/API.md` |
| A conversation, live | `useConversation(runId)` | `src/session` |
| To ask or change something on the Mac | `useSession().request(method, params)` | types in `phone/protocol/protocol.generated.ts` |
| The connection, the scope, the outbox | `useSessionValue((s) => s.connection)`, `.scope`, `.outbox` | `src/session/types.ts` |
| The frame, header, connection line | `<Screen id title …>` | `src/ui` |
| Text, icons, logos, buttons, rows, switches, sheets, menus | `Txt`, `Icon`, `Logo`, `Button`, `IconButton`, `Chip`, `Section`, `Row`, `SwitchRow`, `Sheet`, `Menu`, `Confirm`, `Empty` | `src/ui` |
| Anything pressed | `Tap` (answers on the UI thread, plays the haptic) | `src/motion` |
| Something arriving, the needs-you pulse | `Arrive`, `Pulse`, `useMotion()` | `src/motion` |
| Styles | `makeStyles((theme) => ({ … }))` | `src/ui/styles.ts` |
| Where to go | `routes.agent(id)` … with `useRouter()` of expo-router | `src/routes.ts` |
| The camera, haptics, push, storage, the device | a capability from `useCapabilities()` | `src/platform` |

## Rules the checks enforce

- **Tokens only.** No colour, font size, space, radius, duration or font weight written by hand:
  take it from `theme` (`theme.space[4]`, `theme.font.lg`, `theme.colors.muted`,
  `theme.phone.size.touch`, `theme.phone.motion…`). A value that is missing is added to
  `phone/design/phone-tokens.json` by the owner of the shell, not written in a screen: report it.
- **No platform test.** Never `Platform.OS`, never a comparison with `'ios'` or `'android'`, never
  a platform library imported directly. Ask a capability (`launch.info().conventions`, …).
- **Every control** is a `Tap` (or a piece built on it) with a `testID` (`screen.part`) and an
  `accessibilityLabel`. Text is `Txt`, so it follows the system's text size.
- **Words** are the brief's and the model's (`text.TEXT`, which copies VS Code's wording). Never
  "gateway", "daemon", "session", "handshake", "socket".
- TypeScript is strict: no `any`, no `!`.

## Rules the checks cannot enforce

- **Never wait on the Mac to show what the person did.** A change is shown at once and sent
  through `session.request`, which queues it, sends it once, and keeps it across a lost
  connection. The outbox (`s.outbox`) says what is queued, sending, done or failed.
- **Never keep a copy of the daemon's state.** Read it from the session; derive with `useMemo`.
- **Lists build visible rows only**: `FlashList` from `@shopify/flash-list`, stable keys, rows in
  `React.memo`, callbacks stable. A conversation can hold 5,000 rows.
- **A watch-only phone** (`s.scope === 'watch'`) sees everything and no control that changes
  something; `WatchOnlyLine` stands where the controls would be.
- **Not connected** is not an error screen: what is stored stays, marked with its age.
- **Destructive actions** ask once with `Confirm`, naming what will be lost.
- Portrait and landscape, small phones and large text: rows wrap, nothing has a fixed height
  that text can outgrow.

## Packages in the native build

`@shopify/flash-list`, `react-native-svg`, `react-native-reanimated`,
`react-native-gesture-handler`, `expo-linear-gradient`, `expo-image-picker`,
`expo-image-manipulator`, `expo-clipboard`, `expo-linking`. Adding a package with native code
means a new native build: ask first.

## Tests

Screens are tested on the Mac against the fakes, with no simulator:

```tsx
import { fireEvent, screen } from '@testing-library/react-native';
import { createTestApp, makeEvent } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

test('…', async () => {
  const app = await createTestApp({ state });          // paired and connected unless told
  app.connection.answers['workspace.changes'] = () => ({ … });
  router.params = { run: 'r1' };
  await app.render(<ChangesScreen />);
  await fireEvent.press(screen.getByTestId('changes.comparison'));
  await app.events(makeEvent('status', { status: 'completed' }, { run_id: 'r1' }));
  expect(app.connection.calls('workspace.changes')).toHaveLength(2);
});
```

`render`, `fireEvent` and `act` of the testing library are asynchronous: await them.
Recorded sessions of the fixture agents are in `phone/model/test/fixtures/`.

From `phone/`:

```bash
npx tsc --noEmit
```

```bash
npx eslint app src --max-warnings 0
```

```bash
npx jest src/screens
```
