'use client';

import { useSyncExternalStore } from 'react';

/** The breakpoint from which a list keeps its own page size (Tailwind `md`). */
const QUERY = '(min-width: 768px)';

/** Rows per page below md (R2): a phone reads twenty, then pages on. */
export const PHONE_PAGE_SIZE = 20;

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener('change', onChange);
  return () => mql.removeEventListener('change', onChange);
}

const getSnapshot = () => window.matchMedia(QUERY).matches;

/**
 * A list's page size (R2): `pageSize` from md up, twenty below it — never more than the list's own
 * size, so a list of twelve stays twelve. Follows the viewport across the breakpoint; the API's
 * `limit` carries it, so a phone fetches twenty rows, not fifty.
 */
export function usePageSize(pageSize: number): number {
  if (!Number.isSafeInteger(pageSize) || pageSize <= 0)
    throw new Error(`usePageSize: invalid page size ${pageSize}`);
  const fromMd = useSyncExternalStore(subscribe, getSnapshot, () => true);
  return fromMd ? pageSize : Math.min(pageSize, PHONE_PAGE_SIZE);
}
