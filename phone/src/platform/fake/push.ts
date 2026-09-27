import { defineCapability, type Listener, type Support } from '../capability';
import type {
  ForegroundPresentation,
  PushCapability,
  PushCategory,
  PushDeviceToken,
  PushNotification,
  PushPermission,
  PushResponse,
} from '../capabilities/push';
import { createFakeSupport, type FakeSupport } from './support';

export const FAKE_DEVICE_TOKEN: PushDeviceToken = Object.freeze({
  service: 'apns',
  token: '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff',
});

export interface FakePush {
  readonly capability: PushCapability;
  readonly support: FakeSupport;
  /** What the owner will answer when asked; `granted` unless a test changes it. */
  answerRequestWith(permission: PushPermission): void;
  /** How many times the system's prompt would have been shown. */
  prompts(): number;
  categories(): readonly PushCategory[];
  foregroundPresentation(): ForegroundPresentation;
  /** A notification arrives while the app is open. */
  deliver(notification: PushNotification): void;
  /** The owner taps a notification or one of its actions. */
  respond(response: PushResponse): void;
  /** The app was opened by this response. */
  launchWith(response: PushResponse | null): void;
}

export function createFakePush(initial?: Support): FakePush {
  const support = createFakeSupport('push', initial);
  let permission: PushPermission = 'undetermined';
  let answer: PushPermission = 'granted';
  let prompts = 0;
  let categories: readonly PushCategory[] = [];
  let presentation: ForegroundPresentation = 'show';
  let launch: PushResponse | null = null;
  const received = new Set<Listener<PushNotification>>();
  const responses = new Set<Listener<PushResponse>>();

  function listen<T>(listeners: Set<Listener<T>>, handler: Listener<T>): () => void {
    support.require();
    const entry: Listener<T> = (value) => handler(value);
    listeners.add(entry);
    return () => {
      listeners.delete(entry);
    };
  }

  const capability = defineCapability<PushCapability>('push', support.check, {
    async permission() {
      support.require();
      return permission;
    },
    async requestPermission() {
      support.require();
      // The system asks once; afterwards it repeats the first answer.
      if (permission === 'undetermined') {
        prompts += 1;
        permission = answer;
      }
      return permission;
    },
    async deviceToken() {
      support.require();
      if (permission !== 'granted' && permission !== 'provisional') {
        throw new Error('the fake push service gives a token only after permission was granted');
      }
      return FAKE_DEVICE_TOKEN;
    },
    async setCategories(next) {
      support.require();
      categories = Object.freeze([...next]);
    },
    setForegroundPresentation(next) {
      support.require();
      presentation = next;
    },
    onReceived: (handler) => listen(received, handler),
    onResponse: (handler) => listen(responses, handler),
    async launchResponse() {
      support.require();
      return launch;
    },
  });

  return {
    capability,
    support,
    answerRequestWith(next) {
      answer = next;
    },
    prompts: () => prompts,
    categories: () => categories,
    foregroundPresentation: () => presentation,
    deliver(notification) {
      for (const listener of [...received]) listener(notification);
    },
    respond(response) {
      for (const listener of [...responses]) listener(response);
    },
    launchWith(response) {
      launch = response;
    },
  };
}
