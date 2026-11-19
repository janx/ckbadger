'use client';
// F2 group `objects` (round 2): the pieces of the Object / m-NFT board heroes — one "Live" style
// for both pages (OB-15), the composition badge with its one plain sentence (OB-6, OB-M4), the
// readable traits linked to their genes (OB-5/6), and the life summary sentence (OB-6, OB-M5).
import { columnPx, fontPx, foreignTextPx } from '@/lib/type-metrics';
import type { ItemCellVersion } from '@/lib/api';
import { cellLockSentence, cellLockWords, lockWordsText } from '@/components/ui/lock-kind';
import { PartReadError, readPart } from '@/components/read-failure';
import type { CompositionTier } from '@/lib/asset-utils';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { isReadableTrait, traitDisplay, type DnaTrait } from '@/lib/dob-genes';
import { GENE_COLORS } from '@/lib/design-tokens';
import { cn, formatUtc8Date } from '@/lib/utils';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { formatCount, countNoun } from '@/lib/format-count';
import { ageSeconds, CLOCK_BEHIND } from '@/lib/format-age';
import { useClockText } from '@/components/ui/utc8-date';

/**
 * One status pill for an item page: jade while live, the reading ink once gone (OB-15; readability
 * R8: it was the quiet ink on a border near the ground, the head's weakest mark).
 */
export function LiveBadge({ live, gone }: { live: boolean; gone: 'Melted' | 'Burned' }) {
  return (
    <span
      className={cn(
        'text-meta border px-1.5',
        live ? 'text-jade border-jade/40' : 'text-text border-text-dim'
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

export function CompositionLine({
  tier,
  lede = false,
}: {
  tier: CompositionTier;
  /** Hierarchy v3 (R13): set as the page's lede — on the measure, in the reading ink (readability R3, R4). */
  lede?: boolean;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <CompositionBadge tier={tier} />
      <span className={cn('text-body', lede ? 'text-text max-w-measure' : 'text-text')}>
        {COMPOSITION_SENTENCES[tier]}
      </span>
    </div>
  );
}

/** The trait label column's most: half of a 320 px window's 288 px row, less the 12 px gap. */
const TRAIT_LABEL_CAP = (288 - 12) / 2;

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
  // Lane P2's third round (item 4): a value follows its label at one gap (12 px), left-aligned,
  // the label column as wide as the grid's longest label at the line's 14 px, with the swatch's
  // 8 px and its 8 px margin (it stood at the row's right edge, 600 px from its label at 1280).
  // Lane P2's fifth round (NOTE 5): a trait's name is the creator's text, measured by its bound
  // (`foreignTextPx`: a CJK grapheme 1.25 em, not the Latin 0.6), and the column is capped at half
  // of the narrowest row a window gives the traits (288 px at 320, less the 12 px gap: 138), where
  // a longer name breaks and its value keeps the rest
  const labelColumn = Math.min(
    TRAIT_LABEL_CAP,
    columnPx(Math.max(...shown.map((t) => foreignTextPx(t.name, fontPx('body')))) + 16)
  );
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
                'border-row-rule grid min-w-0 items-baseline gap-x-3 border-b py-2 transition-colors',
                lit && 'bg-base-surface'
              )}
              style={{ gridTemplateColumns: `${labelColumn}px minmax(0, 1fr)` }}
            >
              <dt className="text-text-dim flex min-w-0 items-center [overflow-wrap:anywhere]">
                <span
                  aria-hidden
                  className="mr-2 inline-block h-2 w-2 shrink-0"
                  style={{ background: color }}
                />
                {t.name}
              </dt>
              <dd
                className={cn(
                  'min-w-0 [overflow-wrap:anywhere]',
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
      <p className="text-text text-small max-w-measure">
        Each trait is read from the object’s DNA, below
        {/* PB-23: the dot goes with the hint; when the page's one hint is elsewhere, no dot */}
        <TapOrHoverHint what="one to find its gene" dotted className="text-meta" />
      </p>
    </div>
  );
}

/** A spelled-out duration for prose: "30 seconds", "5 minutes", "2 days". */
export function spelledDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) throw new Error(`invalid duration ${ms} ms`);
  const units: [number, string][] = [
    [365 * 86_400_000, 'year'],
    [30 * 86_400_000, 'month'],
    [86_400_000, 'day'],
    [3_600_000, 'hour'],
    [60_000, 'minute'],
    [1000, 'second'],
  ];
  for (const [size, unit] of units)
    if (ms >= size) {
      const n = Math.floor(ms / size);
      return formatCount(n, unit);
    }
  return 'under a second';
}

function timeOf(v: ItemCellVersion): number {
  if (!/^\d+$/.test(v.timestamp))
    throw new Error(`${v.txHash}:${v.outputIndex}: invalid cell timestamp ${v.timestamp}`);
  return Number(v.timestamp);
}

/**
 * Its life in one sentence, from its canonical cell versions (OB-6, OB-M5): minted when and under
 * which lock, how often its cell moved since, and where it sits now. As the page's lede
 * (`lede`, readability R8: what the object is and whether it is live, said in the head) it names
 * no figure, which the metrics row states (C7): no count of moves, no span, no block.
 */
export function itemLifeSentence(
  versions: readonly ItemCellVersion[],
  verb: 'Minted' | 'Issued' = 'Minted',
  by?: string,
  lede = false
): string {
  const first = versions[0];
  const last = versions.at(-1);
  if (!first || !last) throw new Error('item life sentence needs at least one cell version');
  // Lane P: the sentence tells the path, not the time — the dates are the page's Minted and Last
  // moved metrics (C7: the lede restates no metric), how long ago its hover (itemLifeAges).
  // the lock a cell sits under, through the one decision's sentence form (lane P2's follow-up,
  // item 6): never silent, a lock the registry does not name by its code hash
  const born = `${verb}${by ? ` by ${by}` : ''} to ${cellLockSentence(first, 'row')}`;
  const moves = versions.length - 1;
  if (!last.isLive) {
    const gone = last.consumedAtBlock;
    if (lede)
      return `${born}${moves ? ', then moved' : ''}; its last cell was consumed, so it is gone.`;
    return `${born}${moves ? `, moved ${moves} ${countNoun(moves, 'time')}` : ''}; its last cell was consumed${gone === null ? '' : ` at block ${gone.toLocaleString('en-US')}`}.`;
  }
  if (moves === 0) return `${born}, where it sits now.`;
  const now = cellLockSentence(last, 'row');
  // Two header times need not run forward (a block's timestamp only has to pass the median of the
  // 37 before it), so a move whose header time is earlier than the mint's is told without a span.
  const span = timeOf(last) - timeOf(first);
  const later = !lede && span >= 0 ? ` ${spelledDuration(span)} later` : '';
  return moves === 1
    ? `${born}, then moved${later} to ${now}, where it sits now.`
    : `${born}, then moved${lede ? '' : ` ${moves} times`}; the last move put it under ${now}, where it sits now.`;
}

/**
 * The life sentence's hover (lane P): how long ago the item was minted and, when its cell has
 * moved, how long ago it last moved — the ages the sentence itself dates.
 */
export function itemLifeAges(
  versions: readonly ItemCellVersion[],
  now: number,
  verb: 'Minted' | 'Issued' = 'Minted'
): string {
  const first = versions[0];
  const last = versions.at(-1);
  if (!first || !last) throw new Error('item life ages need at least one cell version');
  // The one age rule (format-age): a header time this device's clock has not reached by more than
  // consensus allows has no age.
  const ago = (label: string, version: ItemCellVersion) => {
    const age = ageSeconds(timeOf(version), now);
    return age.kind === 'clock-behind'
      ? `${label}: ${CLOCK_BEHIND}`
      : `${label} ${spelledDuration(age.seconds * 1000)} ago`;
  };
  const born = ago(verb.toLowerCase(), first);
  return versions.length > 1 ? `${born} · ${ago('last moved', last)}` : born;
}

/**
 * The life sentence with its ages as its hover (lane P), read from the page's one clock, so they
 * move with no data change (lane E4); the sentence itself names no time.
 */
export function ItemLife({
  versions,
  verb = 'Minted',
  by,
  lede = false,
}: {
  versions: readonly ItemCellVersion[];
  verb?: 'Minted' | 'Issued';
  by?: string;
  /** The page's lede (R8): its path and its state, no figure (`itemLifeSentence`). */
  lede?: boolean;
}) {
  const ages = useClockText((now) => itemLifeAges(versions, now, verb));
  // a lock the decision refuses fails the sentence in its place, said with the refusal (ruling D7)
  const read = readPart(() => itemLifeSentence(versions, verb, by, lede));
  if (read.error !== undefined)
    return (
      <PartReadError what="the item’s life" where="the lock it sits under" error={read.error} />
    );
  return <span title={ages}>{read.value}</span>;
}

// ── The cell over its life (OB-9, OB-M7) ──
const SHANNONS = BigInt(100000000);

/** Whole CKB when the amount is whole (cells of bytes), else exact. */
export function ckbText(shannons: bigint): string {
  if (shannons % SHANNONS === BigInt(0)) return (shannons / SHANNONS).toLocaleString('en-US');
  const whole = shannons / SHANNONS,
    frac = (shannons % SHANNONS).toString().padStart(8, '0').replace(/0+$/, '');
  return `${whole.toLocaleString('en-US')}.${frac}`;
}

/**
 * OB-9: what the cell held over its life, in the state it is in, every figure one the route sent
 * (lane P2's fifth round, item 2). The last version first: a live cell "holds" its CKB; a consumed
 * one "held" it "when it was consumed", and nothing is said in the present. With more than one
 * version, what minting (or issue) held, the size of each part that differs between the first
 * version and the last, both sizes as the versions state them, and over how many moves: no
 * difference the page computes (it said "so the cell grew by 13 CKB when it moved", which over
 * several moves named one move that did not happen alone).
 */
export function cellLifeSentence(
  versions: readonly ItemCellVersion[],
  noun: 'object' | 'token'
): string {
  const first = versions[0],
    last = versions.at(-1)!;
  const cap = BigInt(last.capacity),
    occupied = BigInt(last.occupiedCapacity);
  const live = last.isLive;
  const whose = live || versions.length === 1 ? `The ${noun}’s cell` : `The ${noun}’s last cell`;
  const occupiedPart = cap === occupied ? '' : `, ${ckbText(occupied)} of it occupied by its bytes`;
  const state = live
    ? `${whose} holds ${ckbText(cap)} CKB${occupiedPart}.`
    : `${whose} held ${ckbText(cap)} CKB${occupiedPart}${occupiedPart ? ',' : ''} when it was consumed.`;
  if (versions.length === 1) return state;
  const at = noun === 'token' ? 'At issue' : 'At minting';
  const moves = versions.length - 1;
  const parts = (['lock', 'type', 'data'] as const).filter(
    (part) => first.anatomy[`${part}Bytes`] !== last.anatomy[`${part}Bytes`]
  );
  const size = (v: ItemCellVersion, part: (typeof parts)[number]) =>
    formatCount(v.anatomy[`${part}Bytes`], 'byte');
  const then = parts.map((part) => `, its ${part} ${size(first, part)}`).join('');
  const after = parts.length
    ? `; after ${formatCount(moves, 'move')} ${parts
        .map(
          (part, k) =>
            `${k === 0 ? 'its' : 'and its'} ${part} ${live ? 'is' : 'was'} ${size(last, part)}`
        )
        .join(' ')}.`
    : live
      ? `; it has moved ${formatCount(moves, 'time')} since.`
      : `; it moved ${formatCount(moves, 'time')} before its last cell was consumed.`;
  return `${state} ${at} it held ${ckbText(BigInt(first.capacity))} CKB${then}${after}`;
}

/**
 * The lock footnote drawn (`lockFootnote`), nothing for one version; a lock the decision refuses
 * fails the footnote in its place, said with the refusal (ruling D7), never the page.
 */
export function LockFootnote({
  versions,
  noun,
  small = false,
}: {
  versions: readonly ItemCellVersion[];
  noun: 'object' | 'token';
  /** At the small step (an m-NFT's history), else at its prose's size. */
  small?: boolean;
}) {
  const read = readPart(() => lockFootnote(versions, noun));
  if (read.error !== undefined)
    return <PartReadError what="the lock footnote" where="its cell versions" error={read.error} />;
  // a statement of the lock (the reading ink, R3), running text inside the measure (R4)
  return read.value === null ? null : (
    <p className={small ? 'text-text text-small max-w-measure' : 'text-text max-w-measure'}>
      {read.value}
    </p>
  );
}

export function lockFootnote(
  versions: readonly ItemCellVersion[],
  noun: 'object' | 'token'
): string | null {
  const first = versions[0];
  if (versions.length < 2) return null;
  const args = (v: ItemCellVersion) => v.anatomy.lockBytes - 33;
  const sameType = versions.every(
    (v) => JSON.stringify(v.type) === JSON.stringify(first.type) && v.dataHash === first.dataHash
  );
  // the decision's words in a sentence, without an article (lane P2's follow-up, item 6)
  const words = (v: ItemCellVersion) => lockWordsText(cellLockWords(v), 'row');
  // a lock is its code hash, hash type and args (lane P2's third round, item 15: "throughout"
  // compared the first and the last version's code hash and args, so A → B → A read "throughout")
  const sameLock = (a: ItemCellVersion, b: ItemCellVersion) =>
    a.lock.codeHash === b.lock.codeHash &&
    a.lock.hashType === b.lock.hashType &&
    a.lock.args === b.lock.args;
  // the versions where the cell went under another lock, the first among them
  const runs = versions.filter((v, i) => i === 0 || !sameLock(v, versions[i - 1]));
  const last = versions.at(-1)!;
  const lock =
    runs.length === 1
      ? `Lock: ${words(first)} throughout.`
      : `Lock: ${runs
          .map((v, i) => {
            const named = `${words(v)} with ${
              // the same words over other args: another holder under the same kind of lock
              i > 0 && words(v) === words(runs[i - 1]) && v.lock.args !== runs[i - 1].lock.args
                ? 'other '
                : ''
            }${args(v)}-byte args`;
            if (i === 0) return `${named} ${noun === 'token' ? 'at issue' : 'at minting'}`;
            const day = formatUtc8Date(v.timestamp);
            if (i < runs.length - 1) return `${named} from ${day}`;
            return last.isLive ? `${named} since ${day}` : `${named} from ${day}, last`;
          })
          .join(' → ')}.`;
  return `${lock}${sameType ? ' Type and data never changed.' : ''}`;
}
