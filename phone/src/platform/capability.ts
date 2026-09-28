/**
 * The generic base of the platform layer (AC-134).
 *
 * Everything that differs between iOS and Android sits behind one `Capability` per concern.
 * Screens and shared code use these interfaces only and never ask which platform they run on.
 */

export interface Supported {
  readonly supported: true;
}

/** An honest gap: the capability is missing here, and `reason` says why in plain words. */
export interface Unsupported {
  readonly supported: false;
  readonly reason: string;
}

/** The answer to "can this capability work on this device, in this build, right now?". */
export type Support = Supported | Unsupported;

export const SUPPORTED: Supported = Object.freeze({ supported: true });

export function unsupported(reason: string): Unsupported {
  return Object.freeze({ supported: false, reason });
}

/**
 * One platform capability: its name, a support check and its typed API.
 *
 * @typeParam Name  The capability's key in `Capabilities`.
 * @typeParam Api   What the capability can do. Every implementation (iOS, Android, fake)
 *                  provides exactly this, so callers cannot tell them apart by type.
 */
export type Capability<Name extends string, Api extends object> = Api & {
  readonly name: Name;
  /**
   * Whether the capability works here. Never throws, never prompts the user and never changes
   * anything. A capability that reports unsupported rejects (or throws) a
   * `CapabilityUnsupportedError` with the same reason when it is used anyway, unless its
   * interface says otherwise.
   */
  support(): Promise<Support>;
};

/** Any capability, whatever its name and API. */
export type AnyCapability = Capability<string, object>;

/** The API of a capability: everything except its name and its support check. */
export type ApiOf<C extends AnyCapability> = Omit<C, 'name' | 'support'>;

/**
 * Builds a capability from its parts, keeping the API's methods as plain closures.
 * The capability's type is named by the caller, so the API is checked against the interface.
 *
 * @example
 * defineCapability<HapticsCapability>('haptics', async () => SUPPORTED, { play() {} });
 */
export function defineCapability<C extends AnyCapability>(
  name: C['name'],
  support: () => Promise<Support>,
  api: ApiOf<C>,
): C {
  // The three parts are exactly the members of C.
  return Object.freeze({ ...api, name, support }) as C;
}

/** Thrown or rejected when a capability that reported unsupported is used anyway. */
export class CapabilityUnsupportedError extends Error {
  readonly capability: string;
  readonly reason: string;

  constructor(capability: string, reason: string) {
    super(`${capability} is not supported here: ${reason}`);
    this.name = 'CapabilityUnsupportedError';
    this.capability = capability;
    this.reason = reason;
  }
}

/** Stops a subscription. Calling it twice is harmless. */
export type Unsubscribe = () => void;

export type Listener<T> = (value: T) => void;

/** Values that JSON can carry: what the key-value store and notification payloads hold. */
export type Json =
  string | number | boolean | null | readonly Json[] | { readonly [key: string]: Json };

/**
 * `T` when JSON can carry it without loss, otherwise a type nothing satisfies. Unlike `Json`
 * it accepts interfaces, which have no index signature.
 */
export type JsonOf<T> = T extends string | number | boolean | null
  ? T
  : T extends (...args: never[]) => unknown
    ? never
    : T extends readonly (infer Item)[]
      ? readonly JsonOf<Item>[]
      : T extends object
        ? { readonly [K in keyof T]: JsonOf<T[K]> }
        : never;
