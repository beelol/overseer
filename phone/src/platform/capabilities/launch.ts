import type { Capability } from '../capability';

/**
 * The device as the gateway knows it (the `platform` of the protocol's hello).
 * This is data to report and to show. Shared code never branches on it: what differs between
 * platforms is asked of a capability instead, and a lint rule fails on a comparison with a
 * platform's name outside the platform layer.
 */
export interface LaunchDevice {
  readonly platform: 'ios' | 'android';
  /** The system's version as the system reports it, for example `26.5` or `15`. */
  readonly systemVersion: string;
  /** The model, for example `iPhone 17 Pro` or `sdk_gphone64_arm64`. */
  readonly model: string;
}

export interface LaunchRuntime {
  readonly engine: 'hermes' | 'other';
  /** React Native's New Architecture (Fabric and bridgeless mode). */
  readonly newArchitecture: boolean;
}

/** What each platform does its own way, as values a screen hands on without asking which. */
export interface LaunchConventions {
  /** How a screen makes room for the keyboard: iOS pads its bottom, Android resizes the window. */
  readonly keyboard: 'padding' | 'height';
  /** How a screen enters: iOS slides in from the side, Android fades up from the bottom. */
  readonly screenEnter: 'slide_from_right' | 'fade_from_bottom';
}

export interface LaunchInfo {
  readonly device: LaunchDevice;
  readonly conventions: LaunchConventions;
  /** An iOS simulator or an Android emulator, not a real phone. */
  readonly isSimulator: boolean;
  /**
   * Addresses the platform adds for reaching the Mac that runs the simulator:
   * `127.0.0.1` on the iOS simulator (it shares the Mac's network), `10.0.2.2` on the Android
   * emulator (its alias for the host's loopback). Empty on a real phone.
   */
  readonly hostAddresses: readonly string[];
  readonly runtime: LaunchRuntime;
}

/** What is known about the device and the build the moment the app starts. */
export interface LaunchApi {
  /** Fixed for the life of the process; returns the same object every time. */
  info(): LaunchInfo;
}

export type LaunchCapability = Capability<'launch', LaunchApi>;
