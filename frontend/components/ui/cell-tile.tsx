import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import { CellBar, type CellAnatomy } from '@/components/ui/cell-bar';
import { OwnerSeal } from '@/components/ui/owner-seal';
import { RowSeal } from '@/components/ui/row-seal';
import { CkbAmount } from '@/components/ui/ckb-amount';
import type { CellSeal } from '@/lib/cell-seal';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { occupiedBytesText } from '@/lib/occupied';
import { SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import type { AssetClass } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import { OutpointIndex } from '@/components/ui/outpoint-index';

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
        'bg-base-surface text-small grid min-w-0 grid-cols-[18px_minmax(0,1fr)] gap-x-2.5 gap-y-2.5 border px-3 pb-2 pt-3 transition-colors',
        highlighted
          ? 'border-jade io-linked-highlight border-emphasis/70'
          : 'border-base-border-subtle',
        // set back (R4): every word dim, the stones at full strength in their own state's ink
        dimmed && 'quiet'
      )}
    >
      {/* One mark column, one text edge (seals by kind, C11): a column of marks as wide as the
          widest, the cell's 18 px stone (readability R6: it was the 30 px stone's 30; the owner's
          stone is 15 wide, the class mark 16), and the text, 10 px apart, in four rows — the
          cell's stone with its label and out point, the owner's stone with the owner, the class
          mark with the content, then the occupied bar. Each mark stands alone, centred in the
          column, on its row's first line (the row line, 20 px): the cell's 18 px stone centred on
          it, 1 px clear above and below (RowSeal); the owner's overhangs it by 1 px; the 16 px
          class mark sits 1 px down. Every row's text starts on the same edge; the rows after the
          first end with their right-end item (lock kind, capacity, the cell's state beside the
          bar), which wraps under its row's text when it does not fit and stays right. Row 1 is the
          label and the out point alone: with the state beside them it wrapped on every tile of a
          203 px text column (the sheets at 1024 beside the 30 px mark column; 215 beside the 18).
          A row is a `contents` pair: its two cells are the grid's. */}
      <div data-tile-row="cell" className="contents">
        <span data-tile-mark className="flex items-start justify-center">
          <RowSeal id={cell.seal.id} state={cell.seal.state} gap={false} />
        </span>
        <TileText>
          <span className="min-w-0">
            {/* R3: the label ("input 0") is quiet; the out point, the tile's link, reads */}
            {cell.outPoint ? (
              <>
                <span className="text-text-dim">{cell.label}</span>{' '}
                {/* an out point with its index is one unit, never broken; tight: a tile's text
                    column is under 200 px (U-09) */}
                <Link
                  href={cell.href}
                  title={`${cell.outPoint.txHash}:${cell.outPoint.index}`}
                  className="text-text hover:text-jade whitespace-nowrap"
                >
                  <ShortId value={cell.outPoint.txHash} kind="hash" size="tight" />
                  <OutpointIndex index={cell.outPoint.index} />
                </Link>
              </>
            ) : (
              <Link href={cell.href} className="text-text hover:text-jade">
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
        {/* the lock's kind, a label beside the owner: quiet (R3) */}
        <TileText end={cell.lockKind && <span className="text-text-dim">{cell.lockKind}</span>}>
          {/* the owner is one unit, never broken */}
          <span className="whitespace-nowrap">{cell.owner}</span>
        </TileText>
      </div>
      <div data-tile-row="content" className="contents">
        <span data-tile-mark className="flex items-start justify-center pt-px">
          <ClassMark kind={cell.kind} size={16} />
        </span>
        <TileText end={<CkbAmount form="column" shannons={capacity} signed={false} tone="plain" />}>
          <span className="text-text-bright min-w-0 max-w-full">{cell.content}</span>
        </TileText>
      </div>
      <div data-tile-row="occupied" className="contents">
        <span data-tile-mark />
        {/* the bar, its bytes and the state at 12 px, budgeted at a cell's widest occupied figure:
            at most a block's serialized size (MAX_BLOCK_BYTES, 597,000, consensus), so
            "occupied 597,000 B", 129.6 px: 56 + 6 + 129.6 + 6 + 57.6 ('consumed') = 255.2 px. It
            holds in the 343 px text column at 1280; in the 215 px column at 1024 (from "occupied
            9 B" on) the state, the line's end, drops under the bar on its own line at the right */}
        <TileText
          align="center"
          // the cell's state is a value: the reading ink (R3)
          end={<span className="text-text text-meta">{cell.state}</span>}
        >
          {cell.anatomy ? (
            <CellBar subject={cell.id} anatomy={cell.anatomy} className="min-w-0 flex-1" />
          ) : (
            <div aria-hidden="true" className="bg-bar-track h-1 w-14 shrink-0">
              <div
                className="bg-jade scan-fill h-full"
                style={{ width: `${(Number(occupied) / Number(capacity)) * 100}%` }}
              />
            </div>
          )}
          {/* F2-tx (TX-19): occupied capacity is bytes, written whole with its unit by the one
              writer, "occupied 195 B" (B4). R3: "occupied" is the label, quiet; its figure is a
              number, the reading ink. */}
          <span className="text-text-dim text-meta">
            occupied{' '}
            <span className="text-text">{occupiedBytesText(occupiedBytes(cell.id, occupied))}</span>
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
