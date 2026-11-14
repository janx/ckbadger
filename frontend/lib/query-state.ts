import type { FetchStatus, QueryStatus } from '@tanstack/react-query';

/** The two fields of a query result (`useQuery`, `useInfiniteQuery`, `useQueries`) a page reads. */
export interface ReadState {
  status: QueryStatus;
  fetchStatus: FetchStatus;
}

/**
 * Whether a read is still on its way to its first answer: no answer yet (`status` 'pending') and
 * not idle (`fetchStatus` 'fetching' or 'paused'). The one test of "this read is awaited": a
 * page shows its loading form while it holds, and says "no …", "unavailable" or "could not …"
 * only once it does not.
 *
 * Why not the library's own flags (TanStack Query 5.90, query-core `queryObserver.ts`
 * `createResult`): `isLoading` is `isPending && isFetching`, with `isFetching` =
 * `fetchStatus === 'fetching'`, so it is false while the read is paused (`fetchStatus`
 * 'paused': the browser offline, or a hidden tab between retries); `isPending` is
 * `status === 'pending'`, which a disabled read (`enabled: false`, `fetchStatus` 'idle') also
 * is, forever; `isPaused` is `fetchStatus === 'paused'`, also true for a refetch of a read that
 * has answered. This is `isLoading || (isPending && isPaused)`.
 */
export function isAwaited(read: ReadState): boolean {
  return read.status === 'pending' && read.fetchStatus !== 'idle';
}
