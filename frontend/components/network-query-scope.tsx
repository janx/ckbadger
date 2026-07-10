'use client';
import { useEffect, useRef, type ReactNode } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';

/** Clears the React Query cache when the active network changes, so a
 *  switched-away network's cached data is never served. */
export function NetworkQueryScope({ children }: { children: ReactNode }) {
  const network = useActiveNetwork();
  const queryClient = useQueryClient();
  const previousNetwork = useRef(network);
  useEffect(() => {
    if (previousNetwork.current !== network) {
      queryClient.clear();
      previousNetwork.current = network;
    }
  }, [network, queryClient]);
  return <>{children}</>;
}
