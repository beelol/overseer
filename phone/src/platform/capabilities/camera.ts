import type { ComponentType } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';

import type { Capability } from '../capability';

export type CameraPermission = 'granted' | 'denied' | 'undetermined';

export interface CodeScannerProps {
  /** The camera runs only while this is true. */
  readonly active: boolean;
  /** Called with the text of each QR code seen. The caller decides whether it is a pairing code. */
  readonly onCode: (text: string) => void;
  /** What VoiceOver and TalkBack read for the camera view. */
  readonly accessibilityLabel: string;
  readonly style?: StyleProp<ViewStyle>;
}

/**
 * The camera, for scanning a pairing code. Simulators have no camera and report unsupported;
 * typing the code is the first-class path there.
 */
export interface CameraApi {
  permission(): Promise<CameraPermission>;
  /** Asks the system once. The app explains why before calling this. */
  requestPermission(): Promise<CameraPermission>;
  /**
   * The native camera view that reads QR codes. Render it only when `support()` reports
   * supported and the permission is granted.
   */
  readonly CodeScanner: ComponentType<CodeScannerProps>;
}

export type CameraCapability = Capability<'camera', CameraApi>;
