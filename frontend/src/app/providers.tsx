'use client';

import { QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { ErrorBoundary } from '@/components/error-boundary';
import { NetworkInitializingBanner } from '@/components/ui/network-initializing-banner';
import { WarmupPendingBanner } from '@/components/ui/warmup-pending-banner';
import { createAppQueryClient } from '@/lib/query-client';

export function Providers({ children }: { children: React.ReactNode }) {
  const [queryClient] = useState(() => createAppQueryClient());

  return (
    <ErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <WarmupPendingBanner />
        <NetworkInitializingBanner />
        {children}
      </QueryClientProvider>
    </ErrorBoundary>
  );
}
