import { type Asset } from '@/lib/api';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ClassMark } from '@/components/ui/class-mark';
import { formatStandard } from '@/components/activity-event-row';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { exactUnsignedInteger, formatDecimal } from '@/lib/exact-integer';
import { ckbAmountText } from '@/lib/amount-form';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { RowTag } from '@/components/ui/row-tag';
import {
  getClusterDetailHref,
  getIdentityCollectionHref,
  getMnftClassDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { formatCompositionTier } from '@/lib/asset-utils';
import type { CSSProperties, ReactNode } from 'react';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';
import { percentOf } from '@/lib/format-unit';
import { groupCount } from '@/lib/format-count';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { UNAVAILABLE, WIDEST_CKB_HELD, WIDEST_ROW_COUNT } from '@/lib/list-bounds';

/**
 * What every inventory column holds, measured at its step (readability F3-e, lib/list-bounds.ts):
 * names at the body step (14 px), cells at the small (13), tags, heads and notes at the meta (12),
 * and the 16 px gap after every column but the last.
 */
const name = (chars: number) => textPx(chars, fontPx('body'));
const cell = (text: string) => textPx(text.length, fontPx('small'));
const meta = (text: string) => textPx(text.length, fontPx('meta'));
/** A row tag (`RowTag`): its words at the meta step, 5 px sides and a 1 px border each side. */
const tag = (text: string) => meta(text) + 2 * 5 + 2 * 1;
/**
 * A name column's least share of the name, 12 characters at 14 px: the column takes what the
 * container gives beyond the table's sum, and a longer name ends in the one ellipsis there, the
 * row's title holding it whole.
 */
const NAME_CHARS = 12;
/** A count (holders, transfers, live, registered), or "Unavailable" before the API derives it. */
const COUNT_PX = columnPx(Math.max(cell(WIDEST_ROW_COUNT), cell(UNAVAILABLE)) + CELL_GAP_PX);
/** The H-multiplier to 2 dp ("×1.64"), or "Unavailable": the last column, no gap after it. */
const H_MULT_PX = columnPx(cell(UNAVAILABLE));

export function AssetCount({ value }: { value: number | string | null | undefined }) {
  if (value === null || value === undefined)
    return <span className="text-text-dim">Unavailable</span>;
  if (typeof value === 'number' && (!Number.isSafeInteger(value) || value < 0))
    throw new Error(`Invalid asset count ${value}`);
  return <>{exactUnsignedInteger(String(value), 'asset count').toLocaleString('en-US')}</>;
}
export function assetHref(asset: Asset) {
  if (asset.assetType === 'token') return getTokenDetailHref(asset.id);
  if (asset.assetType === 'identity') return getIdentityCollectionHref(asset.standard, asset.id);
  if (asset.standard === 'spore') return getClusterDetailHref(asset.clusterId ?? asset.id);
  if (asset.standard === 'm-nft') return getMnftClassDetailHref(asset.id);
  return getObjectDetailHref(asset.id);
}
export interface InventoryRowsProps {
  rows: readonly Asset[];
  assetType: Asset['assetType'];
}
// ── F2 objects (round 2): the Inventory board's rows ──
/**
 * Occupied capacity in a row: the summary form (R5), whole CKB from 1,000 up (it is bytes, so
 * mostly whole), an amount with a fraction cut and titled with its exact value (IV-06).
 */
function OccupiedCkb({ shannons }: { shannons: bigint }) {
  return (
    <CkbAmount form="summary" shannons={shannons} signed={false} tone="inherit" unit={false} />
  );
}
/** The thumbnail frame of a collection row: crystal / copper for the fully on-chain tiers (IV-10). */
function thumbFrame(tier: Asset['compositionTier']): CSSProperties | undefined {
  if (tier !== 'pure_ckb' && tier !== 'btc_ckb') return undefined;
  return {
    backgroundImage: COMPOSITION_BADGES[tier].frame,
    backgroundClip: 'padding-box, border-box',
    backgroundOrigin: 'padding-box, border-box',
    border: '1px solid transparent',
  };
}
/** The occupied CKB of a phone row's quiet line, with its share of what the cells hold. */
function occupiedPart(a: Asset): ReactNode {
  const { held, occupied } = heldAndOccupied(a);
  if (occupied === null) return 'occupied unavailable';
  return (
    <span key="occupied" className="whitespace-nowrap">
      <Labelled label="occupied" after>
        <CkbAmount form="summary" shannons={occupied} signed={false} tone="plain" />
      </Labelled>
      {held !== null && held > BigInt(0) && ` (${percentOf(occupied, held)})`}
    </span>
  );
}

/** An exact count and its noun, singular for one ("1 holder", "93 objects"): a named figure. */
function Counted({
  value,
  noun,
  one = noun,
}: {
  value: number | string | null | undefined;
  noun: string;
  one?: string;
}) {
  return (
    <Labelled label={value === 1 || value === '1' ? one : noun} after>
      <AssetCount value={value} />
    </Labelled>
  );
}

/** The H-multiplier, named, to 2 dp and grouped as the table writes it (PA-09). */
function hMultiplierPart(a: Asset): ReactNode {
  return (
    <Labelled key="hm" label="H-mult.">
      <HMultiplier value={a.hMultiplier} />
    </Labelled>
  );
}

/**
 * Line 1 of an inventory row on a phone (the phone-rows board's head(): 287 of 328 px there for
 * "Wrapped CKB" and its tag beside "30,879,240 CKB"): the class's mark and its 6 px (18), a name of
 * 18 characters at the key's 14 px (151.2), the 12 px between the parts and the widest CKB a row's
 * cells hold at the key's step, "99,999,999,999" (WIDEST_CKB_HELD, 117.6) and " CKB" at the unit's
 * 12 px (28.8): 327.6 of a 360 px phone's 328 px line, 357.6 at 390 leaving 21 characters. The
 * tags stand on line 2, where the board drew the symbol's beside the name: beside the widest figure
 * a name and its tags would have 18 characters between them. A longer name drops the widest figure
 * under it, whole and right-aligned (PhoneRow's rule).
 */
const INVENTORY_PHONE_HEAD_PX = phoneHead(
  'inventory',
  12 +
    6 +
    textPx(18, fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(WIDEST_CKB_HELD.length, fontPx('body')) +
    textPx(' CKB'.length, fontPx('meta'))
);

const INVENTORY_MARK = { token: 'tokens', object: 'objects', identity: 'identities' } as const;

/**
 * An inventory row on a phone (the phone-rows board, its tokens pane; objects and identities by its
 * rules): the class's mark, the name and the CKB its cells hold, the figure the list sorts by; what
 * it is and who uses it, named figures that wrap between them, an object collection's composition
 * chip held at the line's right edge (the owner's rule: on every row of an object list); the rest,
 * quiet. The cards from 560 px take the same three parts.
 */
function InventoryPhoneRow({ asset: a }: { asset: Asset }) {
  const standard = formatStandard(a.standard);
  const { held } = heldAndOccupied(a);
  const identity = {
    key: 'identity',
    tone: 'head' as const,
    left: (
      <>
        {/* its 12 px written as a class too, so the first line's measure reads it */}
        <ClassMark kind={INVENTORY_MARK[a.assetType]} size={12} className="mr-1.5 h-3 w-3" />
        <AppLink href={assetHref(a)} look="key" className="font-medium">
          {(a.assetType === 'token' ? (a.name ?? a.symbol) : a.name) || (
            <Hash hash={a.id} size="tight" className="text-small" />
          )}
        </AppLink>
      </>
    ),
    right: (
      <span className="text-body text-text-bright">
        {held === null ? (
          'CKB held unavailable'
        ) : (
          <CkbAmount form="summary" shannons={held} signed={false} tone="plain" />
        )}
      </span>
    ),
  };
  if (a.assetType === 'token') {
    const supply = (
      <Labelled key="supply" label="supply">
        {a.totalSupply === null ? (
          'unavailable'
        ) : (
          <TokenAmount form="summary" amount={a.totalSupply} decimals={a.decimals} />
        )}
        {a.symbol && ` ${a.symbol}`}
      </Labelled>
    );
    return (
      <PhoneRow
        testId="inventory-row-mobile"
        head={INVENTORY_PHONE_HEAD_PX}
        lines={[
          identity,
          {
            key: 'what',
            tone: 'body',
            left: (
              <Dotted
                parts={[
                  a.name && a.symbol && <RowTag key="symbol">{a.symbol}</RowTag>,
                  a.standard !== 'xudt' && a.standard !== 'xudt_compatible' && (
                    <RowTag key="standard">{standard}</RowTag>
                  ),
                  <Counted key="h" value={a.holdersCount} noun="holders" one="holder" />,
                  <Counted key="t" value={a.transfersCount} noun="transfers" one="transfer" />,
                  hMultiplierPart(a),
                ]}
              />
            ),
          },
          { key: 'figures', tone: 'quiet', left: <Dotted parts={[supply, occupiedPart(a)]} /> },
        ]}
      />
    );
  }
  if (a.assetType === 'object') {
    const composition = a.compositionTier ? (
      <CompositionBadge tier={a.compositionTier} size="sm" />
    ) : (
      <span className="text-text-dim">Composition unavailable</span>
    );
    return (
      <PhoneRow
        testId="inventory-row-mobile"
        head={INVENTORY_PHONE_HEAD_PX}
        lines={[
          identity,
          {
            key: 'what',
            tone: 'body',
            hold: true,
            left: (
              <Dotted
                parts={[
                  <RowTag key="standard">{standard}</RowTag>,
                  <Counted key="l" value={a.liveCount} noun="objects" one="object" />,
                  <Counted key="h" value={a.holdersCount} noun="holders" one="holder" />,
                  // Round 3 (contracts L2): a count the indexer has not derived yet says nothing.
                  a.activitiesCount === null ? null : (
                    <Counted key="a" value={a.activitiesCount} noun="activities" one="activity" />
                  ),
                ]}
              />
            ),
            right: composition,
          },
          {
            key: 'figures',
            tone: 'quiet',
            left: (
              <Dotted
                parts={[
                  <Counted key="m" value={a.totalSupply} noun="minted" />,
                  occupiedPart(a),
                  hMultiplierPart(a),
                ]}
              />
            ),
          },
        ]}
      />
    );
  }
  return (
    <PhoneRow
      testId="inventory-row-mobile"
      head={INVENTORY_PHONE_HEAD_PX}
      lines={[
        identity,
        {
          key: 'what',
          tone: 'body',
          left: (
            <Dotted
              parts={[
                a.name !== standard && <RowTag key="standard">{standard}</RowTag>,
                <Counted key="l" value={a.liveCount} noun="live" />,
                <Counted key="e" value={a.totalSupply} noun="ever registered" />,
                <Counted key="h" value={a.holdersCount} noun="holders" one="holder" />,
              ]}
            />
          ),
        },
        { key: 'figures', tone: 'quiet', left: <Dotted parts={[occupiedPart(a)]} /> },
      ]}
    />
  );
}

/**
 * Each plain column head as its words in an inline box (a bar column keeps its scale head). The
 * page is unchanged; the table survey then measures the words, not the whole head cell.
 */
export function wordHeads<T>(columns: readonly ListColumn<T>[]): ListColumn<T>[] {
  return columns.map((c) =>
    c.header === undefined ? { ...c, header: <span>{c.label}</span> } : c
  );
}

// ── Table-rows round, lane L2 (board Lists.dc.html): one 40 px line per row, one bright name;
// what leaves the row is in its title and on the detail page. ──

/** CKB held and occupied, parsed once; occupied above held is an upstream bug. */
function heldAndOccupied(a: Asset): { held: bigint | null; occupied: bigint | null } {
  const held =
    a.ownedCapacity === null ? null : exactUnsignedInteger(a.ownedCapacity, `${a.id} capacity`);
  const occupied =
    a.ownedKnowledge === null ? null : exactUnsignedInteger(a.ownedKnowledge, `${a.id} occupied`);
  if (held !== null && occupied !== null && occupied > held)
    throw new Error(`${a.id}: occupied capacity exceeds held capacity`);
  return { held, occupied };
}

/** "100.00000001 CKB held · 61 CKB occupied (61.0%)" for a row's title. */
function heldTitle(a: Asset): string {
  const { held, occupied } = heldAndOccupied(a);
  const parts = [
    held === null ? 'CKB held unavailable' : `${ckbAmountText(held, 'exact')} CKB held`,
  ];
  if (occupied === null) parts.push('occupied unavailable');
  else
    parts.push(
      `${ckbAmountText(occupied, 'exact')} CKB occupied${held !== null && held > BigInt(0) ? ` (${percentOf(occupied, held)})` : ''}`
    );
  return parts.join(' · ');
}

const countText = (value: number | string | null | undefined, one: string, many: string) =>
  value === null || value === undefined
    ? `${many} unavailable`
    : `${exactUnsignedInteger(String(value), many).toLocaleString('en-US')} ${value === 1 || value === '1' ? one : many}`;

/**
 * The H-multiplier to 2 dp for the row (its exact value is in the title), its whole part grouped as
 * every figure of the site is (PA-09: "×15019.09" read as no figure of ours).
 */
function HMultiplier({ value }: { value: string | null }) {
  if (value === null) return <span>Unavailable</span>;
  const [whole, fraction] = formatDecimal(value, 2).split('.');
  return <span>{`×${groupCount(BigInt(whole))}.${fraction}`}</span>;
}

/**
 * The CKB-held column: the amount in its summary form (cut, never rounded, its exact value in its
 * title; readability R5), right aligned under its own head (readability B5,
 * plan §7.6: a bar on a linear scale to the page's largest holding was a 1–3 px stub on most rows,
 * and its head stood over the bar, not over the sorted figure). The figure shows the magnitude.
 * Its head, "CKB held in its cells" (21 characters at 12 px, 151.2), is wider than the widest
 * amount, "99,999,999,999" (14 at 13 px, 109.2); with the 16 px gap, 168 px.
 */
const HELD_LABEL = 'CKB held in its cells';
export const HELD_COLUMN: ListColumn<Asset> = {
  key: 'capacity',
  label: HELD_LABEL,
  // the head at the meta step, wider than the widest amount, and the gap
  minWidth: columnPx(Math.max(meta(HELD_LABEL), cell(WIDEST_CKB_HELD)) + CELL_GAP_PX),
  numeric: true,
  render: (a) => {
    const { held } = heldAndOccupied(a);
    if (held === null) return <span className="text-text-dim">Unavailable</span>;
    return <CkbAmount form="summary" shannons={held} signed={false} tone="inherit" unit={false} />;
  },
};

/** Board tokens row: 16 px mark, the name bright, the symbol a tag, a set-back tag for sUDT. */
function TokenName({ asset: a }: { asset: Asset }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <ClassMark kind="tokens" size={16} />
      <AppLink
        href={assetHref(a)}
        className="text-body text-text-bright leading-row min-w-0 truncate"
      >
        {a.name ?? a.symbol ?? <Hash hash={a.id} size="row" className="text-small" />}
      </AppLink>
      {a.name && a.symbol && <RowTag>{a.symbol}</RowTag>}
      {a.standard !== 'xudt' && a.standard !== 'xudt_compatible' && (
        <RowTag className="text-text-dim">{formatStandard(a.standard)}</RowTag>
      )}
    </span>
  );
}

function tokenTitle(a: Asset): string {
  return [
    a.name ?? a.symbol ?? a.id,
    ...(a.name && a.symbol ? [a.symbol] : []),
    formatStandard(a.standard),
    `${countText(a.transfers24h, 'transfer', 'transfers')} in 24 h`,
    heldTitle(a),
    a.totalSupply === null
      ? 'supply unavailable'
      : `supply ${formatTokenBalanceWithRawMarker(a.totalSupply, a.decimals)}${a.symbol ? ` ${a.symbol}` : ''}`,
    a.hMultiplier === null ? 'H-mult. unavailable' : `H-mult. ×${a.hMultiplier}`,
  ].join(' · ');
}

function tokenColumns(): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'Token',
      // The flexible column: the 16 px mark, a name (NAME_CHARS), the symbol's tag (a symbol of 8
      // characters) and the "sUDT" tag, 8 px apart, and the gap: 267.2. It was 240.
      minWidth: columnPx(
        16 + 8 + name(NAME_CHARS) + 8 + tag('x'.repeat(8)) + 8 + tag('sUDT') + CELL_GAP_PX
      ),
      width: 'auto',
      render: (a) => <TokenName asset={a} />,
    },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: COUNT_PX,
      numeric: true,
      render: (a) => (
        <span>
          <AssetCount value={a.holdersCount} />
        </span>
      ),
    },
    {
      key: 'activity',
      label: 'Transfers',
      minWidth: COUNT_PX,
      numeric: true,
      render: (a) => (
        <span>
          <AssetCount value={a.transfersCount} />
        </span>
      ),
    },
    {
      key: 'supply',
      label: 'Supply',
      // 19 characters of a supply, "100,000,000,000,000" (a hundred trillion whole units, 148.2 at
      // 13 px), and the gap; a wider one ends in the one ellipsis, its exact value in its title.
      minWidth: columnPx(cell('100,000,000,000,000') + CELL_GAP_PX),
      numeric: true,
      className: 'text-text-dim',
      // One line; a supply too long for the column ends in an ellipsis, every digit in its hover.
      render: (a) =>
        a.totalSupply === null ? (
          <span>Unavailable</span>
        ) : (
          <span className="block truncate">
            <TokenAmount form="summary" amount={a.totalSupply} decimals={a.decimals} />
          </span>
        ),
    },
    HELD_COLUMN,
    {
      key: 'hm',
      label: 'H-mult.',
      minWidth: H_MULT_PX,
      numeric: true,
      className: 'text-text-dim',
      render: (a) => <HMultiplier value={a.hMultiplier} />,
    },
  ];
}

/** A count column of one inventory table: exact, right-aligned, in its own inline box. */
function countColumn(
  key: string,
  label: string,
  value: (a: Asset) => number | string | null | undefined
): ListColumn<Asset> {
  return {
    key,
    label,
    minWidth: COUNT_PX,
    numeric: true,
    render: (a) => (
      <span>
        <AssetCount value={value(a)} />
      </span>
    ),
  };
}

/**
 * Board objects row: a 24 px mark in the collection's frame (crystal / copper for the fully
 * on-chain tiers, IV-10), the name bright, then the standard as a tag and the composition: a fully
 * on-chain tier as the site's one composition badge (IV-24), 18 px tall as a tag is, an off-chain
 * tier as a tag. The mark overhangs the 18 px line by 3 px each way, so the row stays 41 px.
 */
function CollectionName({ asset: a }: { asset: Asset }) {
  const frame = thumbFrame(a.compositionTier);
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span
        data-thumb-frame={frame ? (a.compositionTier ?? undefined) : undefined}
        className={cn(
          '-my-[3px] flex h-6 w-6 shrink-0 items-center justify-center',
          !frame && 'border-base-border border'
        )}
        style={frame}
      >
        <ClassMark kind="objects" size={12} />
      </span>
      <AppLink
        href={assetHref(a)}
        className="text-body text-text-bright leading-row min-w-0 truncate"
      >
        {a.name || <Hash hash={a.id} size="row" className="text-small" />}
      </AppLink>
      <RowTag>{formatStandard(a.standard)}</RowTag>
      {a.compositionTier === 'pure_ckb' || a.compositionTier === 'btc_ckb' ? (
        // One class, one hue (readability 2026-11-18): the frame and ink of COMPOSITION_BADGES, as
        // the filter chip above the list and the phone row draw it; the accent row tag drew
        // BTC+CKB in Pure CKB's jade.
        <CompositionBadge tier={a.compositionTier} size="sm" />
      ) : a.compositionTier ? (
        <RowTag>{formatCompositionTier(a.compositionTier)}</RowTag>
      ) : (
        <RowTag>Composition unavailable</RowTag>
      )}
    </span>
  );
}

/** Live items, and a dim "/ N" only when minted differs from live (board objects). */
function LiveOfMinted({ asset: a }: { asset: Asset }) {
  const differs =
    a.liveCount !== null && a.totalSupply !== null && String(a.liveCount) !== a.totalSupply;
  return (
    <span>
      <AssetCount value={a.liveCount} />
      {differs && (
        <span className="text-text-dim text-meta">
          {' / '}
          <AssetCount value={a.totalSupply} />
        </span>
      )}
    </span>
  );
}

function objectTitle(a: Asset): string {
  return [
    a.name || a.id,
    formatStandard(a.standard),
    a.id,
    `${countText(a.liveCount, 'live', 'live')} of ${countText(a.totalSupply, 'minted', 'minted')}`,
    heldTitle(a),
    a.hMultiplier === null ? 'H-mult. unavailable' : `H-mult. ×${a.hMultiplier}`,
  ].join(' · ');
}

function objectColumns(): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'Collection',
      // The flexible column: the 24 px framed thumb, the name's first 10 characters, the
      // standard's tag ("m-NFT", "Spore") and the widest composition tag, "Composition
      // unavailable", 8 px apart, and the gap: 373.6. Ten, not NAME_CHARS' 12: beside the widest
      // tags the table keeps its form at a 1024 px window (976 px of content; 984 with 12). It was
      // 300, which the tags alone could fill.
      minWidth: columnPx(
        24 + 8 + name(10) + 8 + tag('m-NFT') + 8 + tag('Composition unavailable') + CELL_GAP_PX
      ),
      width: 'auto',
      render: (a) => <CollectionName asset={a} />,
    },
    {
      key: 'live',
      label: 'Live items',
      // Six digits live and the quiet " / 999,999" minted beside them (the largest collections
      // hold thousands), and the gap: 142.6.
      minWidth: columnPx(cell('999,999') + meta(' / 999,999') + CELL_GAP_PX),
      numeric: true,
      render: (a) => <LiveOfMinted asset={a} />,
    },
    countColumn('holders', 'Holders', (a) => a.holdersCount),
    {
      key: 'activity',
      label: 'Activity',
      // Eight digits (a count not derived yet says nothing, never "Unavailable") and the gap.
      minWidth: columnPx(cell(WIDEST_ROW_COUNT) + CELL_GAP_PX),
      numeric: true,
      // Round 3 (contracts L2): a collection's count is null (unknown) on the cached list until
      // the indexer has derived it — say nothing rather than a stand-in.
      render: (a) =>
        a.activitiesCount === null ? null : (
          <span>
            <AssetCount value={a.activitiesCount} />
          </span>
        ),
    },
    HELD_COLUMN,
    {
      key: 'hm',
      label: 'H-mult.',
      minWidth: H_MULT_PX,
      numeric: true,
      className: 'text-text-dim',
      render: (a) => <HMultiplier value={a.hMultiplier} />,
    },
  ];
}

/** An identity system's name, bright, with its standard as a tag when the name is not it. */
function SystemName({ asset: a }: { asset: Asset }) {
  const standard = formatStandard(a.standard);
  return (
    <span className="flex min-w-0 items-center gap-2">
      <ClassMark kind="identities" size={16} />
      <AppLink
        href={assetHref(a)}
        className="text-body text-text-bright leading-row min-w-0 truncate"
      >
        {a.name || <Hash hash={a.id} size="row" className="text-small" />}
      </AppLink>
      {a.name !== standard && <RowTag>{standard}</RowTag>}
    </span>
  );
}

function identityTitle(a: Asset): string {
  const standard = formatStandard(a.standard);
  return [
    a.name || a.id,
    ...(a.name === standard ? [] : [standard]),
    countText(a.liveCount, 'live', 'live'),
    `${countText(a.totalSupply, 'ever registered', 'ever registered')}`,
    countText(a.holdersCount, 'holder', 'holders'),
    heldTitle(a),
  ].join(' · ');
}

function identityColumns(): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'System',
      // The flexible column: the 16 px mark, a name (NAME_CHARS) and the widest standard's tag,
      // ".bit Cell", 8 px apart, and the gap: 225.6. It was 200.
      minWidth: columnPx(16 + 8 + name(NAME_CHARS) + 8 + tag('.bit Cell') + CELL_GAP_PX),
      width: 'auto',
      render: (a) => <SystemName asset={a} />,
    },
    countColumn('live', 'Live now', (a) => a.liveCount),
    countColumn('minted', 'Ever registered', (a) => a.totalSupply),
    countColumn('holders', 'Holders', (a) => a.holdersCount),
    HELD_COLUMN,
    {
      key: 'occupied',
      label: 'Occupied',
      // Occupied CKB whole from 1,000 up, "99,999,999,999" (wider than "Unavailable"); the last
      // column, no gap after it. It was 130.
      minWidth: columnPx(Math.max(cell(WIDEST_CKB_HELD), cell(UNAVAILABLE))),
      numeric: true,
      className: 'text-text-dim',
      render: (a) => {
        const { occupied } = heldAndOccupied(a);
        return (
          <span>{occupied === null ? 'Unavailable' : <OccupiedCkb shannons={occupied} />}</span>
        );
      },
    },
  ];
}

export function InventoryRows({ rows, assetType }: InventoryRowsProps) {
  if (assetType === 'token')
    return (
      <ListTable
        label="Tokens"
        columns={wordHeads(tokenColumns())}
        rows={rows}
        rowKey={(a) => a.id}
        empty="No tokens."
        density="row"
        rowTitle={tokenTitle}
        mobileRow={(a) => <InventoryPhoneRow asset={a} />}
      />
    );
  if (assetType === 'object')
    return (
      <ListTable
        label="Object collections"
        columns={wordHeads(objectColumns())}
        rows={rows}
        rowKey={(a) => a.id}
        empty="No objects."
        density="row"
        rowTitle={objectTitle}
        mobileRow={(a) => <InventoryPhoneRow asset={a} />}
      />
    );
  return (
    <ListTable
      label="Identity systems"
      columns={wordHeads(identityColumns())}
      rows={rows}
      rowKey={(a) => a.id}
      empty="No identities."
      density="row"
      rowTitle={identityTitle}
      mobileRow={(a) => <InventoryPhoneRow asset={a} />}
    />
  );
}
