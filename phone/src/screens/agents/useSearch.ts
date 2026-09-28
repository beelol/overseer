import { useEffect, useState } from 'react';

import { useSession, useSessionValue } from '@/session';
import { useTheme } from '@/theme';

/** As many as VS Code's search asks for. */
const LIMIT = 200;

const NONE: readonly string[] = Object.freeze([]);

/**
 * What the Mac finds for `query` (message text, files, status), as task ids. The list filters by
 * what the phone knows at once; this adds to it a moment after the typing stops. The same array
 * until an answer arrives.
 */
export function useSearch(query: string): readonly string[] {
  const session = useSession();
  const theme = useTheme();
  const online = useSessionValue((s) => s.connection === 'online');
  const [found, setFound] = useState<{ readonly query: string; readonly ids: readonly string[] }>({ query: '', ids: NONE });
  const asked = query.trim();
  const wait = theme.motion.duration.slow;

  useEffect(() => {
    if (!asked || !online) return;
    let current = true;
    const timer = setTimeout(() => {
      session.request('search', { query: asked, limit: LIMIT }).then(
        (answer) => {
          if (current) setFound({ query: asked, ids: answer.task_ids });
        },
        () => undefined, // What the phone knows is still searched.
      );
    }, wait);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [session, asked, online, wait]);

  return asked && found.query === asked ? found.ids : NONE;
}
