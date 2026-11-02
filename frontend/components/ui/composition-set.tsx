import type { ReactNode } from 'react';
import type { AddressObjectComposition, ObjectCollection } from '@/lib/api';
import { COMPOSITION_TIER_LABELS, type CompositionTier } from '@/lib/asset-utils';
import { COMPOSITION_BADGES, COMPOSITION_COLORS } from '@/lib/design-tokens';
import { CompositionMark } from '@/components/ui/composition-mark';
import { cn } from '@/lib/utils';

// Consistency round C8 (Main.dc.html, Address.dc.html): what a set of objects is made of, on the
// permanence scale, is first-class information — "N of M fully on-chain", one 8 px set bar and the
// five-tier legend with every tier's count (a zero tier stays, in the ghost ink). One drawing for
// the inventory, a collection's Objects head and an address's Holdings head; the per-row chips
// stay as they are.

/** How many members of the set sit in each tier (every tier present, zeros included). */
export type CompositionCounts = Readonly<Record<CompositionTier, number>>;

/** Legend order (the boards): the two fully on-chain tiers, then the off-chain ones. */
const LEGEND: readonly CompositionTier[] = [
  'pure_ckb',
  'btc_ckb',
  'decentralized_mixture',
  'centralized_mixture',
  'unknown',
];
/** Bar order (InventoryObjects board): the fully on-chain tiers first, BTC+CKB leading. */
const BAR: readonly CompositionTier[] = [
  'btc_ckb',
  'pure_ckb',
  'decentralized_mixture',
  'unknown',
  'centralized_mixture',
  'decode_pending',
];
/** The off-chain tiers share the dim ink, told apart by how much of it shows. */
const OFFCHAIN_SHADE: Record<string, number> = {
  decentralized_mixture: 0.55,
  unknown: 0.35,
  centralized_mixture: 0.22,
  decode_pending: 0.15,
};
const onchain = (tier: CompositionTier) => tier === 'btc_ckb' || tier === 'pure_ckb';
const n = (v: number) => v.toLocaleString('en-US');

/** The address holdings payload (camelCase keys) as tier counts. */
export function compositionCountsFromAddress(c: AddressObjectComposition): CompositionCounts {
  return {
    pure_ckb: c.pureCkb,
    btc_ckb: c.btcCkb,
    decentralized_mixture: c.decentralizedMixture,
    centralized_mixture: c.centralizedMixture,
    unknown: c.unknown,
    decode_pending: c.decodePending,
  };
}

/**
 * A collection's composition as tier counts. The API states the on-chain count (Pure CKB +
 * BTC+CKB) and every other tier; an object still awaiting decoding is counted as unknown there.
 */
export function compositionCountsFromCollection(
  c: NonNullable<ObjectCollection['composition']>
): CompositionCounts {
  if (c.onchainCount < c.pureCkbCount)
    throw new Error(
      `collection composition: on-chain count ${c.onchainCount} is below its Pure CKB count ${c.pureCkbCount}`
    );
  return {
    pure_ckb: c.pureCkbCount,
    btc_ckb: c.onchainCount - c.pureCkbCount,
    decentralized_mixture: c.decentralizedMixtureCount,
    centralized_mixture: c.centralizedMixtureCount,
    unknown: c.unknownCount,
    decode_pending: 0,
  };
}

/** The sentence after "fully on-chain" when the caller gives none: where the parts live. */
function defaultCaption(counts: CompositionCounts): string | null {
  const on = counts.btc_ckb + counts.pure_ckb;
  if (on === 0) return null;
  if (counts.pure_ckb === 0) return 'records in CKB cells, pictures in Bitcoin inscriptions';
  if (counts.btc_ckb === 0) return 'records and content in CKB cells';
  return 'records in CKB cells; pictures in CKB cells or Bitcoin inscriptions';
}

export interface CompositionSetProps {
  counts: CompositionCounts;
  /** Members of the set; the counts must add up to it. */
  total: number;
  /** What the set is made of: `objects` (default) or `collections` (the inventory). */
  noun?: string;
  /** The dim sentence after "fully on-chain"; default: where the on-chain parts live. */
  caption?: ReactNode;
  /** The group's accessible name. */
  label?: string;
  /** The right-hand meta word on the first line. */
  aside?: ReactNode;
  /** The inventory filters by tier: the legend becomes the filter chips, "All" first. */
  filter?: {
    selected: string | undefined;
    onSelect: (tier: string) => void;
    label: string;
  };
  className?: string;
}

export function CompositionSet({
  counts,
  total,
  noun = 'objects',
  caption,
  label = 'Object composition',
  aside = 'composition',
  filter,
  className,
}: CompositionSetProps) {
  const sum = (Object.keys(COMPOSITION_TIER_LABELS) as CompositionTier[]).reduce((s, t) => {
    const v = counts[t];
    if (!Number.isSafeInteger(v) || v < 0)
      throw new Error(`${label}: composition count for ${t} is ${v}`);
    return s + v;
  }, 0);
  if (sum !== total)
    throw new Error(`${label}: composition counts ${n(sum)} ${noun}, the set has ${n(total)}`);
  const fully = counts.btc_ckb + counts.pure_ckb;
  const legend = counts.decode_pending > 0 ? [...LEGEND, 'decode_pending' as const] : LEGEND;
  const shares = BAR.filter((t) => counts[t] > 0);
  const said = caption === undefined ? defaultCaption(counts) : caption;
  const entry = (tier: CompositionTier) => {
    const v = counts[tier];
    const zero = v === 0;
    return (
      <>
        <CompositionMark tier={tier} size={16} className={cn(!onchain(tier) && 'opacity-50')} />
        <span
          className={cn(zero ? 'text-text-ghost' : onchain(tier) ? 'font-medium' : 'text-text')}
          style={
            !zero && onchain(tier)
              ? {
                  backgroundImage: COMPOSITION_BADGES[tier as 'btc_ckb' | 'pure_ckb'].ink,
                  WebkitBackgroundClip: 'text',
                  backgroundClip: 'text',
                  color: 'transparent',
                }
              : undefined
          }
        >
          {COMPOSITION_TIER_LABELS[tier]}
        </span>{' '}
        <span
          data-count
          className={cn('tabular-nums', zero ? 'text-text-ghost' : 'text-text-bright')}
        >
          {n(v)}
        </span>
      </>
    );
  };
  return (
    <div role="group" aria-label={label} className={cn('min-w-0', className)}>
      <div className="text-body flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1.5">
        <span>
          <span className="text-text-bright font-medium tabular-nums">
            {n(fully)} of {n(total)}
          </span>{' '}
          <span className="text-text">
            {noun === 'objects' ? '' : `${noun} are `}fully on-chain
          </span>
          {said && <span className="text-text-dim"> · {said}</span>}
        </span>
        {aside && <span className="text-text-dim text-meta">{aside}</span>}
      </div>
      <div
        role="img"
        aria-label={`Composition of the ${n(total)} ${noun}: ${
          shares.length
            ? shares.map((t) => `${COMPOSITION_TIER_LABELS[t]} ${n(counts[t])}`).join(', ')
            : 'none'
        }`}
        className="bg-base-surface mt-2 flex h-2 gap-0.5 overflow-hidden rounded"
      >
        {total > 0 &&
          shares.map((t) => (
            <span
              key={t}
              data-tier-share={t}
              className="h-full"
              style={{
                width: `${(counts[t] / total) * 100}%`,
                minWidth: 2,
                ...(onchain(t)
                  ? { backgroundImage: COMPOSITION_BADGES[t as 'btc_ckb' | 'pure_ckb'].bar }
                  : { backgroundColor: COMPOSITION_COLORS.offchain, opacity: OFFCHAIN_SHADE[t] }),
              }}
            />
          ))}
      </div>
      {filter ? (
        <div
          role="group"
          aria-label={filter.label}
          className="text-small mt-2 flex flex-wrap items-center gap-x-1.5 gap-y-1.5"
        >
          <button
            type="button"
            aria-pressed={!filter.selected}
            onClick={() => filter.onSelect('')}
            className={cn(
              'inline-flex h-7 items-center gap-1.5 rounded border px-2.5',
              !filter.selected ? 'border-jade text-text-bright' : 'border-base-border text-text-dim'
            )}
          >
            All <span className="text-text-bright tabular-nums">{n(total)}</span>
          </button>
          {legend.map((tier) => (
            <button
              key={tier}
              type="button"
              data-tier={tier}
              data-zero={counts[tier] === 0 || undefined}
              aria-pressed={filter.selected === tier}
              disabled={counts[tier] === 0 && filter.selected !== tier}
              onClick={() => filter.onSelect(tier)}
              className={cn(
                'inline-flex h-7 items-center gap-1.5 rounded border px-2.5 disabled:cursor-default',
                filter.selected === tier ? 'border-jade' : 'border-base-border'
              )}
            >
              {entry(tier)}
            </button>
          ))}
        </div>
      ) : (
        <ul className="text-small text-text-dim mt-2 flex flex-wrap items-center gap-x-[22px] gap-y-1.5">
          {legend.map((tier) => (
            <li
              key={tier}
              data-tier={tier}
              data-zero={counts[tier] === 0 || undefined}
              className="inline-flex items-center gap-1.5"
            >
              {entry(tier)}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
