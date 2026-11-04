import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import { CellBar, type CellAnatomy } from '@/components/ui/cell-bar';
import { ExactCkb } from '@/components/activity-event-row';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import type { AssetClass } from '@/lib/design-tokens';
import { cn, truncateHash } from '@/lib/utils';

export interface CellTileData {
  id: string;
  href: string;
  label: string;
  owner: ReactNode;
  lockKind: string | null;
  kind: AssetClass;
  content: ReactNode;
  capacity: string;
  occupied: string;
  state: string;
  anatomy?: CellAnatomy;
  testId?: string;
  /**
   * Lane L (L1): the cell's out point. When given, the out point is the tile's link to the cell
   * page (its hover title the full out point) and `label` ("input 0") stays as text beside it;
   * without it the label is the link, as before.
   */
  outPoint?: { txHash: string; index: number };
}
export interface CellTileProps {
  cell: CellTileData;
  highlighted?: boolean;
  dimmed?: boolean;
  onHover?: (id: string | null) => void;
  onClick?: () => void;
}
export function CellTile({ cell, highlighted, dimmed, onHover, onClick }: CellTileProps) {
  const capacity = exactUnsignedInteger(cell.capacity, `${cell.id}: capacity`);
  const occupied = exactUnsignedInteger(cell.occupied, `${cell.id}: occupied`);
  if (capacity === BigInt(0) || occupied > capacity)
    throw new Error(
      `${cell.id}: occupied ${occupied} exceeds capacity ${capacity} or capacity is zero`
    );
  return (
    <article
      data-cell-id={cell.id}
      data-testid={cell.testId}
      onClick={
        onClick &&
        ((event) => {
          // A click inside a link (the out point, the owner, the content) is the link's: the
          // tile's own click never follows it, so it cannot cancel the navigation (review
          // 2026-11-03: a lit tile's clear-focus click replaced the URL right after the out point
          // link had pushed the cell page).
          if ((event.target as Element).closest('a')) return;
          onClick();
        })
      }
      data-highlighted={Boolean(highlighted)}
      onMouseEnter={() => onHover?.(cell.id)}
      onMouseLeave={() => onHover?.(null)}
      onFocus={() => onHover?.(cell.id)}
      onBlur={() => onHover?.(null)}
      className={cn(
        'bg-base-surface text-small min-w-0 rounded border px-3 py-2 transition-colors',
        highlighted
          ? 'border-jade io-linked-highlight border-emphasis/70'
          : 'border-base-border-subtle',
        dimmed && 'opacity-30'
      )}
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="min-w-0 break-all">
          {cell.outPoint ? (
            <>
              <span className="text-text-dim">{cell.label}</span>{' '}
              <Link
                href={cell.href}
                title={`${cell.outPoint.txHash}:${cell.outPoint.index}`}
                className="text-text-dim hover:text-jade"
              >
                {truncateHash(cell.outPoint.txHash, 6, 4)}:{cell.outPoint.index}
              </Link>
            </>
          ) : (
            <Link href={cell.href} className="text-text-dim hover:text-jade">
              {cell.label}
            </Link>
          )}{' '}
          {cell.owner}
        </span>
        {cell.lockKind && <span className="text-text-dim">{cell.lockKind}</span>}
      </div>
      <div className="my-2 flex flex-wrap items-baseline justify-between gap-2">
        <span className="text-text-bright flex min-w-0 max-w-full items-start gap-1.5">
          <ClassMark kind={cell.kind} />
          {cell.content}
        </span>
        <ExactCkb shannons={capacity} signed={false} tone="plain" />
      </div>
      {cell.anatomy ? (
        <CellBar subject={cell.id} anatomy={cell.anatomy} />
      ) : (
        <div aria-hidden="true" className="bg-base-elevated h-1 w-14">
          <div
            className="bg-jade h-full"
            style={{ width: `${(Number(occupied) / Number(capacity)) * 100}%` }}
          />
        </div>
      )}
      <div className="text-text-dim text-meta mt-2 flex flex-wrap justify-between gap-2">
        {/* F2-tx (TX-19): occupied capacity is bytes, so the board writes it whole: "occupied 195". */}
        <span>occupied {occupiedBytes(cell.id, occupied).toLocaleString('en-US')}</span>
        <span>{cell.state}</span>
      </div>
    </article>
  );
}

// F2-tx (TX-19): one byte of state occupies exactly one CKB.
function occupiedBytes(id: string, shannons: bigint): bigint {
  if (shannons % SHANNONS_PER_CKB !== BigInt(0))
    throw new Error(`${id}: occupied ${shannons} shannons is not whole bytes`);
  return shannons / SHANNONS_PER_CKB;
}
