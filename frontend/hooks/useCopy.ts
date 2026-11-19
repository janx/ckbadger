'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

/** How long a copy that worked shows that it did: HexCode's tick, the copy buttons' "copied". */
export const COPIED_MS = 1500;

/**
 * The one copy path (A4 review 3): the controls that copy a value and show it for a moment copy
 * through it (HexCode, HexDisplay, a head's HeadId, and the controls library's CopyButton, which
 * the address, transaction, cell and crash pages draw). The protocol evidence's copy, which says a
 * failure in words of its own, keeps its handler for its placement lane. `copy()` writes `text`
 * and resolves whether it did; `copied` is true for COPIED_MS after a copy that worked, and a
 * second copy starts that over.
 *
 * A copy that cannot work claims nothing: where there is no clipboard (a page served over plain
 * HTTP on a LAN address has no `navigator.clipboard`) or the browser refuses, `copy()` resolves
 * false, `copied` stays false and nothing is thrown or left rejected; CopyButton says `not copied`
 * from that boolean. The timer goes with the component, and a copy that finishes after the
 * component has gone sets nothing.
 */
export function useCopy(text: string): { copied: boolean; copy: () => Promise<boolean> } {
  const [copied, setCopied] = useState(false);
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
    if (!clipboard) return false;
    try {
      await clipboard.writeText(text);
    } catch {
      return false;
    }
    if (!mounted.current) return true;
    if (timer.current !== null) clearTimeout(timer.current);
    setCopied(true);
    timer.current = setTimeout(() => {
      timer.current = null;
      setCopied(false);
    }, COPIED_MS);
    return true;
  }, [text]);
  return { copied, copy };
}
