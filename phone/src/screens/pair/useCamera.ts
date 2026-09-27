import { useCallback, useEffect, useState } from 'react';

import { useCapabilities, type CameraPermission } from '@/platform';

import { useSupport } from '../settings/useSupport';

/**
 * Where the camera stands: not known yet, not there (a simulator), there and to be asked for,
 * on, or refused by the owner.
 */
export type CameraState = 'checking' | 'none' | 'ask' | 'on' | 'off';

/** The camera for scanning, and the way to ask the system for it. Looking never asks. */
export function useCamera(): { readonly state: CameraState; readonly allow: () => void } {
  const { camera } = useCapabilities();
  const support = useSupport(camera);
  const [permission, setPermission] = useState<CameraPermission | null>(null);

  useEffect(() => {
    if (!support?.supported) return undefined;
    let current = true;
    camera
      .permission()
      .then((answer) => {
        if (current) setPermission(answer);
      })
      .catch(() => {
        if (current) setPermission('denied');
      });
    return () => {
      current = false;
    };
  }, [support, camera]);

  const allow = useCallback(() => {
    camera
      .requestPermission()
      .then(setPermission)
      .catch(() => setPermission('denied'));
  }, [camera]);

  if (support === null) return { state: 'checking', allow };
  if (!support.supported) return { state: 'none', allow };
  if (permission === null) return { state: 'checking', allow };
  return {
    state: permission === 'granted' ? 'on' : permission === 'denied' ? 'off' : 'ask',
    allow,
  };
}
