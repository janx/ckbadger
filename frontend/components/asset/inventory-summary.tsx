import type { InventorySummary } from '@/lib/api';
import { AssetCount } from '@/components/asset/inventory-table';
import { CompositionBadge, CompositionMark } from '@/components/ui/composition-mark';
import {
  COMPOSITION_TIER_LABELS,
  formatCompositionTier,
  type CompositionTier,
} from '@/lib/asset-utils';
import { COMPOSITION_BADGES, COMPOSITION_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { cn } from '@/lib/utils';

export interface InventorySummaryNoticeProps {
  summary: InventorySummary;
  assetType: 'token' | 'object' | 'identity';
}
/** The quiet line under each view: what it lists and the canonical anchor of its totals. */
export function InventorySummaryNotice({ summary, assetType }: InventorySummaryNoticeProps) {
  return (
    <section aria-label="Inventory totals" className="text-text-dim space-y-2 text-xs">
      {assetType === 'identity' && (
        // F2 objects (IV-12): the Identities board's one line.
        <p>
          {summary.counts.identity.toLocaleString('en-US')} naming systems · every identity’s record
          lives in its cell on CKB
        </p>
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

// ── F2 objects (round 2, IV-09): the Objects view leads with composition ──
/** Bar order: the fully on-chain tiers first, then the off-chain ones (InventoryObjects board). */
const TIER_ORDER: readonly CompositionTier[] = [
  'btc_ckb',
  'pure_ckb',
  'decentralized_mixture',
  'unknown',
  'centralized_mixture',
  'decode_pending',
];
const OFFCHAIN_SHADE: Record<string, number> = {
  decentralized_mixture: 0.55,
  unknown: 0.35,
  centralized_mixture: 0.22,
  decode_pending: 0.15,
};

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
 * composition chips (badges for on-chain tiers, plain marks for the rest) with their counts.
 */
export function ObjectComposition({ summary, selected, onSelect }: ObjectCompositionProps) {
  const tiers = summary.objectComposition.map((c) => {
    if (!isTier(c.tier)) throw new Error(`Inventory summary: unknown composition tier ${c.tier}`);
    return { ...c, tier: c.tier };
  });
  const total = tiers.reduce((n, c) => n + c.collectionsCount, 0);
  if (total !== summary.counts.object)
    throw new Error(
      `Inventory summary: composition counts ${total} collections, object count is ${summary.counts.object}`
    );
  const onchain = tiers
    .filter((c) => c.tier === 'btc_ckb' || c.tier === 'pure_ckb')
    .reduce((n, c) => n + c.collectionsCount, 0);
  const ordered = TIER_ORDER.flatMap((t) => tiers.filter((c) => c.tier === t));
  const standards = summary.objectStandards;
  const spore = standards.find((s) => s.standard === 'spore');
  const mnft = standards.find((s) => s.standard === 'm-nft');
  return (
    <section aria-label="Object composition" className="space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <span className="text-text text-[13px]">
          <span className="text-text-bright text-[15px]">
            <AssetCount value={onchain} />
          </span>{' '}
          of <AssetCount value={total} /> collections are fully on-chain ·{' '}
          <span className="text-text-bright">
            <AssetCount value={summary.fullyOnchainObjectsCount} />
          </span>{' '}
          live objects are
          {summary.pendingItemsCount > 0 && (
            <>
              {' '}
              · <AssetCount value={summary.pendingItemsCount} /> await DOB decoding
            </>
          )}
        </span>
        {spore && mnft && (
          <span className="text-text-dim text-[12px]">
            <AssetCount value={spore.collectionsCount} /> Spore clusters ·{' '}
            <AssetCount value={mnft.collectionsCount} /> m-NFT classes
          </span>
        )}
      </div>
      <div
        role="img"
        aria-label={`Collections by composition: ${ordered.map((c) => `${formatCompositionTier(c.tier)} ${c.collectionsCount.toLocaleString('en-US')}`).join(', ')}`}
        className="bg-base-elevated flex h-2.5 gap-px overflow-hidden rounded"
      >
        {total > 0 &&
          ordered
            .filter((c) => c.collectionsCount > 0)
            .map((c) => (
              <span
                key={c.tier}
                data-tier-share={c.tier}
                className="h-full"
                style={{
                  width: `${(c.collectionsCount / total) * 100}%`,
                  minWidth: 2,
                  ...(c.tier === 'btc_ckb' || c.tier === 'pure_ckb'
                    ? { backgroundImage: COMPOSITION_BADGES[c.tier].bar }
                    : {
                        backgroundColor: COMPOSITION_COLORS.offchain,
                        opacity: OFFCHAIN_SHADE[c.tier],
                      }),
                }}
              />
            ))}
      </div>
      <div
        role="group"
        aria-label="Filter by composition"
        className="flex flex-wrap items-center gap-2"
      >
        <button
          type="button"
          aria-pressed={!selected}
          onClick={() => onSelect('')}
          className={cn(
            'rounded border px-3 py-1 text-xs',
            !selected ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'
          )}
        >
          All <AssetCount value={total} />
        </button>
        {ordered
          .filter((c) => c.collectionsCount > 0 || selected === c.tier)
          .map((c) => (
            <button
              key={c.tier}
              type="button"
              aria-pressed={selected === c.tier}
              onClick={() => onSelect(c.tier)}
              className={cn(
                'inline-flex items-center gap-1.5 rounded border px-2 py-1 text-xs',
                selected === c.tier
                  ? 'border-jade text-text-bright'
                  : 'border-base-border text-text-dim'
              )}
            >
              {c.tier === 'btc_ckb' || c.tier === 'pure_ckb' ? (
                <CompositionBadge tier={c.tier} size="sm" />
              ) : (
                <>
                  <CompositionMark tier={c.tier} size={12} />
                  {formatCompositionTier(c.tier)}
                </>
              )}
              <span className="text-text-dim">{c.collectionsCount.toLocaleString('en-US')}</span>
            </button>
          ))}
      </div>
    </section>
  );
}
