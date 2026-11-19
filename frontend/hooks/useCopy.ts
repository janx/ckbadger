'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

/** How long a copy's answer stands: the copy button's `copied`, or its `not copied`. */
export const COPIED_MS = 1500;

/**
 * The one copy path (A4 review 3): the controls library's CopyButton copies through it, the one
 * control that copies a value (lane c2-finish, item 4: copy has one form), which every head's id
 * (HeadId's `copy`), a full id on its own line in tier 2, the cell page and the crash page draw.
 * `copy()` writes `text` and resolves whether it did.
 *
 * Its answer stands for COPIED_MS, on one timer, and a second copy starts it over: `copied` after a
 * copy that worked, `refused` after one that could not (lane c2-finish, rulings round 1: the copy
 * button draws its answer over the line under its box, so neither may stay). A copy that cannot
 * work claims nothing: where there is no clipboard (a page served over plain HTTP on a LAN address
 * has no `navigator.clipboard`) or the browser refuses, `copy()` resolves false, `copied` stays
 * false and nothing is thrown or left rejected. The timer goes with the component, and a copy that
 * finishes after the component has gone sets nothing.
 */
export function useCopy(text: string): {
  copied: boolean;
  refused: boolean;
  copy: () => Promise<boolean>;
} {
  const [answer, setAnswer] = useState<'copied' | 'refused' | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
    };
  }, []);
  const copy = useCallback(async () => {
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
    let done = false;
    if (clipboard) {
      try {
        await clipboard.writeText(text);
        done = true;
      } catch {
        done = false;
      }
    }
    if (!mounted.current) return done;
    if (timer.current !== null) clearTimeout(timer.current);
    setAnswer(done ? 'copied' : 'refused');
    timer.current = setTimeout(() => {
      timer.current = null;
      setAnswer(null);
    }, COPIED_MS);
    return done;
  }, [text]);
  return { copied: answer === 'copied', refused: answer === 'refused', copy };
}
