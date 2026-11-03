import type { InventorySummary } from '@/lib/api';
import { AssetCount } from '@/components/asset/inventory-table';
import { CompositionSet } from '@/components/ui/composition-set';
import { COMPOSITION_TIER_LABELS, type CompositionTier } from '@/lib/asset-utils';
import { BlockLink } from '@/components/ui/block-link';

export interface InventorySummaryNoticeProps {
  summary: InventorySummary;
  assetType: 'token' | 'object' | 'identity';
}
/** The quiet line under each view: what it lists and the canonical anchor of its totals. */
export function InventorySummaryNotice({ summary, assetType }: InventorySummaryNoticeProps) {
  return (
    <section aria-label="Inventory totals" className="text-text-dim text-small space-y-2">
      {assetType === 'identity' && (
        // F2 objects (IV-12): the Identities board's one line.
        <p>
          {summary.counts.identity.toLocaleString('en-US')} naming systems · every identity’s record
          lives in its cell on CKB
        </p>
      )}
      <p className="text-meta">
        Canonical asset totals
        {summary.anchor ? (
          <>
            {' '}
            at block <BlockLink number={summary.anchor.blockNumber} />
          </>
        ) : (
          ' · empty index'
        )}
        . The list refreshes separately from a cached snapshot and can lag behind these totals.
      </p>
    </section>
  );
}

// ── F2 objects (round 2, IV-09): the Objects view leads with composition. Consistency round C8:
// drawn by the shared CompositionSet ("N of M fully on-chain", the set bar, the five tiers with
// their counts, zeros kept dim); here the legend is the filter. ──
function isTier(value: string): value is CompositionTier {
  return value in COMPOSITION_TIER_LABELS;
}

export interface ObjectCompositionProps {
  summary: InventorySummary;
  selected: string | undefined;
  onSelect: (tier: string) => void;
}
/**
 * "N of M collections are fully on-chain", a share bar of collections by composition, and the
 * composition chips (every tier with its count) that filter the list.
 */
export function ObjectComposition({ summary, selected, onSelect }: ObjectCompositionProps) {
  const counts = Object.fromEntries(
    (Object.keys(COMPOSITION_TIER_LABELS) as CompositionTier[]).map((t) => [t, 0])
  ) as Record<CompositionTier, number>;
  for (const c of summary.objectComposition) {
    if (!isTier(c.tier)) throw new Error(`Inventory summary: unknown composition tier ${c.tier}`);
    if (counts[c.tier] !== 0)
      throw new Error(`Inventory summary: composition tier ${c.tier} listed twice`);
    counts[c.tier] = c.collectionsCount;
  }
  const total = Object.values(counts).reduce((n, c) => n + c, 0);
  if (total !== summary.counts.object)
    throw new Error(
      `Inventory summary: composition counts ${total} collections, object count is ${summary.counts.object}`
    );
  const standards = summary.objectStandards;
  const spore = standards.find((s) => s.standard === 'spore');
  const mnft = standards.find((s) => s.standard === 'm-nft');
  return (
    <section aria-label="Object composition" className="space-y-3">
      <CompositionSet
        counts={counts}
        total={total}
        noun="collections"
        label="Collections by composition"
        caption={
          <>
            so are{' '}
            <span className="text-text-bright">
              <AssetCount value={summary.fullyOnchainObjectsCount} />
            </span>{' '}
            live objects
            {summary.pendingItemsCount > 0 && (
              <>
                {' '}
                · <AssetCount value={summary.pendingItemsCount} /> await DOB decoding
              </>
            )}
          </>
        }
        aside={
          spore && mnft ? (
            <span className="text-small">
              <AssetCount value={spore.collectionsCount} /> Spore clusters ·{' '}
              <AssetCount value={mnft.collectionsCount} /> m-NFT classes
            </span>
          ) : null
        }
        filter={{ selected, onSelect, label: 'Filter by composition' }}
      />
    </section>
  );
}
