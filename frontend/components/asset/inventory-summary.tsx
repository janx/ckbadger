import type { InventorySummary } from '@/lib/api';
import { AssetCount } from '@/components/asset/inventory-table';
import { formatStandard } from '@/components/activity-event-row';
import { formatCompositionTier } from '@/lib/asset-utils';
import { formatBlockNumber } from '@/lib/format-block';
export interface InventorySummaryNoticeProps {
  summary: InventorySummary;
  assetType: 'token' | 'object' | 'identity';
}
export function InventorySummaryNotice({ summary, assetType }: InventorySummaryNoticeProps) {
  return (
    <section aria-label="Inventory totals" className="text-text-dim space-y-2 text-xs">
      <p>
        {assetType === 'token' ? (
          'Fungible balances, each identified by its full type hash.'
        ) : assetType === 'identity' ? (
          'Names and identities that can be held and moved on CKB.'
        ) : (
          <>
            <AssetCount value={summary.fullyOnchainObjectsCount} /> live objects have fully on-chain
            composition. <AssetCount value={summary.pendingItemsCount} /> live objects await DOB
            decoding.
          </>
        )}
      </p>
      {assetType === 'object' && (
        <>
          <dl className="flex flex-wrap gap-x-5 gap-y-2">
            {summary.objectStandards.map((s) => (
              <div key={s.standard}>
                <dt className="inline">{formatStandard(s.standard)} </dt>
                <dd className="inline">
                  <AssetCount value={s.collectionsCount} /> collections ·{' '}
                  <AssetCount value={s.liveCount} /> live items
                </dd>
              </div>
            ))}
          </dl>
          <details>
            <summary className="cursor-pointer">Composition by collection</summary>
            <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-2">
              {summary.objectComposition.map((c) => (
                <div key={c.tier}>
                  <dt>{formatCompositionTier(c.tier)}</dt>
                  <dd>
                    <AssetCount value={c.collectionsCount} /> collections ·{' '}
                    <AssetCount value={c.liveCount} /> live items
                  </dd>
                </div>
              ))}
            </dl>
            <p className="mt-2 text-[11px]">
              Items are grouped by their collection’s composition. Pending decoding is counted
              separately across live items.
            </p>
          </details>
        </>
      )}
      <p className="text-[11px]">
        Canonical asset totals
        {summary.anchor
          ? ` at block ${formatBlockNumber(summary.anchor.blockNumber)}`
          : ' · empty index'}
        . The list refreshes separately from a cached snapshot and can lag behind these totals.
      </p>
    </section>
  );
}
