import { type Asset } from '@/lib/api';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ClassMark } from '@/components/ui/class-mark';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { splitTokenAmount, formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
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
          {a.symbol ?? a.name ?? <Hash hash={a.id} copyable={false} className="text-xs" />}
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
                <span className="text-text-dim text-xs"> · {standard}</span>
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
      {a.name || <Hash hash={a.id} copyable={false} className="text-xs" />}
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
                <span className="text-text-dim text-xs"> · {standard}</span>
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
                <span className="text-text-dim text-xs"> · {sub}</span>
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

export function InventoryRows({ rows, assetType }: InventoryRowsProps) {
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
                {title || <Hash hash={a.id} copyable={false} className="text-xs" />}
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
