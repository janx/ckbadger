'use client';

import { useSyncExternalStore } from 'react';

/**
 * Tailwind's `md` and `lg` as media queries: from `md` the site has its desktop arrangement, below
 * it the phone's; from `lg` the page's gutter and the header's spacing widen.
 */
const QUERIES = { md: '(min-width: 768px)', lg: '(min-width: 1024px)' } as const;

type Breakpoint = keyof typeof QUERIES;

/** A breakpoint's subscription and snapshot, made once so useSyncExternalStore keeps them. */
function viewportFrom(breakpoint: Breakpoint) {
  const query = QUERIES[breakpoint];
  return {
    subscribe(onChange: () => void): () => void {
      const mql = window.matchMedia(query);
      mql.addEventListener('change', onChange);
      return () => mql.removeEventListener('change', onChange);
    },
    getSnapshot: () => window.matchMedia(query).matches,
  };
}

const FROM = { md: viewportFrom('md'), lg: viewportFrom('lg') };

/**
 * Whether the viewport is at least `md` wide, following it across the breakpoint. For what CSS
 * cannot switch: a page size the API is asked for, or the one place a control is drawn when the
 * phone puts it elsewhere. A layout CSS can do stays in CSS; what depends on the room a box is
 * given is measured (the search field's placeholder, a list's form).
 */
export function useFromMd(): boolean {
  return useSyncExternalStore(FROM.md.subscribe, FROM.md.getSnapshot, () => true);
}

/**
 * Whether the viewport is at least `lg` wide: for a budget of a box whose spacing the `lg:`
 * classes widen (the header's nav, 24 px between its entries from lg, 16 below).
 */
export function useFromLg(): boolean {
  return useSyncExternalStore(FROM.lg.subscribe, FROM.lg.getSnapshot, () => true);
}
