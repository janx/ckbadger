'use client';
import { QueryClientProvider } from '@tanstack/react-query';
import { useEffect, useState, type ReactNode } from 'react';
import { NetworkInitializingBanner } from '@/components/ui/network-initializing-banner';
import { WarmupPendingBanner } from '@/components/ui/warmup-pending-banner';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { useRecentBlocksStore } from '@/hooks/useRecentBlocksStore';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import { createAppQueryClient } from '@/lib/query-client';

function ScopedQueryClient({ children }: { children: ReactNode }) {
  const [queryClient] = useState(() => createAppQueryClient());
  useEffect(
    () => () => {
      queryClient.clear();
      // The recent-blocks 24h series is a module-global store fed by this
      // network's API + realtime stream, so the per-network query cache reset
      // does not cover it: clear it with the scope it belongs to.
      useRecentBlocksStore.getState().reset();
      // Realtime block/transaction snapshots are module-global too. Reset them
      // at this keyed boundary even on routes that never mount useRealtimeData.
      useRealtimeStore.getState().reset();
    },
    [queryClient]
  );

  return (
    <QueryClientProvider client={queryClient}>
      <WarmupPendingBanner />
      <NetworkInitializingBanner />
      {children}
    </QueryClientProvider>
  );
}

/**
 * Gives each active-network mount its own query client. The key forces every
 * observer below this boundary to unmount before the next network's observers
 * are created, so network-neutral query keys can never remain attached to a
 * switched-away network's cache entry.
 */
export function NetworkQueryScope({ children }: { children: ReactNode }) {
  const network = useActiveNetwork();

  return <ScopedQueryClient key={network}>{children}</ScopedQueryClient>;
}
