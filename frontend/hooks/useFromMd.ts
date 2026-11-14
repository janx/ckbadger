'use client';

import { useSyncExternalStore } from 'react';

/** Tailwind's `md`: from here the site has its desktop arrangement, below it the phone's. */
const QUERY = '(min-width: 768px)';

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener('change', onChange);
  return () => mql.removeEventListener('change', onChange);
}

const getSnapshot = () => window.matchMedia(QUERY).matches;

/**
 * Whether the viewport is at least `md` wide, following it across the breakpoint. For what CSS
 * cannot switch: a page size the API is asked for, or the one place a control is drawn when the
 * phone puts it elsewhere. A layout CSS can do stays in CSS; what depends on the room a box is
 * given is measured (the search field's placeholder, a list's form).
 */
export function useFromMd(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, () => true);
}
