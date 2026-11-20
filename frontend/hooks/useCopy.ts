'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

/** How long a copy's answer stands: the copy button's `copied`, or its `not copied`. */
export const COPIED_MS = 1500;

/**
 * How long a standing answer goes before a second copy writes the same answer again (lane
 * cd-fixes-controls, L1 NOTE 13): a live region is heard when its text changes, and "copied"
 * written over "copied" changes nothing, so the second copy within COPIED_MS was silent. The answer
 * leaves for this long and comes back, and the region announces it again.
 */
export const ANNOUNCE_GAP_MS = 100;

/**
 * The one copy path (A4 review 3): the controls library's CopyButton copies through it, the one
 * control that copies a value (lane c2-finish, item 4: copy has one form), which every head's id
 * (HeadId's `copy`), a full id on its own line in tier 2, the cell page and the crash page draw.
 * `copy()` writes `text` and resolves whether it did.
 *
 * Its answer stands for COPIED_MS, on one timer, and a second copy starts it over: `copied` after a
 * copy that worked, `refused` after one that could not (lane c2-finish, rulings round 1: the copy
 * button draws its answer over the line under its box, so neither may stay). Where the same answer
 * still stands, the second copy takes it away for ANNOUNCE_GAP_MS and writes it again after, so it
 * is announced again (another answer is written at once: its words change); one timer at a time. A copy that cannot
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
  // the answer that stands, or waits to be written again (a timer runs for either)
  const shown = useRef<'copied' | 'refused' | null>(null);
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
    const next = done ? 'copied' : 'refused';
    const stand = () => {
      setAnswer(next);
      timer.current = setTimeout(() => {
        timer.current = null;
        shown.current = null;
        setAnswer(null);
      }, COPIED_MS);
    };
    if (timer.current !== null) clearTimeout(timer.current);
    if (shown.current === next) {
      setAnswer(null);
      timer.current = setTimeout(stand, ANNOUNCE_GAP_MS);
    } else stand();
    shown.current = next;
    return done;
  }, [text]);
  return { copied: answer === 'copied', refused: answer === 'refused', copy };
}
