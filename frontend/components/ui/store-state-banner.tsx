'use client';

import { useQueryClient } from '@tanstack/react-query';
import { useSyncExternalStore } from 'react';
import type { DeriveProgress } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { getDerivePendingQueryError, getStoreCatchingUpQueryMessage } from '@/lib/query-client';

// Round 3 lane F: the two transient store states the API reports as 503 (contracts.md S, L0).
// Pages keep their loading state while these retry; these banners say what is being waited for.

const BANNER_CLASS = 'border-emphasis-dim/60 bg-base-surface/95 text-text-bright border-b py-2';

/** What each indexer-derived aggregate is, in the page's words. */
const AGGREGATE_NAMES: Record<DeriveProgress['aggregate'], string> = {
  dao_live_agg: 'DAO depositor totals',
  object_collection_activity_count: 'collection activity counts',
  bit_cell_by_account: '.bit Cell index',
};

/** One sentence for a derivation's state; exact counts only, nothing the indexer did not report. */
export function derivationSentence(progress: DeriveProgress): string {
  const name = AGGREGATE_NAMES[progress.aggregate];
  switch (progress.state) {
    case 'not_started':
      return `The indexer will derive the ${name} once its live sync starts.`;
    case 'outdated':
      return `The indexer is deriving the ${name} again: the stored one predates this version.`;
    case 'running': {
      if (progress.rowsDone === null || progress.derivedFromTip === null) {
        throw new Error(
          `derive_pending for ${progress.aggregate} is running without rowsDone / derivedFromTip`
        );
      }
      return `The indexer is deriving the ${name}: ${progress.rowsDone.toLocaleString('en-US')} source rows folded, from the chain at block ${formatBlockNumber(progress.derivedFromTip)}.`;
    }
  }
}

export function StoreCatchingUpBanner() {
  const queryClient = useQueryClient();
  const detail = useSyncExternalStore(
    (onStoreChange) => queryClient.getQueryCache().subscribe(onStoreChange),
    () => getStoreCatchingUpQueryMessage(queryClient),
    () => null
  );
  if (detail === null) return null;
  return (
    <div role="status" aria-label="Catching up" className={BANNER_CLASS}>
      <div className="text-small container mx-auto flex flex-wrap items-baseline gap-x-3 gap-y-1 font-mono">
        <span className="text-emphasis">Catching up</span>
        <span>Catching up with the indexer. Retrying automatically.</span>
        <span className="text-text-dim min-w-0 truncate">{detail}</span>
      </div>
    </div>
  );
}

export function DerivePendingBanner() {
  const queryClient = useQueryClient();
  const error = useSyncExternalStore(
    (onStoreChange) => queryClient.getQueryCache().subscribe(onStoreChange),
    () => getDerivePendingQueryError(queryClient),
    () => null
  );
  if (error === null) return null;
  return (
    <div role="status" aria-label="Being derived" className={BANNER_CLASS}>
      <div className="text-small container mx-auto flex flex-wrap items-baseline gap-x-3 gap-y-1 font-mono">
        <span className="text-emphasis">Being derived</span>
        {/* Without the optional `derive` field the API's own message is the whole statement. */}
        <span>{error.derive ? derivationSentence(error.derive) : error.apiMessage}</span>
        <span className="text-text-dim">Retrying automatically.</span>
      </div>
    </div>
  );
}
