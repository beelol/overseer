import * as Notifications from 'expo-notifications';

import { SUPPORTED, defineCapability, type Json } from '../capability';
import type {
  ForegroundPresentation,
  PushCapability,
  PushNotification,
  PushPermission,
  PushResponse,
} from '../capabilities/push';

function toPermission(status: Notifications.NotificationPermissionsStatus): PushPermission {
  const ios = status.ios?.status;
  if (ios === Notifications.IosAuthorizationStatus.PROVISIONAL) return 'provisional';
  if (ios === Notifications.IosAuthorizationStatus.EPHEMERAL) return 'provisional';
  if (status.granted) return 'granted';
  return status.canAskAgain && status.status === 'undetermined' ? 'undetermined' : 'denied';
}

function toJson(value: unknown): Json {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number') return Number.isFinite(value) ? value : null;
  if (Array.isArray(value)) return value.map(toJson);
  if (typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, toJson(item)]));
  }
  return null;
}

/**
 * What a notification carries besides its text. For a notification from Apple's service,
 * expo-notifications gives only the payload's `body` key as `content.data` (the shape of Expo's
 * own push service); the whole payload is in the trigger. Everything but `aps` is taken from
 * there, so a sender's own keys (the daemon's `overseer`) reach the app.
 */
function dataOf(request: Notifications.NotificationRequest): Json {
  const trigger = request.trigger as { type?: unknown; payload?: unknown } | null;
  const remote =
    trigger !== null && trigger.type === 'push' && typeof trigger.payload === 'object' && trigger.payload !== null
      ? Object.fromEntries(Object.entries(trigger.payload).filter(([key]) => key !== 'aps'))
      : {};
  return toJson({ ...remote, ...(request.content.data ?? {}) });
}

function toNotification(notification: Notifications.Notification): PushNotification {
  const { identifier, content } = notification.request;
  const data = dataOf(notification.request);
  return {
    id: identifier,
    title: content.title,
    body: content.body,
    categoryId: content.categoryIdentifier,
    data:
      typeof data === 'object' && data !== null && !Array.isArray(data)
        ? (data as Record<string, Json>)
        : {},
  };
}

function toResponse(response: Notifications.NotificationResponse): PushResponse {
  const tapped = response.actionIdentifier === Notifications.DEFAULT_ACTION_IDENTIFIER;
  return {
    notification: toNotification(response.notification),
    actionId: tapped ? null : response.actionIdentifier,
  };
}

function present(presentation: ForegroundPresentation): void {
  const show = presentation === 'show';
  Notifications.setNotificationHandler({
    handleNotification: async () => ({
      shouldShowBanner: show,
      shouldShowList: show,
      shouldPlaySound: show,
      shouldSetBadge: false,
    }),
  });
}

/** Apple's push service, through expo-notifications. */
export function createPush(): PushCapability {
  return defineCapability<PushCapability>('push', async () => SUPPORTED, {
    permission: async () => toPermission(await Notifications.getPermissionsAsync()),
    requestPermission: async () =>
      toPermission(
        await Notifications.requestPermissionsAsync({
          ios: { allowAlert: true, allowSound: true, allowBadge: false },
        }),
      ),
    async deviceToken() {
      const token = await Notifications.getDevicePushTokenAsync();
      return { service: 'apns', token: String(token.data) };
    },
    async setCategories(categories) {
      const wanted = new Set(categories.map((category) => category.id));
      for (const existing of await Notifications.getNotificationCategoriesAsync()) {
        if (!wanted.has(existing.identifier)) {
          await Notifications.deleteNotificationCategoryAsync(existing.identifier);
        }
      }
      for (const category of categories) {
        await Notifications.setNotificationCategoryAsync(
          category.id,
          category.actions.map((action) => ({
            identifier: action.id,
            buttonTitle: action.title,
            options: {
              isDestructive: action.destructive,
              isAuthenticationRequired: action.requiresUnlock,
              opensAppToForeground: action.opensApp,
            },
          })),
        );
      }
    },
    setForegroundPresentation: present,
    onReceived(handler) {
      const subscription = Notifications.addNotificationReceivedListener((notification) =>
        handler(toNotification(notification)),
      );
      return () => subscription.remove();
    },
    onResponse(handler) {
      const subscription = Notifications.addNotificationResponseReceivedListener((response) =>
        handler(toResponse(response)),
      );
      return () => subscription.remove();
    },
    async launchResponse() {
      const response = await Notifications.getLastNotificationResponseAsync();
      return response === null ? null : toResponse(response);
    },
  });
}
