import { useEffect } from 'react';

import { defineCapability, type Support, unsupported } from '../capability';
import type { CameraCapability, CameraPermission, CodeScannerProps } from '../capabilities/camera';
import type { LaunchCapability } from '../capabilities/launch';
import { createFakeSupport, type FakeSupport } from './support';

export interface FakeCamera {
  readonly capability: CameraCapability;
  readonly support: FakeSupport;
  /** What the owner will answer when asked; `granted` unless a test changes it. */
  answerRequestWith(permission: CameraPermission): void;
  /** Holds a code in front of the camera: every active scanner on screen reads it. */
  show(text: string): void;
  /** How many scanners are on screen and active. */
  activeScanners(): number;
}

export function createFakeCamera(launch: LaunchCapability, initial?: Support): FakeCamera {
  const support = createFakeSupport(
    'camera',
    initial ??
      (launch.info().isSimulator ? unsupported('The fake simulator has no camera.') : undefined),
  );
  let permission: CameraPermission = 'undetermined';
  let answer: CameraPermission = 'granted';
  const scanners = new Set<(text: string) => void>();

  /** Draws nothing; while it is mounted and active it reads what `show` holds up. */
  function CodeScanner({ active, onCode }: CodeScannerProps) {
    useEffect(() => {
      if (!active) return undefined;
      const read = (text: string): void => onCode(text);
      scanners.add(read);
      return () => {
        scanners.delete(read);
      };
    }, [active, onCode]);
    return null;
  }

  const capability = defineCapability<CameraCapability>('camera', support.check, {
    async permission() {
      support.require();
      return permission;
    },
    async requestPermission() {
      support.require();
      if (permission === 'undetermined') permission = answer;
      return permission;
    },
    CodeScanner,
  });

  return {
    capability,
    support,
    answerRequestWith(next) {
      answer = next;
    },
    show(text) {
      for (const read of [...scanners]) read(text);
    },
    activeScanners: () => scanners.size,
  };
}
