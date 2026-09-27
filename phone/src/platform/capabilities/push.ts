import type { Capability, Json, Listener, Unsubscribe } from '../capability';

/** `provisional` is the quiet delivery iOS can grant without asking. */
export type PushPermission = 'granted' | 'provisional' | 'denied' | 'undetermined';

/** The token the daemon sends to the push service to reach this device. */
export interface PushDeviceToken {
  readonly service: 'apns' | 'fcm';
  readonly token: string;
}

/** A button on a notification, for example Allow or Deny on a permission request. */
export interface PushAction {
  readonly id: string;
  readonly title: string;
  /** The phone must be unlocked before the action is delivered. */
  readonly requiresUnlock: boolean;
  /** The system shows the action as destructive. */
  readonly destructive: boolean;
  /** Choosing the action brings the app to the foreground. */
  readonly opensApp: boolean;
}

/** A kind of notification and the actions it offers. */
export interface PushCategory {
  readonly id: string;
  readonly actions: readonly PushAction[];
}

export interface PushNotification {
  readonly id: string;
  readonly title: string | null;
  readonly body: string | null;
  readonly categoryId: string | null;
  /** The payload the daemon sent with the notification. */
  readonly data: Readonly<Record<string, Json>>;
}

/** What the owner did with a notification. */
export interface PushResponse {
  readonly notification: PushNotification;
  /** The chosen action's id, or `null` when the notification itself was tapped. */
  readonly actionId: string | null;
}

/** Whether a notification that arrives while the app is open is also shown by the system. */
export type ForegroundPresentation = 'show' | 'silent';

/**
 * Push notifications: permission, the device token, categories with actions, and handlers.
 * Reports unsupported on Android in this gate (Android push needs a Firebase project and comes
 * with the relay).
 */
export interface PushApi {
  permission(): Promise<PushPermission>;
  /** Asks the system once. The app explains why before calling this. */
  requestPermission(): Promise<PushPermission>;
  deviceToken(): Promise<PushDeviceToken>;
  /** Replaces the registered categories with `categories`. */
  setCategories(categories: readonly PushCategory[]): Promise<void>;
  setForegroundPresentation(presentation: ForegroundPresentation): void;
  /** A notification arrived while the app was open. */
  onReceived(handler: Listener<PushNotification>): Unsubscribe;
  /** The owner tapped a notification or chose one of its actions. */
  onResponse(handler: Listener<PushResponse>): Unsubscribe;
  /** The response that opened the app, or `null` when it was opened another way. */
  launchResponse(): Promise<PushResponse | null>;
}

export type PushCapability = Capability<'push', PushApi>;
