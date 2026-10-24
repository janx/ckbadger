'use client';

import { useEffect, useRef, useState } from 'react';

/**
 * The keys that appeared since the previous list, held for `holdMs` so a feed can flash its new
 * rows. The first list marks nothing new (there is nothing to compare it with).
 */
export function useFreshKeys(keys: string[] | null, holdMs = 2000): Set<string> {
  const previous = useRef<Set<string> | null>(null);
  const [fresh, setFresh] = useState<Set<string>>(() => new Set());
  // One string dependency: the effect runs when the list's content changes, not its identity.
  const joined = keys === null ? null : keys.join('\n');

  useEffect(() => {
    if (joined === null) return;
    const current = joined === '' ? [] : joined.split('\n');
    const before = previous.current;
    previous.current = new Set(current);
    if (before === null) return;
    const added = current.filter((k) => !before.has(k));
    if (added.length === 0) return;
    setFresh(new Set(added));
    const timer = setTimeout(() => setFresh(new Set()), holdMs);
    return () => clearTimeout(timer);
  }, [joined, holdMs]);

  return fresh;
}
