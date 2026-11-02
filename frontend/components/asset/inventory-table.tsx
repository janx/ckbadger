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
import type { CSSProperties } from 'react';
import { CLASS_COLORS, COMPOSITION_BADGES } from '@/lib/design-tokens';
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
/** Width of a value as a share of the largest, rounded to 1/1000 %, from exact integers. */
function shareOfLargest(value: bigint, largest: bigint): string {
  if (largest === BigInt(0)) return '0%';
  return `${Number(((value * BigInt(200000)) / largest + BigInt(1)) / BigInt(2)) / 1000}%`;
}
const SHANNONS = BigInt(100000000);
/** Occupied capacity is bytes, so whole CKB; an amount with a fraction is shown exactly (IV-06). */
function OccupiedCkb({ shannons }: { shannons: bigint }) {
  if (shannons % SHANNONS === BigInt(0))
    return <>{(shannons / SHANNONS).toLocaleString('en-US')}</>;
  return <ExactCkb shannons={shannons} signed={false} tone="plain" wrap />;
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

// ── Table-rows round, lane L2 (board Lists.dc.html): one 40 px line per row, one bright name, one
// 8 px CKB bar on the page's scale; what leaves the row is in its title and on the detail page. ──
/** The bar track and the 2-dp amount beside it (the boards' 110 px amount column). */
const TRACK = 120;
const AMOUNT_WIDTH = 118;

/**
 * An exact amount of `unitDecimals` decimals rounded half-up to 2 places (fewer when the unit has
 * fewer): the row's form of it (T1). The exact amount stays in the row's title.
 */
function twoPlaces(value: bigint, unitDecimals: number): { whole: string; fraction: string } {
  const text = formatScaledInteger(
    value,
    BigInt(10) ** BigInt(unitDecimals),
    Math.min(unitDecimals, 2)
  );
  const [whole, fraction = ''] = text.split('.');
  return { whole: BigInt(whole).toLocaleString('en-US'), fraction };
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

export function InventoryRows({ rows, assetType }: InventoryRowsProps) {
  if (assetType === 'token')
    return (
      <ListTable
        label="Tokens"
        columns={tokenColumns(rows)}
        rows={rows}
        rowKey={(a) => a.id}
        empty="No tokens found"
        density="row"
        rowTitle={tokenTitle}
        mobileRow={(a, wide) => <InventoryPhoneRow asset={a} wide={wide} />}
      />
    );
  return <CollectionRows rows={rows} assetType={assetType} />;
}

function CollectionRows({ rows, assetType }: InventoryRowsProps) {
  const mark = assetType === 'token' ? 'tokens' : assetType === 'object' ? 'objects' : 'identities';
  const largestHeld = rows.reduce((max, a) => {
    if (a.ownedCapacity === null) return max;
    const held = exactUnsignedInteger(a.ownedCapacity, `${a.id} capacity`);
    return held > max ? held : max;
  }, BigInt(0));
  const columns: ListColumn<Asset>[] = [
    {
      key: 'identity',
      label: assetType === 'token' ? 'Token' : assetType === 'object' ? 'Collection' : 'System',
      minWidth: 220,
      width: '22%',
      render: (a) => {
        const standard = formatStandard(a.standard);
        const title = assetType === 'token' ? (a.name ?? a.symbol) : a.name;
        const sub =
          assetType === 'token'
            ? a.name && a.symbol
              ? `${a.symbol} · ${standard}`
              : standard
            : assetType === 'identity'
              ? title === standard
                ? null
                : standard
              : null;
        return (
          <div className="flex min-w-0 items-center gap-3">
            <span
              data-thumb-frame={
                assetType === 'object' && thumbFrame(a.compositionTier)
                  ? (a.compositionTier ?? undefined)
                  : undefined
              }
              className={
                assetType === 'object'
                  ? cn(
                      'flex h-11 w-11 shrink-0 items-center justify-center rounded',
                      !thumbFrame(a.compositionTier) && 'border-base-border border'
                    )
                  : ''
              }
              style={assetType === 'object' ? thumbFrame(a.compositionTier) : undefined}
            >
              <ClassMark kind={mark} size={16} />
            </span>
            <div className="min-w-0">
              <AppLink
                href={assetHref(a)}
                className={cn(
                  'text-text-bright block break-words',
                  assetType === 'identity' && 'text-body'
                )}
              >
                {title || <Hash hash={a.id} copyable={false} className="text-small" />}
              </AppLink>
              {sub && <span className="text-text-dim text-meta">{sub}</span>}
              {a.assetType === 'object' && (
                <span className="text-text-dim text-meta">
                  {standard} ·{' '}
                  <Hash
                    hash={a.id}
                    startChars={6}
                    endChars={4}
                    copyable={false}
                    className="text-meta"
                  />
                </span>
              )}
            </div>
          </div>
        );
      },
    },
  ];
  if (assetType === 'object')
    columns.push({
      key: 'composition',
      label: 'Composition',
      minWidth: 160,
      render: (a) =>
        a.compositionTier ? (
          <CompositionBadge tier={a.compositionTier} />
        ) : (
          <span className="text-text-dim text-meta">Composition unavailable</span>
        ),
    });
  if (assetType !== 'token')
    columns.push({
      key: 'live',
      label: assetType === 'identity' ? 'Live now' : 'Live items',
      minWidth: 110,
      numeric: true,
      render: (a) => (
        <>
          <AssetCount value={a.liveCount} />
          {assetType === 'object' && (
            <span className="text-text-dim text-meta block">
              of <AssetCount value={a.totalSupply} /> minted
            </span>
          )}
        </>
      ),
    });
  if (assetType === 'identity')
    columns.push({
      key: 'minted',
      label: 'Ever registered',
      minWidth: 130,
      numeric: true,
      render: (a) => <AssetCount value={a.totalSupply} />,
    });
  columns.push({
    key: 'holders',
    label: 'Holders',
    minWidth: 86,
    numeric: true,
    render: (a) => <AssetCount value={a.holdersCount} />,
  });
  if (assetType !== 'identity')
    columns.push({
      key: 'activity',
      label: assetType === 'token' ? 'Transfers' : 'Activity',
      minWidth: 100,
      numeric: true,
      render: (a) =>
        // Round 3 (contracts L2): a collection's count is null (unknown) on the cached list until
        // the indexer has derived it — say nothing rather than a stand-in.
        assetType !== 'token' && a.activitiesCount === null ? null : (
          <>
            <AssetCount value={assetType === 'token' ? a.transfersCount : a.activitiesCount} />
            {assetType === 'token' && (
              <span className="text-text-dim text-meta block">
                <AssetCount value={a.transfers24h} /> in 24 h
              </span>
            )}
          </>
        ),
    });
  if (assetType === 'token')
    columns.push({
      key: 'supply',
      label: 'Supply',
      minWidth: 210,
      numeric: true,
      className: '!whitespace-normal',
      render: (a) =>
        a.totalSupply === null ? (
          'Unavailable'
        ) : (
          <AssetAmount amount={a.totalSupply} decimals={a.decimals} />
        ),
    });
  columns.push(
    {
      key: 'capacity',
      label: 'CKB held in its cells',
      minWidth: 200,
      numeric: true,
      className: '!whitespace-normal',
      render: (a) => {
        if (a.ownedCapacity === null) return <span className="text-text-dim">Unavailable</span>;
        const held = exactUnsignedInteger(a.ownedCapacity, `${a.id} capacity`);
        const occupied =
          a.ownedKnowledge === null
            ? null
            : exactUnsignedInteger(a.ownedKnowledge, `${a.id} occupied`);
        if (occupied !== null && occupied > held)
          throw new Error(`${a.id}: occupied capacity exceeds held capacity`);
        return (
          <div className="space-y-1">
            <ExactCkb shannons={held} signed={false} tone="plain" wrap />
            {/* F2 objects (IV-02): CKB held as a bar in the class colour, against the largest
                on this page, so rows compare at a glance. */}
            <div className="bg-base-elevated flex h-1 overflow-hidden rounded">
              <span
                data-held-bar
                style={{
                  width: shareOfLargest(held, largestHeld),
                  backgroundColor: CLASS_COLORS[mark],
                }}
              />
            </div>
          </div>
        );
      },
    },
    {
      key: 'occupied',
      label: 'Occupied',
      minWidth: 110,
      numeric: true,
      className: '!whitespace-normal text-text-dim',
      render: (a) =>
        a.ownedKnowledge === null ? (
          'Unavailable'
        ) : (
          <OccupiedCkb shannons={exactUnsignedInteger(a.ownedKnowledge, `${a.id} occupied`)} />
        ),
    }
  );
  if (assetType !== 'identity')
    columns.push({
      key: 'hm',
      label: 'H-mult.',
      minWidth: 110,
      className: '!whitespace-normal break-all',
      numeric: true,
      render: (a) => (a.hMultiplier === null ? 'Unavailable' : `×${a.hMultiplier}`),
    });
  return (
    <ListTable
      label={
        assetType === 'token'
          ? 'Tokens'
          : assetType === 'object'
            ? 'Object collections'
            : 'Identity systems'
      }
      columns={columns}
      rows={rows}
      rowKey={(a) => a.id}
      empty={`No ${mark} found`}
      mobileRow={(a, wide) => <InventoryPhoneRow asset={a} wide={wide} />}
    />
  );
}
