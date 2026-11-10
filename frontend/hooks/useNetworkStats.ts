'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';

/**
 * The chain vitals (`/statistics/network`), polled every 10 s: the one query behind the vitals
 * strip under the header, the status line at the foot of the window and the homepage's sync
 * banner. One key in the network's query client, so they share one fetch.
 */
export function useNetworkStats() {
  return useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });
}
