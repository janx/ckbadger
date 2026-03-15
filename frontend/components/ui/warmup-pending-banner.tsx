'use client';

import { useQueryClient } from '@tanstack/react-query';
import { useSyncExternalStore } from 'react';
import { getWarmupPendingQueryMessage, WARMUP_PENDING_BANNER_TEXT } from '@/lib/query-client';

export function WarmupPendingBanner() {
  const queryClient = useQueryClient();
  const detail = useSyncExternalStore(
    (onStoreChange) => queryClient.getQueryCache().subscribe(onStoreChange),
    () => getWarmupPendingQueryMessage(queryClient),
    () => null
  );

  if (!detail) {
    return null;
  }

  return (
    <div className="border-emphasis-dim/60 bg-base-surface/95 text-text-bright border-b px-4 py-2">
      <div className="container mx-auto flex items-center gap-3 font-mono text-xs">
        <span className="text-emphasis uppercase tracking-wider">Warmup</span>
        <span>{WARMUP_PENDING_BANNER_TEXT}</span>
        <span className="text-text-dim truncate">{detail}</span>
      </div>
    </div>
  );
}
