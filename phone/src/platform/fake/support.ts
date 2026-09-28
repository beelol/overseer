import { CapabilityUnsupportedError, SUPPORTED, type Support } from '../capability';

/** A support answer a test can change, shared by every fake. */
export interface FakeSupport {
  get(): Support;
  set(next: Support): void;
  /** Throws the error a real capability throws when it is unsupported and used anyway. */
  require(): void;
  check(): Promise<Support>;
}

export function createFakeSupport(capability: string, initial: Support = SUPPORTED): FakeSupport {
  let current = initial;
  return {
    get: () => current,
    set(next) {
      current = next;
    },
    require() {
      if (!current.supported) throw new CapabilityUnsupportedError(capability, current.reason);
    },
    check: async () => current,
  };
}
