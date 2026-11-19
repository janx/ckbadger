'use client';

import { useSyncExternalStore } from 'react';

/**
 * Tailwind's `sm`, `md` and `lg` as media queries: below `sm` a phone (an id's tight size, a
 * control's 32 px box); from `md` the site has its desktop arrangement, below it the phone's; from
 * `lg` the page's gutter and the header's spacing widen.
 */
const QUERIES = {
  sm: '(min-width: 640px)',
  md: '(min-width: 768px)',
  lg: '(min-width: 1024px)',
} as const;

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

const FROM = { sm: viewportFrom('sm'), md: viewportFrom('md'), lg: viewportFrom('lg') };

/**
 * Whether the viewport is at least `sm` wide (640 px), following it across the breakpoint: below
 * it a phone. For what a component decides in script at the width its classes switch at: an id's
 * size (`useTightBelowSm`), a control's phone form (the controls of `components/ui/`: a tab row's
 * step, more than four filter values drawn as a select).
 */
export function useFromSm(): boolean {
  return useSyncExternalStore(FROM.sm.subscribe, FROM.sm.getSnapshot, () => true);
}

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
