'use client';

import { useSyncExternalStore } from 'react';
import type { ShortSize } from '@/lib/shorten';

/** Tailwind's `sm`: below it a phone. */
const QUERY = '(min-width: 640px)';

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener('change', onChange);
  return () => mql.removeEventListener('change', onChange);
}

const getSnapshot = () => window.matchMedia(QUERY).matches;

/**
 * The size of an id in a place that is narrow only on a phone (R9, `docs/RESPONSIVE_LAYOUT.md`,
 * "Formats": `tight` only below `sm`): a list's phone row that is also drawn in a desktop column
 * (a holders list in its column at 1280, a list whose table needs more than its container), and a
 * tile under 200 px only on a phone. Below `sm` the tight size; from `sm` the row size, the form
 * the page's tables and prose give the same id. Follows the viewport across the breakpoint; the
 * row size before there is a viewport to ask.
 */
export function useTightBelowSm(): ShortSize {
  const fromSm = useSyncExternalStore(subscribe, getSnapshot, () => true);
  return fromSm ? 'row' : 'tight';
}
