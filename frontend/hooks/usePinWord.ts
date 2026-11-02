'use client';

import { useSyncExternalStore } from 'react';

/** A touch screen is the primary pointer: the pin can only have come from a finger. */
const QUERY = '(pointer: coarse)';

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener('change', onChange);
  return () => mql.removeEventListener('change', onChange);
}

const getSnapshot = () => window.matchMedia(QUERY).matches;

/**
 * The word that introduces a pinned reading (consistency C11): "selected" on a pointer device —
 * a click is not a tap — and "tapped" only on a touch screen, where the hint's "tap" is the act.
 */
export function usePinWord(): 'selected' | 'tapped' {
  const coarse = useSyncExternalStore(subscribe, getSnapshot, () => false);
  return coarse ? 'tapped' : 'selected';
}
