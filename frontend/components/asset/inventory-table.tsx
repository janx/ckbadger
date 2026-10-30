import { type Asset } from '@/lib/api';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ClassMark } from '@/components/ui/class-mark';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { ObjectCompositionLabel } from '@/components/address/address-holdings';
import { splitTokenAmount, formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import {
  getClusterDetailHref,
  getIdentityCollectionHref,
  getMnftClassDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';

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
export function InventoryRows({ rows, assetType }: InventoryRowsProps) {
  const mark = assetType === 'token' ? 'tokens' : assetType === 'object' ? 'objects' : 'identities';
  const columns: ListColumn<Asset>[] = [
    {
      key: 'identity',
      label: assetType === 'token' ? 'Token' : assetType === 'object' ? 'Collection' : 'System',
      minWidth: 220,
      width: '22%',
      render: (a) => (
        <div className="flex min-w-0 items-center gap-3">
          <span
            className={
              assetType === 'object'
                ? 'border-base-border flex h-11 w-11 shrink-0 items-center justify-center rounded border'
                : ''
            }
          >
            <ClassMark kind={mark} size={18} />
          </span>
          <div className="min-w-0">
            <AppLink href={assetHref(a)} className="text-text-bright block break-words">
              {a.symbol || a.name || <Hash hash={a.id} copyable={false} className="text-xs" />}
            </AppLink>
            <span className="text-text-dim text-[11px]">
              {formatStandard(a.standard)}
              {a.assetType === 'object' && (
                <>
                  {' '}
                  ·{' '}
                  <Hash
                    hash={a.id}
                    startChars={6}
                    endChars={4}
                    copyable={false}
                    className="text-[11px]"
                  />
                </>
              )}
            </span>
          </div>
        </div>
      ),
    },
  ];
  if (assetType === 'object')
    columns.push({
      key: 'composition',
      label: 'Composition',
      minWidth: 160,
      render: (a) => <ObjectCompositionLabel tier={a.compositionTier ?? null} />,
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
            <span className="text-text-dim block text-[11px]">
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
      render: (a) => (
        <>
          <AssetCount value={assetType === 'token' ? a.transfersCount : a.activitiesCount} />
          {assetType === 'token' && (
            <span className="text-text-dim block text-[11px]">
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
            {occupied !== null && (
              <div
                aria-label="Occupied share of held capacity"
                className="bg-base-elevated flex h-1 overflow-hidden rounded"
              >
                <span
                  className="bg-jade"
                  style={{
                    width:
                      held === BigInt(0)
                        ? '0%'
                        : `${Number((occupied * BigInt(10000)) / held) / 100}%`,
                  }}
                />
              </div>
            )}
          </div>
        );
      },
    },
    {
      key: 'occupied',
      label: 'Occupied CKB',
      minWidth: 150,
      numeric: true,
      className: '!whitespace-normal',
      render: (a) =>
        a.ownedKnowledge === null ? (
          'Unavailable'
        ) : (
          <ExactCkb
            shannons={exactUnsignedInteger(a.ownedKnowledge, `${a.id} occupied`)}
            signed={false}
            tone="plain"
            wrap
          />
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
    />
  );
}
