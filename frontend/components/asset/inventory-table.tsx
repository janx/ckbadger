import { type Asset } from '@/lib/api';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ClassMark } from '@/components/ui/class-mark';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { splitTokenAmount, formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { exactUnsignedInteger, formatDecimal, formatScaledInteger } from '@/lib/exact-integer';
import { splitCkbAmount } from '@/lib/ckb-decimal';
import { formatTick } from '@/lib/chart-ticks';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { RowTag } from '@/components/ui/row-tag';
import { resourceScale } from '@/components/blocks/block-visual';
import {
  getClusterDetailHref,
  getIdentityCollectionHref,
  getMnftClassDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { formatCompositionTier } from '@/lib/asset-utils';
import type { CSSProperties } from 'react';
import { COMPOSITION_BADGES } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

export interface AssetAmountProps {
  amount: string;
  decimals: number | null;
}
export function AssetAmount({ amount, decimals }: AssetAmountProps) {
  exactUnsignedInteger(amount, 'asset amount');
  const { integer, fraction } = splitTokenAmount(amount, decimals);
  const significant = fraction.replace(/0+$/, '');
  const groups = integer.split(',');
  // A digit run never breaks by itself; the only soft breaks are after thousands separators
  // (IV-28 / IV-13), so a wrapped amount never strands a digit or splits the fraction.
  return (
    <span className="tabular-nums" title={formatTokenBalanceWithRawMarker(amount, decimals)}>
      {groups.flatMap((group, i) =>
        i < groups.length - 1 ? [`${group},`, <wbr key={i} />] : [group]
      )}
      {fraction && (
        <>
          .<span className="opacity-80">{significant}</span>
          <span className="text-text-ghost">{fraction.slice(significant.length)}</span>
        </>
      )}
      {decimals === null && ' (raw)'}
    </span>
  );
}
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
const SHANNONS = BigInt(100000000);
/**
 * Occupied capacity is bytes, so whole CKB; an amount with a fraction is shown exactly (IV-06).
 * `wrap` lets a phone line break it before the decimals; a table row keeps it on one line.
 */
function OccupiedCkb({ shannons, wrap = true }: { shannons: bigint; wrap?: boolean }) {
  if (shannons % SHANNONS === BigInt(0))
    return <>{(shannons / SHANNONS).toLocaleString('en-US')}</>;
  return <ExactCkb shannons={shannons} signed={false} tone="plain" wrap={wrap} />;
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
/** CKB held and occupied on a phone row: "2,885,438.81 CKB held · 2,842,315 occupied (98.5%)". */
function HeldAndOccupied({ asset: a }: { asset: Asset }) {
  if (a.ownedCapacity === null) return <span>CKB held unavailable</span>;
  const held = exactUnsignedInteger(a.ownedCapacity, `${a.id} capacity`);
  const occupied =
    a.ownedKnowledge === null ? null : exactUnsignedInteger(a.ownedKnowledge, `${a.id} occupied`);
  if (occupied !== null && occupied > held)
    throw new Error(`${a.id}: occupied capacity exceeds held capacity`);
  return (
    <Dotted
      parts={[
        <span key="held" className="whitespace-nowrap">
          <ExactCkb shannons={held} signed={false} tone="plain" /> held
        </span>,
        occupied === null ? (
          'occupied unavailable'
        ) : (
          <span key="occupied" className="whitespace-nowrap">
            <OccupiedCkb shannons={occupied} /> occupied
            {held > BigInt(0) && ` (${formatScaledInteger(occupied * BigInt(100), held, 1)}%)`}
          </span>
        ),
      ]}
    />
  );
}

/** An exact count and its noun, singular for one ("1 holder", "93 objects"). */
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
    <span className="whitespace-nowrap">
      <AssetCount value={value} />{' '}
      <span className="text-text-dim">{value === 1 || value === '1' ? one : noun}</span>
    </span>
  );
}

/**
 * An inventory row on a phone (boards L3 tokens, L4 objects; identities by the same rules): name
 * and the sort figure, then standard and activity, then supply and the CKB its cells hold. In a
 * wide container (640) two lines: the standard joins the name and the figures share the second.
 */
function InventoryPhoneRow({ asset: a, wide }: { asset: Asset; wide: boolean }) {
  const standard = formatStandard(a.standard);
  const href = assetHref(a);
  if (a.assetType === 'token') {
    const name = (
      <AppLink href={href} className="text-text-bright">
        <span className="font-medium">
          {a.symbol ?? a.name ?? <Hash hash={a.id} copyable={false} className="text-small" />}
        </span>
        {a.symbol && a.name && <span className="text-text"> {a.name}</span>}
      </AppLink>
    );
    const activity = [
      <Counted key="t" value={a.transfersCount} noun="transfers" one="transfer" />,
      <Counted key="24" value={a.transfers24h} noun="in 24 h" />,
    ];
    const hm = a.hMultiplier === null ? 'H-mult. unavailable' : `×${a.hMultiplier}`;
    const supply = (
      <span>
        supply{' '}
        {a.totalSupply === null ? (
          'unavailable'
        ) : (
          <AssetAmount amount={a.totalSupply} decimals={a.decimals} />
        )}{' '}
        {a.symbol}
      </span>
    );
    return (
      <PhoneRow
        testId="inventory-row-mobile"
        lines={[
          {
            key: 'identity',
            tone: 'head',
            left: wide ? (
              <>
                {name}
                <span className="text-text-dim text-small"> · {standard}</span>
              </>
            ) : (
              name
            ),
            right: <Counted value={a.holdersCount} noun="holders" one="holder" />,
          },
          wide
            ? {
                key: 'figures',
                tone: 'quiet',
                left: <Dotted parts={[supply, <HeldAndOccupied key="h" asset={a} />]} />,
                right: <Dotted parts={[...activity, hm]} />,
              }
            : {
                key: 'activity',
                tone: 'body',
                left: <Dotted parts={[standard, ...activity]} />,
                right: hm,
              },
          !wide && {
            key: 'figures',
            tone: 'quiet',
            left: <Dotted parts={[supply, <HeldAndOccupied key="h" asset={a} />]} />,
          },
        ]}
      />
    );
  }
  const title = (
    <AppLink href={href} className="text-text-bright font-medium">
      {a.name || <Hash hash={a.id} copyable={false} className="text-small" />}
    </AppLink>
  );
  if (a.assetType === 'object') {
    const composition = a.compositionTier ? (
      <CompositionBadge tier={a.compositionTier} size="sm" />
    ) : (
      <span className="text-text-dim">Composition unavailable</span>
    );
    const people = [
      <Counted key="h" value={a.holdersCount} noun="holders" one="holder" />,
      // Round 3 (contracts L2): a count the indexer has not derived yet says nothing.
      a.activitiesCount === null ? null : (
        <Counted key="a" value={a.activitiesCount} noun="activities" one="activity" />
      ),
    ];
    const figures = [
      <span key="m" className="whitespace-nowrap">
        of <AssetCount value={a.totalSupply} /> minted
      </span>,
      <HeldAndOccupied key="c" asset={a} />,
      a.hMultiplier === null ? 'H-mult. unavailable' : `×${a.hMultiplier}`,
      <Hash
        key="id"
        hash={a.id}
        startChars={6}
        endChars={4}
        copyable={false}
        className="text-meta"
      />,
    ];
    return (
      <PhoneRow
        testId="inventory-row-mobile"
        lines={[
          {
            key: 'identity',
            tone: 'head',
            left: wide ? (
              <>
                {title}
                <span className="text-text-dim text-small"> · {standard}</span>
              </>
            ) : (
              title
            ),
            right: <Counted value={a.liveCount} noun="objects" one="object" />,
          },
          wide
            ? {
                key: 'figures',
                tone: 'quiet',
                left: <Dotted parts={[...people, ...figures]} />,
                right: composition,
              }
            : {
                key: 'activity',
                tone: 'body',
                left: <Dotted parts={[standard, ...people]} />,
                right: composition,
              },
          !wide && { key: 'figures', tone: 'quiet', left: <Dotted parts={figures} /> },
        ]}
      />
    );
  }
  const sub = a.name === standard ? null : standard;
  const counts = [
    <Counted key="e" value={a.totalSupply} noun="ever registered" />,
    <Counted key="h" value={a.holdersCount} noun="holders" one="holder" />,
  ];
  return (
    <PhoneRow
      testId="inventory-row-mobile"
      lines={[
        {
          key: 'identity',
          tone: 'head',
          left:
            wide && sub ? (
              <>
                {title}
                <span className="text-text-dim text-small"> · {sub}</span>
              </>
            ) : (
              title
            ),
          right: <Counted value={a.liveCount} noun="live" />,
        },
        wide
          ? {
              key: 'figures',
              tone: 'quiet',
              left: <Dotted parts={[...counts, <HeldAndOccupied key="c" asset={a} />]} />,
            }
          : { key: 'activity', tone: 'body', left: <Dotted parts={[sub, ...counts]} /> },
        !wide && { key: 'figures', tone: 'quiet', left: <HeldAndOccupied asset={a} /> },
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

// ── Table-rows round, lane L2 (board Lists.dc.html): one 40 px line per row, one bright name, one
// 8 px CKB bar on the page's scale; what leaves the row is in its title and on the detail page. ──
/** The bar track and the 2-dp amount beside it (the boards' 110 px amount column). */
const TRACK = 120;
const AMOUNT_WIDTH = 118;

/**
 * An exact amount of `unitDecimals` decimals CUT to 2 places (fewer when the unit has fewer): the
 * row's form of it (T1). Never rounded — a rounded figure can show a digit that is not on chain
 * (site precedent, hierarchy lane D3); the exact amount stays in the row's title.
 */
function twoPlaces(value: bigint, unitDecimals: number): { whole: string; fraction: string } {
  if (value < BigInt(0) || !Number.isInteger(unitDecimals) || unitDecimals < 0)
    throw new Error(`invalid amount ${value} with ${unitDecimals} decimals`);
  const places = Math.min(unitDecimals, 2);
  const kept = value / BigInt(10) ** BigInt(unitDecimals - places);
  const unit = BigInt(10) ** BigInt(places);
  return {
    whole: (kept / unit).toLocaleString('en-US'),
    fraction: places === 0 ? '' : (kept % unit).toString().padStart(places, '0'),
  };
}

/** CKB to 2 dp, the 2 decimals 11 px dim (board: "2,885,438.81"). */
export function CkbTwoPlaces({ shannons }: { shannons: bigint }) {
  const { whole, fraction } = twoPlaces(shannons, 8);
  return (
    <span data-ckb-two-places className="tabular-nums">
      {whole}
      <span className="text-text-dim text-meta">.{fraction}</span>
    </span>
  );
}

/** A token amount to 2 dp, its decimals in ghost ink; unknown decimals print the raw integer. */
export function TokenTwoPlaces({ amount, decimals }: { amount: string; decimals: number | null }) {
  const value = exactUnsignedInteger(amount, 'token amount');
  const exact = formatTokenBalanceWithRawMarker(amount, decimals);
  if (decimals === null)
    return (
      <span className="tabular-nums" title={exact}>
        {exact}
      </span>
    );
  const { whole, fraction } = twoPlaces(value, decimals);
  return (
    <span className="tabular-nums" title={exact}>
      {whole}
      {fraction && <span className="text-text-ghost text-meta">.{fraction}</span>}
    </span>
  );
}

/** "2,885,438.81184468" / "1,838": every digit, for a row's title. */
function exactCkbText(shannons: bigint): string {
  const { integer, fraction } = splitCkbAmount(shannons);
  return `${integer}${fraction}`;
}

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

/** Whole CKB, the bar's geometry only (the printed amount is `CkbTwoPlaces`). */
function wholeCkb(shannons: bigint, context: string): number {
  const whole = Number(shannons / SHANNONS);
  if (!Number.isSafeInteger(whole))
    throw new Error(`${context}: ${shannons} shannons is not drawable`);
  return whole;
}

/** "100.00000001 CKB held · 61 CKB occupied (61.0%)" for a row's title. */
function heldTitle(a: Asset): string {
  const { held, occupied } = heldAndOccupied(a);
  const parts = [held === null ? 'CKB held unavailable' : `${exactCkbText(held)} CKB held`];
  if (occupied === null) parts.push('occupied unavailable');
  else
    parts.push(
      `${exactCkbText(occupied)} CKB occupied${held !== null && held > BigInt(0) ? ` (${formatScaledInteger(occupied * BigInt(100), held, 1)}%)` : ''}`
    );
  return parts.join(' · ');
}

const countText = (value: number | string | null | undefined, one: string, many: string) =>
  value === null || value === undefined
    ? `${many} unavailable`
    : `${exactUnsignedInteger(String(value), many).toLocaleString('en-US')} ${value === 1 || value === '1' ? one : many}`;

/** The H-multiplier to 2 dp for the row (its exact value is in the title). */
function HMultiplier({ value }: { value: string | null }) {
  return <span>{value === null ? 'Unavailable' : `×${formatDecimal(value, 2)}`}</span>;
}

/** The CKB-held column: the page's scale on the head, an 8 px bar and the 2-dp amount per row. */
function heldColumn(
  rows: readonly Asset[],
  mark: 'tokens' | 'objects' | 'identities'
): ListColumn<Asset> {
  const scale = resourceScale(
    rows.flatMap((a) => {
      const { held } = heldAndOccupied(a);
      return held === null ? [] : [wholeCkb(held, `${a.id} capacity`)];
    })
  );
  const label = 'CKB held in its cells';
  return {
    key: 'capacity',
    label,
    minWidth: TRACK + 10 + AMOUNT_WIDTH + 16,
    width: `${TRACK + 10 + AMOUNT_WIDTH + 16}px`,
    header: (
      <ScaleHeader
        label={label}
        scale={scale}
        trackWidth={TRACK}
        midpoint
        format={(n) => formatTick(n, scale / 2)}
      />
    ),
    render: (a) => {
      const { held } = heldAndOccupied(a);
      if (held === null) return <span className="text-text-dim">Unavailable</span>;
      return (
        <AlignedBar
          label={label}
          scale={scale}
          trackWidth={TRACK}
          minFill={2}
          segments={[{ key: 'held', label: 'CKB held', value: wholeCkb(held, a.id), tone: mark }]}
          display={<CkbTwoPlaces shannons={held} />}
          displayWidth={AMOUNT_WIDTH}
        />
      );
    },
  };
}

/** Board tokens row: 16 px mark, the name bright, the symbol a tag, a set-back tag for sUDT. */
function TokenName({ asset: a }: { asset: Asset }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <ClassMark kind="tokens" size={16} />
      <AppLink
        href={assetHref(a)}
        className="text-body text-text-bright min-w-0 truncate leading-[18px]"
      >
        {a.name ?? a.symbol ?? <Hash hash={a.id} copyable={false} className="text-small" />}
      </AppLink>
      {a.name && a.symbol && <RowTag>{a.symbol}</RowTag>}
      {a.standard !== 'xudt' && a.standard !== 'xudt_compatible' && (
        <RowTag className="text-text-ghost">{formatStandard(a.standard)}</RowTag>
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

function tokenColumns(rows: readonly Asset[]): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'Token',
      minWidth: 240,
      width: 'auto',
      render: (a) => <TokenName asset={a} />,
    },
    {
      key: 'holders',
      label: 'Holders',
      minWidth: 90,
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
      minWidth: 100,
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
      minWidth: 170,
      numeric: true,
      className: 'text-text-dim',
      // One line; a supply too long for the column ends in an ellipsis, every digit in its hover.
      render: (a) =>
        a.totalSupply === null ? (
          <span>Unavailable</span>
        ) : (
          <span className="block truncate">
            <TokenTwoPlaces amount={a.totalSupply} decimals={a.decimals} />
          </span>
        ),
    },
    heldColumn(rows, 'tokens'),
    {
      key: 'hm',
      label: 'H-mult.',
      minWidth: 90,
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
  minWidth: number,
  value: (a: Asset) => number | string | null | undefined
): ListColumn<Asset> {
  return {
    key,
    label,
    minWidth,
    numeric: true,
    render: (a) => (
      <span>
        <AssetCount value={value(a)} />
      </span>
    ),
  };
}

const ON_CHAIN_TIERS = new Set<Asset['compositionTier']>(['pure_ckb', 'btc_ckb']);

/**
 * Board objects row: a 24 px mark in the collection's frame (crystal / copper for the fully
 * on-chain tiers, IV-10), the name bright, then the standard and the composition as tags. The
 * mark overhangs the 18 px line by 3 px each way, so the row stays 41 px.
 */
function CollectionName({ asset: a }: { asset: Asset }) {
  const frame = thumbFrame(a.compositionTier);
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span
        data-thumb-frame={frame ? (a.compositionTier ?? undefined) : undefined}
        className={cn(
          '-my-[3px] flex h-6 w-6 shrink-0 items-center justify-center rounded-sm',
          !frame && 'border-base-border border'
        )}
        style={frame}
      >
        <ClassMark kind="objects" size={12} />
      </span>
      <AppLink
        href={assetHref(a)}
        className="text-body text-text-bright min-w-0 truncate leading-[18px]"
      >
        {a.name || <Hash hash={a.id} copyable={false} className="text-small" />}
      </AppLink>
      <RowTag>{formatStandard(a.standard)}</RowTag>
      {a.compositionTier ? (
        <RowTag variant={ON_CHAIN_TIERS.has(a.compositionTier) ? 'accent' : 'default'}>
          {formatCompositionTier(a.compositionTier)}
        </RowTag>
      ) : (
        <RowTag>Composition unavailable</RowTag>
      )}
    </span>
  );
}

/** Live items, and a ghost "/ N" only when minted differs from live (board objects). */
function LiveOfMinted({ asset: a }: { asset: Asset }) {
  const differs =
    a.liveCount !== null && a.totalSupply !== null && String(a.liveCount) !== a.totalSupply;
  return (
    <span>
      <AssetCount value={a.liveCount} />
      {differs && (
        <span className="text-text-ghost text-meta">
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

function objectColumns(rows: readonly Asset[]): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'Collection',
      minWidth: 300,
      width: 'auto',
      render: (a) => <CollectionName asset={a} />,
    },
    {
      key: 'live',
      label: 'Live items',
      minWidth: 120,
      numeric: true,
      render: (a) => <LiveOfMinted asset={a} />,
    },
    countColumn('holders', 'Holders', 90, (a) => a.holdersCount),
    {
      key: 'activity',
      label: 'Activity',
      minWidth: 100,
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
    heldColumn(rows, 'objects'),
    {
      key: 'hm',
      label: 'H-mult.',
      minWidth: 90,
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
        className="text-body text-text-bright min-w-0 truncate leading-[18px]"
      >
        {a.name || <Hash hash={a.id} copyable={false} className="text-small" />}
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

function identityColumns(rows: readonly Asset[]): ListColumn<Asset>[] {
  return [
    {
      key: 'identity',
      label: 'System',
      minWidth: 200,
      width: 'auto',
      render: (a) => <SystemName asset={a} />,
    },
    countColumn('live', 'Live now', 100, (a) => a.liveCount),
    countColumn('minted', 'Ever registered', 130, (a) => a.totalSupply),
    countColumn('holders', 'Holders', 90, (a) => a.holdersCount),
    heldColumn(rows, 'identities'),
    {
      key: 'occupied',
      label: 'Occupied',
      minWidth: 130,
      numeric: true,
      className: 'text-text-dim',
      render: (a) => {
        const { occupied } = heldAndOccupied(a);
        return (
          <span>
            {occupied === null ? 'Unavailable' : <OccupiedCkb shannons={occupied} wrap={false} />}
          </span>
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
        columns={wordHeads(tokenColumns(rows))}
        rows={rows}
        rowKey={(a) => a.id}
        empty="No tokens found"
        density="row"
        rowTitle={tokenTitle}
        mobileRow={(a, wide) => <InventoryPhoneRow asset={a} wide={wide} />}
      />
    );
  if (assetType === 'object')
    return (
      <ListTable
        label="Object collections"
        columns={wordHeads(objectColumns(rows))}
        rows={rows}
        rowKey={(a) => a.id}
        empty="No objects found"
        density="row"
        rowTitle={objectTitle}
        mobileRow={(a, wide) => <InventoryPhoneRow asset={a} wide={wide} />}
      />
    );
  return (
    <ListTable
      label="Identity systems"
      columns={wordHeads(identityColumns(rows))}
      rows={rows}
      rowKey={(a) => a.id}
      empty="No identities found"
      density="row"
      rowTitle={identityTitle}
      mobileRow={(a, wide) => <InventoryPhoneRow asset={a} wide={wide} />}
    />
  );
}
