'use client';

import { useFromMd } from '@/hooks/useFromMd';

/** Rows per page below md (R2): a phone reads twenty, then pages on. */
export const PHONE_PAGE_SIZE = 20;

/**
 * A list's page size (R2): `pageSize` from md up, twenty below it — never more than the list's own
 * size, so a list of twelve stays twelve. Follows the viewport across the breakpoint; the API's
 * `limit` carries it, so a phone fetches twenty rows, not fifty.
 */
export function usePageSize(pageSize: number): number {
  if (!Number.isSafeInteger(pageSize) || pageSize <= 0)
    throw new Error(`usePageSize: invalid page size ${pageSize}`);
  const fromMd = useFromMd();
  return fromMd ? pageSize : Math.min(pageSize, PHONE_PAGE_SIZE);
}
