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
        'bg-base-surface text-small grid min-w-0 grid-cols-[30px_minmax(0,1fr)] gap-x-2.5 gap-y-2.5 rounded border px-3 pb-2 pt-3 transition-colors',
        highlighted
          ? 'border-jade io-linked-highlight border-emphasis/70'
          : 'border-base-border-subtle',
        dimmed && 'opacity-30'
      )}
    >
      {/* One mark column, one text edge (seals by kind, C11): a 30 px column of marks and the
          text, 10 px apart, in four rows — the cell's stone with its label and out point, the
          owner's stone with the owner, the class mark with the content, then the occupied bar.
          Each mark stands alone, centred in the column, its top on its row's first 18 px line
          (the round stone overhangs that line by 6 px, into the 12 px top padding; the owner's by
          1 px; the 16 px class mark sits 1 px down); the 10 px row gap keeps the round stone's
          drawn 5 px overhang 4 px clear of the owner's stone. Every row's text starts on the same
          edge; the rows after the first end with their right-end item (lock kind, capacity, the
          cell's state beside the bar), which wraps under its row's text when it does not fit and
          stays right. Row 1 is the label and the out point alone: with the state beside them it
          wrapped on every tile of a 203 px text column (the sheets at 1024). A row is a
          `contents` pair: its two cells are the grid's. */}
      <div data-tile-row="cell" className="contents">
        <span data-tile-mark className="flex items-start justify-center">
          <RowSeal id={cell.seal.id} state={cell.seal.state} overhang gap={false} />
        </span>
        <TileText>
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
        </TileText>
      </div>
      <div data-tile-row="owner" data-tile-owner className="contents">
        <span data-tile-mark className="flex items-start justify-center">
          <OwnerSeal lockHash={cell.seal.id.lock} gap={false} className="-my-px" />
        </span>
        <TileText end={cell.lockKind && <span className="text-text-dim">{cell.lockKind}</span>}>
          {/* the owner is one unit, never broken */}
          <span className="whitespace-nowrap">{cell.owner}</span>
        </TileText>
      </div>
      <div data-tile-row="content" className="contents">
        <span data-tile-mark className="flex items-start justify-center pt-px">
          <ClassMark kind={cell.kind} />
        </span>
        <TileText end={<ExactCkb shannons={capacity} signed={false} tone="plain" />}>
          <span className="text-text-bright min-w-0 max-w-full">{cell.content}</span>
        </TileText>
      </div>
      <div data-tile-row="occupied" className="contents">
        <span data-tile-mark />
        {/* the bar, its bytes and the state: 56 + 6 + 79.2 ('occupied 231') + 6 + 52.8
            ('consumed') = 200 px, inside the narrowest text column (203 px, the sheets at 1024) */}
        <TileText
          align="center"
          end={<span className="text-text-dim text-meta">{cell.state}</span>}
        >
          {cell.anatomy ? (
            <CellBar subject={cell.id} anatomy={cell.anatomy} className="min-w-0 flex-1" />
          ) : (
            <div aria-hidden="true" className="bg-base-elevated h-1 w-14 shrink-0">
              <div
                className="bg-jade h-full"
                style={{ width: `${(Number(occupied) / Number(capacity)) * 100}%` }}
              />
            </div>
          )}
          {/* F2-tx (TX-19): occupied capacity is bytes, so the board writes it whole: "occupied 195". */}
          <span className="text-text-dim text-meta">
            occupied {occupiedBytes(cell.id, occupied).toLocaleString('en-US')}
          </span>
        </TileText>
      </div>
    </article>
  );
}

/**
 * A tile row's text cell (C11): the row's text from the text edge, its right-end item last,
 * pushed right; when the item does not fit beside the text it wraps under it and stays right
 * (`ml-auto`), and one wider than the whole column wraps inside itself, right-aligned. A text
 * line sits on the baseline; the bar's line is centred, its gaps 6 px.
 */
function TileText({
  children,
  end,
  align = 'baseline',
}: {
  children: ReactNode;
  end?: ReactNode;
  align?: 'baseline' | 'center';
}) {
  return (
    <div
      data-tile-text
      className={cn(
        'flex min-w-0 flex-wrap justify-between',
        align === 'baseline' ? 'items-baseline gap-x-2 gap-y-0.5' : 'items-center gap-x-1.5 gap-y-1'
      )}
    >
      {children}
      {end && (
        <span data-tile-end className="ml-auto max-w-full text-right">
          {end}
        </span>
      )}
    </div>
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
