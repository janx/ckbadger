'use client';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { CellTile, type CellTileData } from '@/components/ui/cell-tile';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { ExactCkb } from '@/components/activity-event-row';
import Link from '@/components/ui/link';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import { Seal } from '@/components/ui/seal';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import type { SealId, SealState } from '@/lib/seal/stones';
import { cn } from '@/lib/utils';
import { CELL_COLORS, CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';

/** A transaction's stone (the seals by kind): its hash and the cells it consumed and created. */
export type TxSealId = Extract<SealId, { kind: 'tx' }>;

/**
 * The seam seal, from lg up (the seals by kind, C2): the transaction's stone at its narrow page
 * height, 64 px (104.5 wide), tilted as the page's head presses it (−4°), at the head of the
 * 112 px capacity-pool column, centred: its consumed rods toward the consumed cells, its created
 * rods toward the created ones. Tilted, it covers 108.7 × 71.1 px — 1.6 px inside the column at
 * either side, 3.6 px past its box above (into the panel's 12 px padding) and below, to 67.6 px.
 * The column's "Capacity pool" label moves under it, to 72 px (a 16.5 px line, to 88.5); both side
 * columns' heads hold the band to 88 px, so the first cell's middle is at least 88 + 27 (half the
 * 54 px shortest tile) = 115 px and the pool, 16 px above it, starts under the label: every flow
 * stays below the stone and the label. Below lg, where the cells stack, there is none.
 */
export const SEAM_SEAL = {
  /** The stone's height, px: the transaction's narrow page height. */
  height: PAGE_SEAL_HEIGHTS.tx.narrow,
  /** The label's top in the pool column, px. */
  labelTop: 72,
  /** Where the cells start in the side columns, px (their heads' height). */
  band: 88,
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
  /** Exact sum supplied by the API; never reconstructed from partially loaded cells. */
  poolCapacity?: string;
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
  /** Capacity entering the pool from no cell: DAO compensation (gold, dashed) or a block reward. */
  inflow?: TxInflow;
  /** `false` when the page draws the continuity legend itself (in its section header). */
  continuityList?: boolean;
  // group scripts (F2-5): what the bridge leads to (Cell page "Not spent yet" box), drawn right
  // under the bridge legend instead of after the script groups.
  afterOnward?: ReactNode;
  /**
   * The transaction page's seam seal (`SEAM_SEAL`): the same stone its head presses, in the same
   * state, pressed again between what it consumed and what it created, from lg up. Every other
   * page leaves it out (a list's seal is whole).
   */
  seamSeal?: { id: TxSealId; state?: SealState };
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
  poolCapacity,
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
}: TxStructureProps) {
  const [hover, setHover] = useState<{ kind: 'cell' | 'group'; id: string } | null>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const [centers, setCenters] = useState<Record<string, number>>({});
  useLayoutEffect(() => {
    const root = canvas.current;
    if (!root) return;
    const measure = () => {
      const top = root.getBoundingClientRect().top;
      const next: Record<string, number> = {};
      root.querySelectorAll<HTMLElement>('[data-cell-id]').forEach((cell) => {
        const bounds = cell.getBoundingClientRect();
        next[cell.dataset.cellId!] = bounds.top - top + bounds.height / 2;
      });
      // F2-tx (D4): the inflow source joins the pool like a consumed cell.
      root.querySelectorAll<HTMLElement>('[data-inflow]').forEach((source) => {
        const bounds = source.getBoundingClientRect();
        next[INFLOW_ID] = bounds.top - top + bounds.height / 2;
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
  }, [inputs, outputs]);
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
  const poolTop = ys.length ? Math.min(...ys) - 16 : 0;
  const poolBottom = ys.length ? Math.max(...ys) + 16 : 0;
  const column = (cells: readonly CellTileData[], label: string) => (
    <div className="min-w-0">
      <h3
        className={cn(
          'text-text-dim text-small mb-3',
          // the seam seal's band (SEAM_SEAL.band): the cells start under the stone and its label
          seamSeal && 'lg:mb-0 lg:min-h-[88px]'
        )}
      >
        {formatCount(cells.length, 'cell')} {label}
      </h3>
      <div className="space-y-3">
        {label === 'consumed' && inflow && <InflowSource txHash={txHash} inflow={inflow} />}
        {cells.map((cell) => (
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
        ))}
      </div>
    </div>
  );
  return (
    <div className="min-w-0 space-y-6" aria-label={`Transaction ${txHash} structure`}>
      {/* Midnight, continued (move 3): the cells diagram is an instrument, so it sits in the one
          frame; the column heads inside it are its eyebrows. The prose and lists under it stay
          flat. */}
      <TerminalPanel aria-label="Cells consumed and created" data-testid="tx-cells-frame">
        <TerminalPanelContent padding="md">
          <div
            ref={canvas}
            data-tx-canvas
            className={cn(
              'relative grid min-w-0 grid-cols-1 items-stretch gap-4 pb-8 lg:gap-0',
              meOut === undefined
                ? 'lg:grid-cols-[minmax(0,1fr)_112px_minmax(0,1fr)]'
                : 'lg:grid-cols-[minmax(0,1fr)_112px_minmax(0,1fr)_96px]'
            )}
          >
            {column(inputs, 'consumed')}
            <div className="text-text-dim text-meta relative flex flex-wrap items-center justify-center gap-2 py-3 text-center lg:block lg:min-h-[180px] lg:p-0">
              {seamSeal && (
                // The head's own stone is the page's image for a screen reader; this second
                // impression is hidden from it.
                <span
                  data-seam-seal=""
                  aria-hidden="true"
                  className="absolute inset-x-0 top-0 hidden justify-center lg:flex"
                >
                  <Seal
                    id={seamSeal.id}
                    form="page"
                    height={SEAM_SEAL.height}
                    state={seamSeal.state}
                    tilt={PAGE_SEAL_TILT}
                    className="[&>svg]:block"
                  />
                </span>
              )}
              <span
                className={cn(
                  'lg:absolute lg:inset-x-0',
                  // under the seam seal (SEAM_SEAL.labelTop), else at the column's head
                  seamSeal ? 'lg:top-[72px]' : 'lg:top-0'
                )}
              >
                Capacity pool
              </span>
              <span aria-hidden="true" className="lg:hidden">
                ↓
              </span>
              <svg
                aria-label="Cells flow through the capacity pool"
                className="pointer-events-none absolute inset-0 hidden h-full w-full overflow-visible lg:block"
              >
                {ys.length > 0 && (
                  <>
                    {inputs.map((cell) => (
                      <path
                        key={cell.id}
                        d={`M0 ${centers[cell.id]}H46`}
                        stroke={linked.has(cell.id) ? 'var(--c-text-bright)' : 'var(--c-ink-quiet)'}
                        strokeWidth="1.5"
                      />
                    ))}
                    {inflow && centers[INFLOW_ID] !== undefined && (
                      <path
                        data-inflow-path={inflow.tone}
                        d={`M0 ${centers[INFLOW_ID]}H46`}
                        className={inflow.tone === 'dao' ? 'stroke-gold' : 'stroke-text-dim'}
                        strokeWidth="1.5"
                        strokeDasharray="4 3"
                      />
                    )}
                    {outputs.map((cell) => (
                      <path
                        key={cell.id}
                        d={`M66 ${centers[cell.id]}H112`}
                        stroke={
                          linked.has(cell.id) || cell.id === meOut
                            ? 'var(--c-text-bright)'
                            : 'var(--c-ink-quiet)'
                        }
                        strokeWidth="1.5"
                      />
                    ))}
                    <rect
                      x="46"
                      y={poolTop}
                      width="20"
                      height={poolBottom - poolTop}
                      rx="4"
                      fill="var(--c-tile)"
                      stroke="var(--c-ink-quiet)"
                    />
                    <path d={`M56 ${poolBottom}v16`} stroke="var(--c-ink-quiet)" />
                    {continuity.map((link) => (
                      <path
                        key={`${link.from}:${link.to}`}
                        data-continuity-kind={link.kind}
                        d={`M0 ${centers[link.from]}C44 ${centers[link.from]} 68 ${centers[link.to]} 112 ${centers[link.to]}`}
                        fill="none"
                        stroke={continuityColor(link.kind)}
                        strokeWidth={linked.has(link.from) || linked.has(link.to) ? 2 : 1.3}
                        strokeDasharray="4 3"
                      />
                    ))}
                  </>
                )}
              </svg>
              <div className="space-y-1 lg:absolute lg:inset-x-0" style={{ top: poolBottom + 20 }}>
                <span className="block">fee</span>
                <ExactCkb
                  shannons={exactUnsignedInteger(fee, `${txHash}: fee`)}
                  signed={false}
                  tone="plain"
                />
              </div>
            </div>
            {column(outputs, 'created')}
            {meOut !== undefined && (
              <div
                data-onward-bridge
                data-onward-from={meOut}
                aria-hidden="true"
                className="relative hidden lg:block"
              >
                <svg className="pointer-events-none absolute inset-0 h-full w-full overflow-visible">
                  {centers[meOut] !== undefined && (
                    <path
                      d={`M0 ${centers[meOut]}H96`}
                      className="stroke-text-bright"
                      strokeWidth="1.2"
                      strokeDasharray="3 3"
                    />
                  )}
                </svg>
              </div>
            )}
          </div>
        </TerminalPanelContent>
      </TerminalPanel>
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
      {poolCapacity !== undefined && (
        <p className="text-text-dim text-meta">
          Capacity entering the pool{' '}
          <ExactCkb
            shannons={exactUnsignedInteger(poolCapacity, `${txHash}: pool capacity`)}
            signed={false}
            tone="plain"
          />
        </p>
      )}
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
                {l.from} → {l.to} · {l.label}
              </button>
            </li>
          ))}
        </ul>
      )}
      {!hideGroups && groups.length > 0 && (
        <section>
          <h3 className="text-text-bright text-body mb-3 font-medium">{groupsTitle}</h3>
          <div className="divide-base-elevated divide-y">
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
                <span className="min-w-0 break-all">
                  <span className="text-text-dim mr-2">{g.kind}</span>
                  <Link href={g.href} className="text-text-bright hover:text-jade">
                    {g.label}
                  </Link>
                  <span className="text-text-dim ml-2">
                    {g.executed ? 'executed' : 'not executed'}
                  </span>
                </span>
                <span className="text-text-dim min-w-0 break-all">{g.cellIds.join(', ')}</span>
                <div className="min-w-0 break-all">{g.witness}</div>
                <div className="min-w-0 break-all">
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

// F2-tx (D4): the inflow source tile and its measurement key.
const INFLOW_ID = '__inflow';
function InflowSource({ txHash, inflow }: { txHash: string; inflow: TxInflow }) {
  return (
    <div
      data-inflow={inflow.tone}
      data-testid="tx-inflow"
      className={cn(
        'text-small rounded border border-dashed px-3 py-2',
        inflow.tone === 'dao' ? 'border-gold/60 text-gold' : 'border-text-dim/60 text-text'
      )}
    >
      <span className="block">{inflow.label}</span>
      <ExactCkb
        shannons={exactUnsignedInteger(inflow.amount, `${txHash}: ${inflow.label}`)}
        signed={false}
        tone="plain"
      />
    </div>
  );
}
