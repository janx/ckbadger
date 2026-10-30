'use client';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { CellTile, type CellTileData } from '@/components/ui/cell-tile';
import { ExactCkb } from '@/components/activity-event-row';
import Link from '@/components/ui/link';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { cn } from '@/lib/utils';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';

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
  kind?: AssetClass | 'file';
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
  focusedCellIds = [],
  onClearFocus,
  onCellHover,
  meOut,
  onward = 'this cell, onward',
}: TxStructureProps) {
  const continuityColor = (kind: TxContinuity['kind']) =>
    kind === 'file' ? '#a0a8be' : kind ? CLASS_COLORS[kind] : CLASS_COLORS.objects;
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
    activeGroup
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
      <h3 className="text-text-dim mb-3 text-xs">
        {formatCount(cells.length, 'cell')} {label}
      </h3>
      <div className="space-y-3">
        {cells.map((cell) => (
          <CellTile
            key={cell.id}
            cell={cell}
            highlighted={linked.has(cell.id) || cell.id === meOut}
            dimmed={
              (Boolean(hover) || focusedCellIds.length > 0) &&
              !linked.has(cell.id) &&
              cell.id !== meOut
            }
            onClick={focusedCellIds.includes(cell.id) ? onClearFocus : undefined}
            onHover={(id) => {
              setHover(id ? { kind: 'cell', id } : null);
              onCellHover?.(id);
            }}
          />
        ))}
      </div>
    </div>
  );
  return (
    <div className="min-w-0 space-y-6" aria-label={`Transaction ${txHash} structure`}>
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
        <div className="text-text-dim relative flex flex-wrap items-center justify-center gap-2 py-3 text-center text-[11px] lg:block lg:min-h-[180px] lg:p-0">
          <span className="lg:absolute lg:inset-x-0 lg:top-0">Capacity pool</span>
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
                    stroke={linked.has(cell.id) ? '#dee2ec' : '#4b5266'}
                    strokeWidth="1.5"
                  />
                ))}
                {outputs.map((cell) => (
                  <path
                    key={cell.id}
                    d={`M66 ${centers[cell.id]}H112`}
                    stroke={linked.has(cell.id) || cell.id === meOut ? '#dee2ec' : '#4b5266'}
                    strokeWidth="1.5"
                  />
                ))}
                <rect
                  x="46"
                  y={poolTop}
                  width="20"
                  height={poolBottom - poolTop}
                  rx="4"
                  fill="#181d2b"
                  stroke="#4b5266"
                />
                <path d={`M56 ${poolBottom}v16`} stroke="#4b5266" />
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
      {meOut !== undefined && (
        <p className="text-text-dim flex items-center gap-1.5 text-[11px]">
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
      {poolCapacity !== undefined && (
        <p className="text-text-dim text-[11px]">
          Capacity entering the pool{' '}
          <ExactCkb
            shannons={exactUnsignedInteger(poolCapacity, `${txHash}: pool capacity`)}
            signed={false}
            tone="plain"
          />
        </p>
      )}
      {continuity.length > 0 && (
        <ul
          aria-label="Cell continuity"
          className="text-text-dim flex flex-wrap gap-x-6 gap-y-2 text-[11px]"
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
          <h3 className="text-text-bright mb-3 text-[15px] font-medium">Script groups</h3>
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
                  'grid min-w-0 gap-2 py-3 text-xs sm:grid-cols-2 lg:grid-cols-4',
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
          <h3 className="text-text-bright mb-3 text-[15px] font-medium">Witnesses</h3>
          {witnesses}
        </section>
      )}
      {dependencies && (
        <section>
          <h3 className="text-text-bright mb-3 text-[15px] font-medium">Dependencies</h3>
          {dependencies}
        </section>
      )}
    </div>
  );
}
