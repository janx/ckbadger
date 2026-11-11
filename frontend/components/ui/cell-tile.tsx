import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import { CellBar, type CellAnatomy } from '@/components/ui/cell-bar';
import { OwnerSeal } from '@/components/ui/owner-seal';
import { RowSeal } from '@/components/ui/row-seal';
import { ExactCkb } from '@/components/activity-event-row';
import type { CellSeal } from '@/lib/cell-seal';
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
  /**
   * The seals by kind (C3): the cell's own stone (its id: out point, lock, type's class and hash,
   * capacity) in the ink of its state, leading the tile; its lock hash is the owner's stone before
   * the owner's address.
   */
  seal: CellSeal;
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
  assertSealOfTile(cell, capacity);
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
        'bg-base-surface text-small min-w-0 rounded border px-3 pb-2 pt-3 transition-colors',
        highlighted
          ? 'border-jade io-linked-highlight border-emphasis/70'
          : 'border-base-border-subtle',
        dimmed && 'opacity-30'
      )}
    >
      {/* The seals by kind (C3). The first line holds three things that wrap as wholes: the
          cell's row stone before its out point (the stone overhangs the 18 px line by 6 px above,
          into the tile's 12 px top padding, and below; an image, the out point stays the link),
          the owner's stone before the owner's address, and the lock's kind. The owner wraps as
          one unit, never broken, under the stone at the narrowest (the 12 px row gap keeps 5 px
          between the two stones); `mr-auto` keeps it beside the out point and the lock's kind at
          the right. Tops aligned (one text size, as the baselines were): a stone has none. */}
      <div className="flex flex-wrap items-start justify-between gap-x-2 gap-y-3">
        <span className="flex min-w-0 items-center">
          <RowSeal id={cell.seal.id} state={cell.seal.state} overhang />
          <span className="min-w-0">
            {cell.outPoint ? (
              <>
                <span className="text-text-dim">{cell.label}</span>{' '}
                {/* an out point with its index is one unit, never broken */}
                <Link
                  href={cell.href}
                  title={`${cell.outPoint.txHash}:${cell.outPoint.index}`}
                  className="text-text-dim hover:text-jade whitespace-nowrap"
                >
                  {truncateHash(cell.outPoint.txHash, 6, 4)}:{cell.outPoint.index}
                </Link>
              </>
            ) : (
              <Link href={cell.href} className="text-text-dim hover:text-jade">
                {cell.label}
              </Link>
            )}
          </span>
        </span>
        <span data-tile-owner className="mr-auto whitespace-nowrap">
          <OwnerSeal lockHash={cell.seal.id.lock} className="-my-px" />
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

/**
 * The seal is drawn from the same reads as the tile (its out point, `kindOf()`'s class, its
 * capacity): a seal that disagrees with the tile it leads is two paths for one datum, refused.
 */
function assertSealOfTile(cell: CellTileData, capacity: bigint): void {
  const { id } = cell.seal;
  const sealClass = id.type === null ? 'ckb' : id.type.class;
  const outPoint = cell.outPoint;
  if (
    id.capacity !== capacity ||
    sealClass !== cell.kind ||
    (outPoint !== undefined && (outPoint.txHash !== id.tx || outPoint.index !== id.index))
  )
    throw new Error(
      `${cell.id}: its seal (cell ${id.tx}:${id.index}, ${sealClass}, ${id.capacity} shannons) is not the tile's (${outPoint ? `cell ${outPoint.txHash}:${outPoint.index}, ` : ''}${cell.kind}, ${capacity} shannons)`
    );
}

// F2-tx (TX-19): one byte of state occupies exactly one CKB.
function occupiedBytes(id: string, shannons: bigint): bigint {
  if (shannons % SHANNONS_PER_CKB !== BigInt(0))
    throw new Error(`${id}: occupied ${shannons} shannons is not whole bytes`);
  return shannons / SHANNONS_PER_CKB;
}
