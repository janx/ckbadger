'use client';
import { useState, type ReactNode } from 'react';
import { CellTile, type CellTileData } from '@/components/ui/cell-tile';
import { ExactCkb } from '@/components/activity-event-row';
import Link from '@/components/ui/link';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { cn } from '@/lib/utils';

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
}: TxStructureProps) {
  const [hover, setHover] = useState<{ kind: 'cell' | 'group'; id: string } | null>(null);
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
        : []
  );
  const column = (cells: readonly CellTileData[], label: string) => (
    <div className="min-w-0">
      <h3 className="text-text-dim mb-3 text-xs">
        {cells.length} {label}
      </h3>
      <div className="space-y-3">
        {cells.map((cell) => (
          <CellTile
            key={cell.id}
            cell={cell}
            highlighted={linked.has(cell.id)}
            dimmed={Boolean(hover) && !linked.has(cell.id)}
            onHover={(id) => setHover(id ? { kind: 'cell', id } : null)}
          />
        ))}
      </div>
    </div>
  );
  return (
    <div className="min-w-0 space-y-6" aria-label={`Transaction ${txHash} structure`}>
      <div className="grid min-w-0 grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_112px_minmax(0,1fr)]">
        {column(inputs, 'cells consumed')}
        <div className="text-text-dim flex flex-wrap items-center justify-center gap-2 py-3 text-center text-[11px] lg:flex-col lg:pt-12">
          <span aria-hidden="true" className="text-text">
            →
          </span>
          <span>Capacity pool</span>
          {poolCapacity !== undefined && (
            <ExactCkb
              shannons={exactUnsignedInteger(poolCapacity, `${txHash}: pool capacity`)}
              signed={false}
              tone="plain"
            />
          )}
          <span>fee</span>
          <ExactCkb
            shannons={exactUnsignedInteger(fee, `${txHash}: fee`)}
            signed={false}
            tone="plain"
          />
          <span aria-hidden="true" className="text-text">
            →
          </span>
        </div>
        {column(outputs, 'cells created')}
      </div>
      {continuity.length > 0 && (
        <ul
          aria-label="Cell continuity"
          className="text-text-dim flex flex-wrap gap-x-6 gap-y-2 text-[11px]"
        >
          {continuity.map((l) => (
            <li key={`${l.from}:${l.to}`}>
              <button
                type="button"
                onMouseEnter={() => setHover({ kind: 'cell', id: l.from })}
                onMouseLeave={() => setHover(null)}
                onFocus={() => setHover({ kind: 'cell', id: l.from })}
                onBlur={() => setHover(null)}
                className="hover:text-text"
              >
                {l.from} → {l.to} · {l.label}
              </button>
            </li>
          ))}
        </ul>
      )}
      {groups.length > 0 && (
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
                  'grid gap-2 py-3 text-xs sm:grid-cols-2 lg:grid-cols-4',
                  (hover?.id === g.id ||
                    (hover?.kind === 'cell' && g.cellIds.includes(hover.id))) &&
                    'bg-base-surface'
                )}
              >
                <span>
                  <span className="text-text-dim mr-2">{g.kind}</span>
                  <Link href={g.href} className="text-text-bright hover:text-jade">
                    {g.label}
                  </Link>
                  <span className="text-text-dim ml-2">
                    {g.executed ? 'executed' : 'not executed'}
                  </span>
                </span>
                <span className="text-text-dim">{g.cellIds.join(', ')}</span>
                <div>{g.witness}</div>
                <div>
                  {g.dependency}
                  {g.cycles !== undefined && (
                    <span className="text-text-dim block">
                      {g.cycles === null
                        ? 'cycles pending'
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
