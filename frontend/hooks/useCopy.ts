'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

/** How long a copy that worked shows that it did: HexCode's tick, the copy buttons' "copied". */
export const COPIED_MS = 1500;

/**
 * The one copy path (A4 review 3): the controls that copy a value and show it for a moment copy
 * through it: the controls library's CopyButton, which every head's id (HeadId's `copy`), a full
 * id on its own line in tier 2, the cell page and the crash page draw (lane c2-finish, item 4:
 * copy has one form), and the click-to-copy of HexCode and HexDisplay (`copyable`), which only the
 * .cell party's lock hash still reaches, an id inside a link, awaiting its ruling. `copy()` writes
 * `text` and resolves whether it did; `copied` is true for COPIED_MS after a copy that worked, and a
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
