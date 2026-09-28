import { defineCapability, type Support } from '../capability';
import type {
  DeviceUnlockCapability,
  UnlockMethod,
  UnlockRequest,
  UnlockResult,
} from '../capabilities/deviceUnlock';
import { createFakeSupport, type FakeSupport } from './support';

export interface FakeDeviceUnlock {
  readonly capability: DeviceUnlockCapability;
  readonly support: FakeSupport;
  setMethods(methods: readonly UnlockMethod[]): void;
  /** What the next prompts end with; success unless a test changes it. */
  answerWith(result: UnlockResult): void;
  /** Every prompt shown so far, in order. */
  requests(): readonly UnlockRequest[];
}

export function createFakeDeviceUnlock(initial?: Support): FakeDeviceUnlock {
  const support = createFakeSupport('deviceUnlock', initial);
  let methods: readonly UnlockMethod[] = ['face', 'passcode'];
  let answer: UnlockResult = { ok: true };
  const requests: UnlockRequest[] = [];
  const capability = defineCapability<DeviceUnlockCapability>('deviceUnlock', support.check, {
    async methods() {
      support.require();
      return methods;
    },
    async unlock(request) {
      support.require();
      requests.push(request);
      return methods.length === 0 ? { ok: false, cause: 'notSetUp' } : answer;
    },
  });
  return {
    capability,
    support,
    setMethods(next) {
      methods = Object.freeze([...next]);
    },
    answerWith(result) {
      answer = result;
    },
    requests: () => requests,
  };
}
