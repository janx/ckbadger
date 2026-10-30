'use client';
// F2 group `objects` (round 2): the pieces of the Object / m-NFT board heroes — one "Live" style
// for both pages (OB-15), the composition badge with its one plain sentence (OB-6, OB-M4), the
// readable traits linked to their genes (OB-5/6), and the life summary sentence (OB-6, OB-M5).
import type { ItemCellVersion } from '@/lib/api';
import type { CompositionTier } from '@/lib/asset-utils';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { isReadableTrait, traitDisplay, type DnaTrait } from '@/lib/dob-genes';
import { GENE_COLORS } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

/** One status pill for an item page: jade while live, grey once gone (OB-15). */
export function LiveBadge({ live, gone }: { live: boolean; gone: 'Melted' | 'Burned' }) {
  return (
    <span
      className={cn(
        'rounded border px-1.5 text-[11px]',
        live ? 'text-jade border-jade/40' : 'text-text-dim border-base-border'
      )}
    >
      {live ? 'Live' : gone}
    </span>
  );
}

/** What each composition tier means, in one sentence (Object / Mnft boards). */
export const COMPOSITION_SENTENCES: Record<CompositionTier, string> = {
  btc_ckb: 'Fully on-chain: its record lives in a CKB cell, its image in a Bitcoin inscription.',
  pure_ckb: 'Fully on-chain: its record and its content both live in CKB cells.',
  decentralized_mixture:
    'Partly off-chain: its content lives on decentralized storage (IPFS or Arweave), outside CKB.',
  centralized_mixture:
    'Partly off-chain: its content lives on a web server, which can change or disappear.',
  unknown: 'Its content references could not be classified, so its composition is unknown.',
  decode_pending: 'Its DOB has not been decoded yet, so its composition is not classified.',
};

export function CompositionLine({ tier }: { tier: CompositionTier }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <CompositionBadge tier={tier} />
      <span className="text-text text-[12.5px]">{COMPOSITION_SENTENCES[tier]}</span>
    </div>
  );
}

/**
 * The readable traits (OB-6): `prev.*` render hints and media values stay on the gel (OB-16).
 * Each row shares the hover state with its gene and its allele-gel lane (OB-5).
 */
export function ObjectTraits({
  traits,
  colors,
  active,
  onActive,
}: {
  traits: readonly DnaTrait[];
  colors: ReadonlyMap<string, string>;
  active: string | null;
  onActive: (gene: string | null) => void;
}) {
  const shown = traits.filter((t) => isReadableTrait(t.name, t.value));
  if (!shown.length) return null;
  return (
    <div className="space-y-2">
      <dl aria-label="Traits" className="grid gap-x-7 sm:grid-cols-2">
        {shown.map((t, i) => {
          const value = traitDisplay(t.value);
          const color = colors.get(t.name) ?? GENE_COLORS.shared;
          const lit = active === t.name;
          return (
            <div
              key={`${t.name}:${i}`}
              data-trait={t.name}
              data-active={lit ? 'true' : undefined}
              tabIndex={0}
              onMouseEnter={() => onActive(t.name)}
              onMouseLeave={() => onActive(null)}
              onFocus={() => onActive(t.name)}
              onBlur={() => onActive(null)}
              className={cn(
                'border-base-elevated flex min-w-0 justify-between gap-3 border-b py-2 outline-none transition-colors',
                lit && 'bg-base-surface'
              )}
            >
              <dt className="text-text-dim flex items-center">
                <span
                  aria-hidden
                  className="mr-2 inline-block h-2 w-2 shrink-0 rounded-sm"
                  style={{ background: color }}
                />
                {t.name}
              </dt>
              <dd
                className={cn(
                  'min-w-0 text-right [overflow-wrap:anywhere]',
                  value === 'empty' ? 'text-text-dim' : 'text-text-bright'
                )}
                style={lit && value !== 'empty' ? { color } : undefined}
              >
                {value}
              </dd>
            </div>
          );
        })}
      </dl>
      <p className="text-text-dim text-[11.5px]">
        Each trait is read from the object’s DNA, below · hover one to find its gene
      </p>
    </div>
  );
}

/** A spelled-out duration for prose: "30 seconds", "5 minutes", "2 days". */
export function spelledDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) throw new Error(`invalid duration ${ms} ms`);
  const units: [number, string][] = [
    [86_400_000, 'day'],
    [3_600_000, 'hour'],
    [60_000, 'minute'],
    [1000, 'second'],
  ];
  for (const [size, unit] of units)
    if (ms >= size) {
      const n = Math.floor(ms / size);
      return `${n} ${unit}${n === 1 ? '' : 's'}`;
    }
  return 'under a second';
}

export function lockPhrase(name: string | null): string {
  if (!name) return 'an unregistered lock';
  const article = /^[aeiou]/i.test(name) ? 'an' : 'a';
  return /lock$/i.test(name) ? `${article} ${name}` : `${article} ${name} lock`;
}

function timeOf(v: ItemCellVersion): number {
  if (!/^\d+$/.test(v.timestamp))
    throw new Error(`${v.txHash}:${v.outputIndex}: invalid cell timestamp ${v.timestamp}`);
  return Number(v.timestamp);
}

/**
 * Its life in one sentence, from its canonical cell versions (OB-6, OB-M5): minted when and under
 * which lock, how often its cell moved since, and where it sits now.
 */
export function itemLifeSentence(
  versions: readonly ItemCellVersion[],
  now: number,
  verb: 'Minted' | 'Issued' = 'Minted'
): string {
  const first = versions[0];
  const last = versions.at(-1);
  if (!first || !last) throw new Error('item life sentence needs at least one cell version');
  const born = `${verb} ${spelledDuration(now - timeOf(first))} ago to ${lockPhrase(first.lockName)}`;
  const moves = versions.length - 1;
  if (!last.isLive) {
    const gone = last.consumedAtBlock;
    return `${born}${moves ? `, moved ${moves} time${moves === 1 ? '' : 's'}` : ''}; its last cell was consumed${gone === null ? '' : ` at block ${gone.toLocaleString('en-US')}`}.`;
  }
  if (moves === 0) return `${born}, where it sits now.`;
  const later = spelledDuration(timeOf(last) - timeOf(first));
  return moves === 1
    ? `${born}, then moved ${later} later to ${lockPhrase(last.lockName)}, where it sits now.`
    : `${born}, then moved ${moves} times; the last move, ${spelledDuration(now - timeOf(last))} ago, put it under ${lockPhrase(last.lockName)}, where it sits now.`;
}
