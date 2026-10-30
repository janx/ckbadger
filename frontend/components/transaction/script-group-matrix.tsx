'use client';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import type { AssetClass } from '@/lib/design-tokens';
import type { TxHover, TxLinkSets } from '@/lib/tx-detail-model';
import { cn } from '@/lib/utils';

export interface MatrixColumn {
  id: string;
  /** What the cell holds, one word (`Spore`, `plain`). */
  top: string;
  label: string;
  /** Short capacity; null while the cell's capacity is not known (a pending input). */
  cap: string | null;
}
export interface MatrixGroup {
  id: string;
  kind: 'lock' | 'type';
  name: string;
  href?: string;
  mark?: AssetClass | null;
  sub: ReactNode;
  ran: boolean;
  cellIds: readonly string[];
  witness: ReactNode;
  code: ReactNode;
  /** Exact decimal cycles of a group that ran; null when not measured (or not run). */
  cycles: string | null;
  /** Shown instead of the cycles line (a pending transaction: "will run when committed"). */
  status?: string;
  focusTestId?: string;
  onFocus?: () => void;
}

const CELL_W = 72;
const ARROW_W = 36;
const SCRIPT_W = 250;
const WITNESS_W = 230;
const CODE_MIN = 180;

/**
 * "How the cells were verified" (Main.dc.html Script groups): rows are script groups, columns
 * are the cells (inputs | → | outputs). A group that ran has ● on each of its cells joined by a
 * line; an output lock that is only attached has ○ ("runs when spent"). Hover is reported to the
 * page, which owns the linking (TX-14); everything the hover does not link dims to 0.25.
 */
export function ScriptGroupMatrix({
  txHash,
  inputs,
  outputs,
  groups,
  lit,
  onHover,
}: {
  txHash: string;
  inputs: readonly MatrixColumn[];
  outputs: readonly MatrixColumn[];
  groups: readonly MatrixGroup[];
  lit: TxLinkSets | null;
  onHover: (hover: TxHover | null) => void;
}) {
  const order: (MatrixColumn | null)[] = [...inputs, null, ...outputs];
  const widths = order.map((c) => (c ? CELL_W : ARROW_W));
  const centers = widths.map((w, i) => widths.slice(0, i).reduce((a, b) => a + b, 0) + w / 2);
  const cellsWidth = widths.reduce((a, b) => a + b, 0);
  const template = `${SCRIPT_W}px ${WITNESS_W}px ${cellsWidth}px minmax(${CODE_MIN}px, 1fr)`;
  const cellTemplate = widths.map((w) => `${w}px`).join(' ');
  const index = new Map(order.flatMap((c, i) => (c ? [[c.id, i] as const] : [])));
  for (const g of groups)
    for (const id of g.cellIds)
      if (!index.has(id))
        throw new Error(`${txHash}: script group ${g.id} references unknown cell ${id}`);
  const dim = (on: boolean) => lit !== null && !on;
  return (
    <div className="overflow-x-auto">
      <div
        role="table"
        aria-label="Script groups"
        className="text-xs"
        style={{ minWidth: SCRIPT_W + WITNESS_W + cellsWidth + CODE_MIN }}
      >
        <div
          role="row"
          className="border-base-border-subtle text-text-dim grid items-end border-b pb-1.5 text-[11px]"
          style={{ gridTemplateColumns: template }}
        >
          <span role="columnheader">Script</span>
          <span role="columnheader">Witness it reads</span>
          <span role="columnheader" className="grid" style={{ gridTemplateColumns: cellTemplate }}>
            {order.map((c, i) =>
              c ? (
                <span
                  key={c.id}
                  data-column={c.id}
                  data-dimmed={dim(lit?.cells.has(c.id) ?? false)}
                  tabIndex={0}
                  onMouseEnter={() => onHover({ t: 'cell', id: c.id })}
                  onMouseLeave={() => onHover(null)}
                  onFocus={() => onHover({ t: 'cell', id: c.id })}
                  onBlur={() => onHover(null)}
                  className={cn(
                    'flex flex-col items-center text-center transition-opacity',
                    lit?.cells.has(c.id) ? 'text-text-bright' : 'text-text',
                    dim(lit?.cells.has(c.id) ?? false) && 'opacity-25'
                  )}
                >
                  <span className="text-text-ghost text-[9.5px]">{c.top}</span>
                  <span>{c.label}</span>
                  <span className="text-text-dim text-[10px]">{c.cap ?? ''}</span>
                </span>
              ) : (
                <span key={`arrow-${i}`} aria-hidden="true" className="text-text-ghost text-center">
                  →
                </span>
              )
            )}
          </span>
          <span role="columnheader" className="pl-4">
            Code, from cell dep
          </span>
        </div>
        {groups.map((g) => {
          const on = lit?.groups.has(g.id) ?? false;
          const members = g.cellIds.map((id) => index.get(id)!);
          const first = Math.min(...members);
          const last = Math.max(...members);
          return (
            <div
              key={g.id}
              role="row"
              tabIndex={0}
              data-group-row={g.id}
              data-lit={on}
              data-dimmed={dim(on)}
              onMouseEnter={() => onHover({ t: 'group', id: g.id })}
              onMouseLeave={() => onHover(null)}
              onFocus={() => onHover({ t: 'group', id: g.id })}
              onBlur={() => onHover(null)}
              className={cn(
                'border-base-elevated grid items-center border-b transition-opacity',
                g.ran ? 'text-text min-h-[54px]' : 'text-text-dim min-h-[46px]',
                on && 'bg-base-surface',
                dim(on) && 'opacity-25'
              )}
              style={{ gridTemplateColumns: template }}
            >
              <span role="cell" className="flex min-w-0 flex-col py-1.5 pr-3">
                <span className="flex min-w-0 items-center gap-1.5">
                  <span className="text-text-dim text-[11px]">{g.kind}</span>
                  {g.mark && <ClassMark kind={g.mark} size={12} />}
                  {g.href ? (
                    <Link
                      href={g.href}
                      className={cn(
                        'hover:text-jade truncate',
                        g.ran ? 'text-text-bright' : 'text-text'
                      )}
                    >
                      {g.name}
                    </Link>
                  ) : (
                    <span className="truncate">{g.name}</span>
                  )}
                </span>
                <span className="text-text-dim truncate">{g.sub}</span>
                <span className="text-text-dim">
                  {g.status ??
                    (g.ran
                      ? g.cycles === null
                        ? 'runs here · cycles not available'
                        : `${BigInt(g.cycles).toLocaleString('en-US')} cycles`
                      : 'attached, runs when spent')}
                </span>
              </span>
              <span role="cell" className="min-w-0 pr-3">
                {g.witness}
              </span>
              <span role="cell" className="relative h-7">
                <button
                  type="button"
                  data-testid={g.focusTestId}
                  aria-label={`Focus the ${g.kind} group ${g.name}`}
                  onClick={g.onFocus}
                  className="absolute inset-0 grid h-full items-center"
                  style={{ gridTemplateColumns: cellTemplate }}
                >
                  {last > first && (
                    <span
                      aria-hidden="true"
                      data-span
                      className={cn(
                        'absolute top-[13px] h-0.5',
                        g.ran ? 'bg-jade/45' : 'bg-text-dim/45'
                      )}
                      style={{ left: centers[first], width: centers[last] - centers[first] }}
                    />
                  )}
                  {order.map((c, i) =>
                    c === null ? (
                      <span
                        key={`arrow-${i}`}
                        aria-hidden="true"
                        className="border-base-border-subtle h-full border-l border-dashed"
                      />
                    ) : (
                      <span key={c.id} className="relative flex h-7 items-center justify-center">
                        {members.includes(i) && (
                          <span
                            aria-hidden="true"
                            data-dot={g.ran ? 'ran' : 'attached'}
                            className={cn(
                              'block rounded-full',
                              g.ran
                                ? 'bg-jade h-2.5 w-2.5'
                                : 'border-text-dim h-[9px] w-[9px] border-[1.2px]'
                            )}
                          />
                        )}
                      </span>
                    )
                  )}
                </button>
              </span>
              <span role="cell" className="min-w-0 pl-4">
                {g.code}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
