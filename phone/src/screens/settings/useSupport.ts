import { useEffect, useState } from 'react';

import type { Support } from '@/platform';

/** Whether a capability works here. `null` until it has answered; it never asks the owner anything. */
export function useSupport(capability: { support(): Promise<Support> }): Support | null {
  const [support, setSupport] = useState<Support | null>(null);
  useEffect(() => {
    let current = true;
    capability
      .support()
      .then((answer) => {
        if (current) setSupport(answer);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [capability]);
  return support;
}
