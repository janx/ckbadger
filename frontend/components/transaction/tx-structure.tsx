'use client';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { CellTile, type CellTileData } from '@/components/ui/cell-tile';
import { TerminalPanel } from '@/components/ui/terminal-panel';
import { CkbAmount } from '@/components/ui/ckb-amount';
import Link from '@/components/ui/link';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import { Seal } from '@/components/ui/seal';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import type { SealId, SealState } from '@/lib/seal/stones';
import { cn } from '@/lib/utils';
import { CELL_COLORS, CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';
import { unitNode } from '@/components/ui/units';
import { Pager } from '@/components/ui/pager';

/** A transaction's stone (the seals by kind): its hash and the cells it consumed and created. */
export type TxSealId = Extract<SealId, { kind: 'tx' }>;

/**
 * The tiles a sheet draws before its fold row (lane D3; the transaction board the owner passed at
 * gate D0′, R10): six, the first six in the transaction's own order, never re-ranked. The plans'
 * older eight was superseded by the board.
 */
export const FOLD_HEAD = 6;

/**
 * The fold (lane D3): what a page shows of each sheet. A sheet draws its first `head` cells, the
 * cells the page keeps and the continuity partner of every cell it draws, in the transaction's
 * order, and one row for the rest that says what it folds by counting (the folded cells' one owner
 * when they share one lock, else the count alone); it never sums. The page owns the open state (the
 * transaction page keeps it in its URL).
 */
export interface TxFold {
  /** The cells a sheet draws first: FOLD_HEAD on the transaction page, none in a cell's embed. */
  head: number;
  /** Cells drawn wherever they stand (the URL's out point, the cell page's own cell), by id. */
  keep: readonly string[];
  /** The sheets drawn whole. */
  open: Readonly<Record<Side, boolean>>;
  /**
   * Opens or folds a sheet in place. Absent, the rows hold no control: the cell page's way to the
   * rest is its one link to the transaction.
   */
  onToggle?: (side: Side, open: boolean) => void;
  /** What a sheet that draws none of its cells is not ("this cell", on the cell page). */
  subject?: string;
}

/**
 * The two sheets from lg up (seals by kind, C11): a transaction is two sheets of one contract,
 * what it consumed | what it created, side by side and one size (both stretch to the taller; a
 * shorter list leaves blank paper), each its own frame with its seam-side corners square, so the
 * pair reads as one frame cut by a 4 px seam through which the page ground shows. Each sheet keeps
 * a lane on its seam side, from its tiles' edge to its own edge (35 px of padding and the 1 px
 * border), wider than its outer margin as a bound book's gutter is, at every transaction, so a
 * tile's width never depends on what is drawn there. A line across the seam says one thing only:
 * this consumed cell continues as that created cell (the continuity links, dashed in their
 * kind's colour, in one overlay centred on the seam, lane + seam + lane wide). A transaction
 * pools capacity — no consumed cell is any created cell's source — so nothing else crosses: no
 * connector from a tile, the inflow source or to the fee (round 3: row-aligned stubs read as a
 * false 1:1 pairing of inputs and outputs), and with no continuity link no overlay at all.
 */
export const SHEETS = {
  /** The seam between the sheets, px. */
  seam: 4,
  /** Each sheet's lane on its seam side, px: from a tile's edge to the sheet's edge. */
  lane: 36,
} as const;

/**
 * The seam seal, from lg up (C2; C11): the transaction's stone at its narrow page height, 64 px
 * (104.5 wide), tilted as the page's head presses it (−4°), pressed across the seam and pulled
 * apart by it: its left half on the consumed sheet up to that sheet's edge, its right half on the
 * created sheet from that sheet's edge, the 4 px of ground between, nothing of the stone lost.
 * Tilted, it covers 108.7 × 71.1 px; centred in the heads' 95 px band (1 px border, 12 px padding,
 * the heads' 70 px, their 12 px margin) it keeps 11.9 px above and below, and each half reaches
 * 54.4 px into its sheet from the sheet's edge: the heads keep 24 px clear inside the 36 px lane.
 * Below lg, where the sheets stack, the same stone is pressed across the 4 px row between them,
 * 18 px from the right (lane D3; `SeamSeal`'s stacked form).
 */
export const SEAM_SEAL = {
  /** The stone's height, px: the transaction's narrow page height. */
  height: PAGE_SEAL_HEIGHTS.tx.narrow,
  /** The heads' band, px: from a sheet's top edge to its first row. */
  band: 95,
  /** How far each half reaches into its sheet from the sheet's seam edge, px (54.4, rounded up). */
  reach: 55,
} as const;

export interface TxStructureGroup {
  id: string;
  kind: 'lock' | 'type';
  label: string;
  href: string;
  cellIds: readonly string[];
  executed: boolean;
  witness: ReactNode;
  dependency: ReactNode;
  cycles?: string | null;
}
export interface TxContinuity {
  from: string;
  to: string;
  label: string;
  // group scripts (F2-5): 'type-id' = the same Type ID continues (a code cell replaced), olive.
  kind?: AssetClass | 'file' | 'type-id';
}
/** The dashed stroke of a continuity link, on the structure and in any legend beside it. */
export function continuityColor(kind: TxContinuity['kind']): string {
  return kind === 'file'
    ? 'var(--c-text)'
    : kind === 'type-id'
      ? CELL_COLORS.type
      : kind
        ? CLASS_COLORS[kind]
        : CLASS_COLORS.objects;
}
export interface TxStructureProps {
  txHash: string;
  inputs: readonly CellTileData[];
  outputs: readonly CellTileData[];
  fee: string;
  /**
   * The consumed sheet's total (C11): the exact shannons of the cells the transaction consumed,
   * as the API sums them (`inputsCapacity`), the head's third line; never summed from the tiles.
   */
  consumedCapacity?: string;
  /** The created sheet's total (C11): the API's exact `outputsCapacity`, as `consumedCapacity`. */
  createdCapacity?: string;
  groups?: readonly TxStructureGroup[];
  continuity?: readonly TxContinuity[];
  witnesses?: ReactNode;
  dependencies?: ReactNode;
  hideGroups?: boolean;
  /**
   * Lane U follow-up: the groups list's heading. Default "Script groups"; a page showing several
   * transactions names each list by its transaction (the cell page: "… of the transaction that
   * created it"), so it never reads as two bare "Script groups" sections.
   */
  groupsTitle?: string;
  focusedCellIds?: readonly string[];
  onClearFocus?: () => void;
  onCellHover?: (id: string | null) => void;
  /**
   * txviz `me_out`: the output this page follows. It stays lit and a dashed bridge carries it
   * past the outputs to the next step (Cell "Its transactions", Fiber channel steps).
   */
  meOut?: string;
  /** txviz `onward`: the legend for that bridge. */
  onward?: string;
  // F2-tx (round 2, Main.dc.html v5 / D4): the three props below are additive; their defaults keep
  // every other page's behaviour.
  /**
   * `page`: the page owns hover linking (cells ↔ script groups ↔ witnesses ↔ deps, TX-14); only
   * `focusedCellIds` light and every other cell dims. `continuity` (default): a hovered cell
   * also lights its continuity partner.
   */
  linking?: 'continuity' | 'page';
  /** Capacity entering from no cell: DAO compensation (gold, dashed) or a block reward. */
  inflow?: TxInflow;
  /** `false` when the page draws the continuity legend itself (in its section header). */
  continuityList?: boolean;
  // group scripts (F2-5): what the bridge leads to (Cell page "Not spent yet" box), drawn right
  // under the bridge legend instead of after the script groups.
  afterOnward?: ReactNode;
  /**
   * The transaction page's seam seal (`SEAM_SEAL`): the same stone its head presses, in the same
   * state, pressed again across the seam between what it consumed and what it created, from lg
   * up. Every other page leaves it out (a list's seal is whole).
   */
  seamSeal?: { id: TxSealId; state?: SealState };
  /** The fold (`TxFold`); absent, every cell is drawn (the Fiber channel's steps). */
  fold?: TxFold;
}
/** F2-tx (D4): a source that is not a consumed cell. */
export interface TxInflow {
  tone: 'dao' | 'reward';
  label: string;
  /** Exact shannons. */
  amount: string;
}
/** One hover selection ties cells, continuity and the script evidence together. */
export function TxStructure({
  txHash,
  inputs,
  outputs,
  fee,
  consumedCapacity,
  createdCapacity,
  groups = [],
  continuity = [],
  witnesses,
  dependencies,
  hideGroups = false,
  groupsTitle = 'Script groups',
  focusedCellIds = [],
  onClearFocus,
  onCellHover,
  meOut,
  onward = 'this cell, onward',
  linking = 'continuity',
  inflow,
  continuityList = true,
  afterOnward,
  seamSeal,
  fold,
}: TxStructureProps) {
  const [hover, setHover] = useState<{ kind: 'cell' | 'group'; id: string } | null>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const [centers, setCenters] = useState<Record<string, number>>({});
  const sheets = foldSheets(txHash, inputs, outputs, continuity, fold);
  // the tiles drawn: a fold that opens or closes moves them, so the links are measured again
  const drawnKey = [...sheets.consumed.shown, ...sheets.created.shown].map((c) => c.id).join(',');
  useLayoutEffect(() => {
    const root = canvas.current;
    if (!root) return;
    // the tiles' middles, for the continuity links and the onward bridge
    const measure = () => {
      const top = root.getBoundingClientRect().top;
      const next: Record<string, number> = {};
      root.querySelectorAll<HTMLElement>('[data-cell-id]').forEach((cell) => {
        const bounds = cell.getBoundingClientRect();
        next[cell.dataset.cellId!] = bounds.top - top + bounds.height / 2;
      });
      setCenters((previous) =>
        JSON.stringify(previous) === JSON.stringify(next) ? previous : next
      );
    };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(root);
    window.addEventListener('resize', measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', measure);
    };
    // (the inflow source's row moves the consumed tiles; a fold opened or closed moves them all)
  }, [inputs, outputs, inflow, drawnKey]);
  const ids = new Set([...inputs, ...outputs].map((c) => c.id));
  if (ids.size !== inputs.length + outputs.length)
    throw new Error(`${txHash}: duplicate cell id in transaction structure`);
  for (const g of groups)
    for (const id of g.cellIds)
      if (!ids.has(id))
        throw new Error(`${txHash}: script group ${g.id} references unknown cell ${id}`);
  for (const l of continuity)
    if (!inputs.some((c) => c.id === l.from) || !outputs.some((c) => c.id === l.to))
      throw new Error(`${txHash}: invalid continuity ${l.from} → ${l.to}`);
  if (meOut !== undefined && !outputs.some((c) => c.id === meOut))
    throw new Error(`${txHash}: onward cell ${meOut} is not an output`);
  const activeGroup = hover?.kind === 'group' ? groups.find((g) => g.id === hover.id) : null;
  const linked = new Set(
    linking === 'page'
      ? focusedCellIds
      : activeGroup
        ? activeGroup.cellIds
        : hover
          ? [
              hover.id,
              ...continuity.flatMap((l) =>
                l.from === hover.id ? [l.to] : l.to === hover.id ? [l.from] : []
              ),
            ]
          : focusedCellIds
  );
  const ys = Object.values(centers);
  const tile = (cell: CellTileData) => (
    <CellTile
      key={cell.id}
      cell={cell}
      highlighted={linked.has(cell.id) || cell.id === meOut}
      dimmed={
        ((linking === 'continuity' && Boolean(hover)) || focusedCellIds.length > 0) &&
        !linked.has(cell.id) &&
        cell.id !== meOut
      }
      onClick={focusedCellIds.includes(cell.id) ? onClearFocus : undefined}
      onHover={(id) => {
        if (linking === 'continuity') setHover(id ? { kind: 'cell', id } : null);
        onCellHover?.(id);
      }}
    />
  );
  const sheet = (side: Side, cells: readonly CellTileData[], total: string | undefined) => {
    const { shown, folded } = sheets[side];
    return (
      <TerminalPanel data-sheet={side} className="min-w-0">
        {/* the sheet's lane on its seam side (SHEETS.lane: 35 px and the 1 px border); 12 px on
          the outer side, above and under its rows, which stay at the top (the fee directly under
          the last created tile) when the sheet stretches to its pair */}
        <div
          data-sheet-content=""
          className={cn(
            'p-3',
            side === 'consumed' ? 'lg:pr-[35px]' : 'lg:pl-[35px]',
            // below lg the seam seal's upper half lies in the consumed sheet's last 35.6 px: 44 px
            // of padding keep it clear of the sheet's last row and its control (SEAM_SEAL)
            seamSeal && side === 'consumed' && 'max-lg:pb-11'
          )}
        >
          <SheetHead
            txHash={txHash}
            side={side}
            count={cells.length}
            total={total}
            sealed={seamSeal !== undefined}
          />
          <div data-sheet-rows="" className="space-y-3">
            {side === 'consumed' && inflow && <InflowSource txHash={txHash} inflow={inflow} />}
            {shown.map(tile)}
            {fold && (folded.length > 0 || sheets[side].opened) && (
              <FoldRow
                side={side}
                count={cells.length}
                shown={shown.length}
                folded={folded}
                subject={fold.subject}
                onToggle={fold.onToggle && ((open) => fold.onToggle!(side, open))}
              />
            )}
            {side === 'created' && <FeeRow txHash={txHash} fee={fee} />}
          </div>
        </div>
      </TerminalPanel>
    );
  };
  // The overlay's x, px: the consumed tiles' edge and the created tiles' edge (lane, seam, lane).
  const [tilesIn, tilesOut] = [0, 2 * SHEETS.lane + SHEETS.seam];
  return (
    <div className="min-w-0 space-y-6" aria-label={`Transaction ${txHash} structure`}>
      {/* Two sheets and the seam (seals by kind, C11; the board's two sheets of one contract):
          each sheet an instrument frame, its head its eyebrow; the prose and lists under them
          stay flat. From lg they lie side by side, one size (stretched to the taller), with the
          4 px seam between, the continuity links drawn over the seam and the sheets' lanes; below
          lg they stack, each its own height, the seam the 4 px row between them (lane D3, the
          board's phone: the sheets of one contract, as from lg; they were 12 px apart). */}
      <div
        ref={canvas}
        data-tx-canvas=""
        role="group"
        aria-label="Cells consumed and created"
        data-testid="tx-cells-frame"
        className={cn(
          'relative grid min-w-0 grid-cols-1 items-stretch',
          meOut === undefined
            ? 'lg:grid-cols-[minmax(0,1fr)_4px_minmax(0,1fr)]'
            : 'lg:grid-cols-[minmax(0,1fr)_4px_minmax(0,1fr)_96px]'
        )}
      >
        {sheet('consumed', inputs, consumedCapacity)}
        <div data-seam="" className="relative h-1 self-stretch lg:h-auto">
          {continuity.length > 0 && (
            // Only the cells that continue (round 3): each link from its consumed tile's edge
            // across both lanes and the seam to its created tile's edge, from lg (below it the
            // sheets stack and no line crosses between them).
            <svg
              aria-label="Cells that continue from consumed to created"
              className="pointer-events-none absolute inset-y-0 -left-9 z-20 hidden h-full w-[76px] overflow-visible lg:block"
            >
              {ys.length > 0 &&
                // a link between two drawn tiles; a folded cell has no tile to start or end at
                continuity
                  .filter((link) => sheets.drawn.has(link.from) && sheets.drawn.has(link.to))
                  .map((link) => (
                    <path
                      key={`${link.from}:${link.to}`}
                      data-continuity-kind={link.kind}
                      d={`M${tilesIn} ${centers[link.from]}C30 ${centers[link.from]} 46 ${centers[link.to]} ${tilesOut} ${centers[link.to]}`}
                      fill="none"
                      stroke={continuityColor(link.kind)}
                      strokeWidth={linked.has(link.from) || linked.has(link.to) ? 2 : 1.3}
                      strokeDasharray="4 3"
                    />
                  ))}
            </svg>
          )}
          {seamSeal && (
            <>
              <SeamSeal id={seamSeal.id} state={seamSeal.state} form="side-by-side" />
              <SeamSeal id={seamSeal.id} state={seamSeal.state} form="stacked" />
            </>
          )}
        </div>
        {sheet('created', outputs, createdCapacity)}
        {meOut !== undefined && (
          <div
            data-onward-bridge
            data-onward-from={meOut}
            aria-hidden="true"
            className="relative hidden self-stretch lg:block"
          >
            <svg className="pointer-events-none absolute inset-0 z-20 h-full w-full overflow-visible">
              {centers[meOut] !== undefined && (
                // from the followed tile's right edge, inside the sheet's 12 px padding and its
                // 1 px border, out of the sheet
                <path
                  d={`M-13 ${centers[meOut]}H96`}
                  className="stroke-text-bright"
                  strokeWidth="1.2"
                  strokeDasharray="3 3"
                />
              )}
            </svg>
          </div>
        )}
      </div>
      {meOut !== undefined && (
        <p className="text-text-dim text-meta flex items-center gap-1.5">
          <svg aria-hidden="true" width="22" height="6">
            <path
              d="M0 3h22"
              className="stroke-text-bright"
              strokeWidth="1.2"
              strokeDasharray="3 3"
            />
          </svg>
          {onward}
        </p>
      )}
      {meOut !== undefined && afterOnward}
      {continuityList && continuity.length > 0 && (
        <ul
          aria-label="Cell continuity"
          className="text-text-dim text-meta flex flex-wrap gap-x-6 gap-y-2"
        >
          {continuity.map((l) => (
            <li key={`${l.from}:${l.to}`}>
              <button
                type="button"
                onMouseEnter={() => {
                  setHover({ kind: 'cell', id: l.from });
                  onCellHover?.(l.from);
                }}
                onMouseLeave={() => {
                  setHover(null);
                  onCellHover?.(null);
                }}
                onFocus={() => {
                  setHover({ kind: 'cell', id: l.from });
                  onCellHover?.(l.from);
                }}
                onBlur={() => {
                  setHover(null);
                  onCellHover?.(null);
                }}
                className="hover:text-text flex items-center gap-2"
              >
                <svg aria-hidden="true" width="22" height="6" className="shrink-0">
                  <path
                    d="M0 3h22"
                    stroke={continuityColor(l.kind)}
                    strokeWidth="1.3"
                    strokeDasharray="4 3"
                  />
                </svg>
                {unitNode(l.from)} → {l.to} · {l.label}
              </button>
            </li>
          ))}
        </ul>
      )}
      {!hideGroups && groups.length > 0 && (
        <section>
          <h3 className="text-text-bright text-body mb-3 font-medium">{groupsTitle}</h3>
          <div className="divide-row-rule divide-y">
            {groups.map((g) => (
              <div
                key={g.id}
                data-group-id={g.id}
                data-highlighted={
                  hover?.id === g.id || (hover?.kind === 'cell' && g.cellIds.includes(hover.id))
                }
                tabIndex={0}
                onMouseEnter={() => setHover({ kind: 'group', id: g.id })}
                onMouseLeave={() => setHover(null)}
                onFocus={() => setHover({ kind: 'group', id: g.id })}
                onBlur={() => setHover(null)}
                className={cn(
                  'text-small grid min-w-0 gap-2 py-3 sm:grid-cols-2 lg:grid-cols-4',
                  (hover?.id === g.id ||
                    (hover?.kind === 'cell' && g.cellIds.includes(hover.id))) &&
                    'bg-base-surface'
                )}
              >
                <span className="min-w-0 [overflow-wrap:anywhere]">
                  <span className="text-text-dim mr-2">{g.kind}</span>
                  <Link href={g.href} className="text-text-bright hover:text-jade">
                    {g.label}
                  </Link>
                  <span className="text-text-dim ml-2">
                    {g.executed ? 'executed' : 'not executed'}
                  </span>
                </span>
                <span className="text-text-dim min-w-0 break-all">{g.cellIds.join(', ')}</span>
                <div className="min-w-0 [overflow-wrap:anywhere]">{g.witness}</div>
                <div className="min-w-0 [overflow-wrap:anywhere]">
                  {g.dependency}
                  {g.cycles !== undefined && (
                    <span className="text-text-dim block">
                      {g.cycles === null
                        ? 'cycles not available'
                        : `${exactUnsignedInteger(g.cycles, `${txHash}: ${g.id} cycles`).toLocaleString('en-US')} cycles`}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}
      {witnesses && (
        <section>
          <h3 className="text-text-bright text-body mb-3 font-medium">Witnesses</h3>
          {witnesses}
        </section>
      )}
      {dependencies && (
        <section>
          <h3 className="text-text-bright text-body mb-3 font-medium">Dependencies</h3>
          {dependencies}
        </section>
      )}
    </div>
  );
}

/** A sheet of the transaction: what it consumed, or what it created. */
type Side = 'consumed' | 'created';

/** One sheet under the fold: the cells it draws and the cells it folds, both in order. */
interface FoldedSheet {
  shown: readonly CellTileData[];
  folded: readonly CellTileData[];
  /** Drawn whole by the page, with cells a closed fold would hide: its row is the way back. */
  opened: boolean;
}

/**
 * What each sheet draws under the fold (`TxFold`): with no fold, every cell. Folded, a sheet draws
 * its first `head` cells, the cells the page keeps, and the continuity partner of any of those
 * (a drawn cell's link is never cut by the fold), in the transaction's order; opened, all of them.
 * A kept cell the transaction does not have is refused, with the transaction named.
 */
function foldSheets(
  txHash: string,
  inputs: readonly CellTileData[],
  outputs: readonly CellTileData[],
  continuity: readonly TxContinuity[],
  fold: TxFold | undefined
): Record<Side, FoldedSheet> & { drawn: ReadonlySet<string> } {
  const all = { consumed: inputs, created: outputs };
  if (!fold) {
    const whole = (cells: readonly CellTileData[]) => ({ shown: cells, folded: [], opened: false });
    return {
      consumed: whole(inputs),
      created: whole(outputs),
      drawn: new Set([...inputs, ...outputs].map((c) => c.id)),
    };
  }
  const ids = new Set([...inputs, ...outputs].map((c) => c.id));
  for (const id of fold.keep)
    if (!ids.has(id))
      throw new Error(`${txHash}: the fold keeps cell ${id}, which the transaction does not have`);
  // the cells drawn on their own: each sheet's head, the kept ones, every cell of an open sheet;
  // then the continuity partner of each of them
  const own = (closedOnly: boolean) => {
    const set = new Set<string>(fold.keep);
    for (const side of ['consumed', 'created'] as const)
      all[side]
        .slice(0, !closedOnly && fold.open[side] ? undefined : fold.head)
        .forEach((c) => set.add(c.id));
    for (const l of continuity) {
      if (set.has(l.from)) set.add(l.to);
      else if (set.has(l.to)) set.add(l.from);
    }
    return set;
  };
  const drawn = own(false);
  // what a closed sheet would draw: an open sheet with more than that has a way back
  const closed = own(true);
  const sheet = (side: Side): FoldedSheet => ({
    shown: all[side].filter((c) => drawn.has(c.id)),
    folded: all[side].filter((c) => !drawn.has(c.id)),
    opened: fold.open[side] && all[side].some((c) => !closed.has(c.id)),
  });
  return { consumed: sheet('consumed'), created: sheet('created'), drawn };
}

/**
 * The way back from an opened fold of the transaction's lists (lane D3: a sheet's cells, the
 * developer tier's empty witnesses), in the rows' fold's words, "fewer ↑", set as the Pager's
 * short-list part it answers ("all 158 cells ↓"): the Pager has no way back of its own.
 */
export function FoldBack({ onFold }: { onFold: () => void }) {
  return (
    <button
      type="button"
      aria-expanded={true}
      onClick={onFold}
      className="text-jade text-small inline-flex h-8 items-center underline-offset-4 hover:underline sm:h-7"
    >
      fewer ↑
    </button>
  );
}

/**
 * The fold row (lane D3; the board's dashed row): what the sheet folds, by counting — "152 more
 * cells from ckb1…sqylvfrs" when every folded cell has one lock (the same lock, not the same
 * shortened address), else "152 more cells consumed"; where the sheet draws none of its cells and
 * the page says what they are not, ", none of them this cell". It never sums: the sheet's total
 * is the head's, the API's. The way to the rest is the short list's form, the Pager's
 * "all 158 cells ↓"; opened, the row stands after the last cell and folds the sheet again with the
 * rows' fold's words, "fewer ↑". Without a control (the cell page) the row is the count alone.
 */
function FoldRow({
  side,
  count,
  shown,
  folded,
  subject,
  onToggle,
}: {
  side: Side;
  count: number;
  shown: number;
  folded: readonly CellTileData[];
  subject?: string;
  onToggle?: (open: boolean) => void;
}) {
  const words = () => {
    // an owner shared by the folded cells: two or more with one lock (one folded cell is counted,
    // "1 more cell created", as the board writes it)
    const lock = folded[0].seal.id.lock;
    const shared = folded.length > 1 && folded.every((c) => c.seal.id.lock === lock);
    const not =
      shown === 0 && subject !== undefined
        ? folded.length === 1
          ? `, not ${subject}`
          : `, none of them ${subject}`
        : '';
    return (
      <span data-fold-words="" className="min-w-0">
        {unitNode(
          shown > 0
            ? formatCount(folded.length, 'more cell', 'more cells')
            : formatCount(folded.length, 'cell')
        )}{' '}
        {shared ? (
          <>
            from <span className="whitespace-nowrap">{folded[0].owner}</span>
          </>
        ) : (
          side
        )}
        {not}
      </span>
    );
  };
  return (
    <div
      data-fold={side}
      className="text-small border-text-dim/60 text-text flex flex-wrap items-center gap-x-3 gap-y-1 border border-dashed px-3 py-1.5"
    >
      {folded.length > 0 && words()}
      {onToggle && (
        <div className="sm:ml-auto">
          {folded.length > 0 ? (
            <Pager mode="more" all={{ count, singular: 'cell', onShow: () => onToggle(true) }} />
          ) : (
            <FoldBack onFold={() => onToggle(false)} />
          )}
        </div>
      )}
    </div>
  );
}

/**
 * A sheet's head (C11; the board's label, value and sub-line): the eyebrow label, the count of
 * its cells and, when the caller has it, the API's exact total of their capacity. One h3, named
 * "5 cells consumed" as the old column heads were: the count comes first in the markup and the
 * label is drawn above it. The created head is mirrored from lg (right-aligned); below lg both
 * read from the left. With the seam seal the heads hold its band and keep 24 px clear on their
 * seam side (`SEAM_SEAL`).
 */
function SheetHead({
  txHash,
  side,
  count,
  total,
  sealed,
}: {
  txHash: string;
  side: Side;
  count: number;
  total: string | undefined;
  sealed: boolean;
}) {
  const mirrored = side === 'created' && 'lg:items-end lg:text-right';
  return (
    <div
      data-sheet-head={side}
      className={cn(
        'mb-3 flex flex-col',
        mirrored,
        sealed && 'lg:min-h-[70px]',
        sealed && (side === 'consumed' ? 'lg:pr-6' : 'lg:pl-6')
      )}
    >
      <h3 className={cn('flex flex-col', mirrored)}>
        <span className="text-text-bright text-section">
          {unitNode(formatCount(count, 'cell'))}
        </span>{' '}
        <span className="eyebrow text-text-dim text-meta order-first">{side}</span>
      </h3>
      {total !== undefined && (
        <span data-sheet-total="" className="text-small">
          <CkbAmount
            form="column"
            shannons={exactUnsignedInteger(total, `${txHash}: ${side} capacity`)}
            signed={false}
            tone="plain"
          />
        </span>
      )}
    </div>
  );
}

/**
 * The seam seal (C11): one stone in an `aria-hidden` holder centred on the seam, drawn twice —
 * the first clipped to its left half and moved 2 px left, the second over it clipped to its right
 * half and moved 2 px right — so each half lies on its own sheet up to the sheet's edge and the
 * seam's 4 px of ground runs between them. The clip's outer insets are −8 px so the tilt's
 * overhang (2.1 px across, 3.6 px down) is kept. The library's `seamPx` is not used: its gap
 * tilts with the stone and would not meet the real seam.
 *
 * Two forms, one shown at a width (lane D3, PA-07, the board's phone): `side-by-side`, from lg,
 * in the heads' band over the vertical seam; `stacked`, below lg, where the sheets stack and the
 * seam is the 4 px row between them, the stone at the right of that row, cut across: its upper
 * half clipped at its middle and moved 2 px up onto the consumed sheet, its lower half moved 2 px
 * down onto the created sheet. Tilted it reaches 35.6 px past each sheet's edge: into the consumed
 * sheet's bottom padding (44 px there below lg) and over the created head's first line at the
 * right, where the head (reading from the left below lg) writes nothing and holds no control.
 */
function SeamSeal({
  id,
  state,
  form,
}: {
  id: TxSealId;
  state?: SealState;
  form: 'side-by-side' | 'stacked';
}) {
  const half = (side: Side) => (
    <span
      data-seam-half={side}
      className={cn(
        'flex',
        form === 'side-by-side'
          ? side === 'consumed'
            ? '-translate-x-0.5 [clip-path:inset(-8px_50%_-8px_-8px)]'
            : 'absolute inset-0 translate-x-0.5 [clip-path:inset(-8px_-8px_-8px_50%)]'
          : side === 'consumed'
            ? '-translate-y-0.5 [clip-path:inset(-8px_-8px_50%_-8px)]'
            : 'absolute inset-0 translate-y-0.5 [clip-path:inset(50%_-8px_-8px_-8px)]'
      )}
    >
      <Seal
        id={id}
        form="page"
        height={SEAM_SEAL.height}
        state={state}
        tilt={PAGE_SEAL_TILT}
        className="[&>svg]:block"
      />
    </span>
  );
  // The head's own stone is the page's image for a screen reader; this second impression is
  // hidden from it, and from the pointer.
  return (
    <span
      data-seam-seal={form}
      aria-hidden="true"
      className={
        form === 'side-by-side'
          ? 'pointer-events-none absolute left-1/2 top-0 z-20 hidden h-[95px] -translate-x-1/2 items-center lg:flex'
          : 'pointer-events-none absolute right-[18px] top-1/2 z-20 flex -translate-y-1/2 lg:hidden'
      }
    >
      <span className="relative flex">
        {half('consumed')}
        {half('created')}
      </span>
    </span>
  );
}

/**
 * The fee (C11): the created sheet's last row, the mirror of the inflow source on the consumed
 * side — capacity that leaves the transaction without becoming a cell. Dashed, in the dim tone;
 * no line reaches it across the seam (round 3).
 */
function FeeRow({ txHash, fee }: { txHash: string; fee: string }) {
  return (
    <div
      data-fee=""
      data-testid="tx-fee"
      className="text-small border-text-dim/60 text-text border border-dashed px-3 py-2"
    >
      <span className="block">fee</span>
      <CkbAmount
        form="column"
        shannons={exactUnsignedInteger(fee, `${txHash}: fee`)}
        signed={false}
        tone="plain"
      />
    </div>
  );
}
/**
 * F2-tx (D4): a source that is not a consumed cell, the consumed sheet's first row; like the fee
 * row, nothing connects it across the seam (round 3).
 */
function InflowSource({ txHash, inflow }: { txHash: string; inflow: TxInflow }) {
  return (
    <div
      data-inflow={inflow.tone}
      data-testid="tx-inflow"
      className={cn(
        'text-small border border-dashed px-3 py-2',
        inflow.tone === 'dao' ? 'border-gold/60 text-gold' : 'border-text-dim/60 text-text'
      )}
    >
      <span className="block">{inflow.label}</span>
      <CkbAmount
        form="column"
        shannons={exactUnsignedInteger(inflow.amount, `${txHash}: ${inflow.label}`)}
        signed={false}
        tone="plain"
      />
    </div>
  );
}
