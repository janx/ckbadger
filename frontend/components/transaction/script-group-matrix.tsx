'use client';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { AssetClass } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';
import type { TxHover, TxLinkSets } from '@/lib/tx-detail-model';
import { cn } from '@/lib/utils';

export interface MatrixColumn {
  id: string;
  /** What the cell holds, one word (`Spore`, `plain`). */
  top: string;
  label: string;
  /** Short capacity; null while the cell's capacity is not known (a pending input). */
  cap: string | null;
  /** Lane L (L1): the cell's page; the label links to it. Absent: the label is text. */
  href?: string;
  /** The label's hover title: the full out point, or why the label is not a link. */
  title?: string;
}
/** The witness a group reads (the witness at its first cell's index). */
export interface MatrixWitness {
  index: number;
  /** Its kind (`WitnessArgs`) and size in bytes; null while the witness bytes are unavailable. */
  detail: { kind: string; size: number } | null;
  /** Select it in the page's witness inspector. */
  onSelect?: () => void;
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
  /** Null when the group reads no witness. */
  witness: MatrixWitness | null;
  /** The cell dep its code comes from; null when there is none to point at (an attached lock). */
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

/** `input 0, 1, 2`, `input 0 → output 0`, `output 1`: a group's cells in words. */
export function cellsInWords(
  cellIds: readonly string[],
  inputs: readonly MatrixColumn[],
  outputs: readonly MatrixColumn[]
): string {
  // The columns are the transaction's cells in order, so a column's position is its index.
  const side = (word: string, cells: readonly MatrixColumn[]) => {
    const at = cells.flatMap((c, i) => (cellIds.includes(c.id) ? [i] : []));
    return at.length ? `${word} ${at.join(', ')}` : null;
  };
  return [side('input', inputs), side('output', outputs)]
    .filter((part): part is string => part !== null)
    .join(' → ');
}

function cyclesLine(g: MatrixGroup, ranText: string): string {
  return (
    g.status ??
    (g.ran
      ? g.cycles === null
        ? ranText
        : `${BigInt(g.cycles).toLocaleString('en-US')} cycles`
      : 'attached, runs when spent')
  );
}

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
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const order: (MatrixColumn | null)[] = [...inputs, null, ...outputs];
  const widths = order.map((c) => (c ? CELL_W : ARROW_W));
  const centers = widths.map((w, i) => widths.slice(0, i).reduce((a, b) => a + b, 0) + w / 2);
  const cellsWidth = widths.reduce((a, b) => a + b, 0);
  const template = `${SCRIPT_W}px ${WITNESS_W}px ${cellsWidth}px minmax(${CODE_MIN}px, 1fr)`;
  const cellTemplate = widths.map((w) => `${w}px`).join(' ');
  const index = new Map(order.flatMap((c, i) => (c ? [[c.id, i] as const] : [])));
  for (const g of groups) {
    if (g.cellIds.length === 0) throw new Error(`${txHash}: script group ${g.id} has no cells`);
    for (const id of g.cellIds)
      if (!index.has(id))
        throw new Error(`${txHash}: script group ${g.id} references unknown cell ${id}`);
  }
  const dim = (on: boolean) => lit !== null && !on;
  // The matrix only where every cell column fits (docs/RESPONSIVE_LAYOUT.md rule 4): it is never
  // scrolled or shrunk. Until the box is measured (before the first layout) the matrix is assumed.
  const matrix = width === null || width >= SCRIPT_W + WITNESS_W + cellsWidth + CODE_MIN;
  if (!matrix)
    return (
      <div ref={ref} className="min-w-0">
        <GroupList
          inputs={inputs}
          outputs={outputs}
          groups={groups}
          lit={lit}
          dim={dim}
          onHover={onHover}
        />
      </div>
    );
  return (
    <div ref={ref} className="min-w-0">
      <div role="table" aria-label="Script groups" className="text-small">
        <div
          role="row"
          className="border-base-border-subtle text-text-dim text-meta grid items-end border-b pb-1.5"
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
                  <span title={c.top} className="text-text-ghost text-meta max-w-[68px] truncate">
                    {c.top}
                  </span>
                  {c.href ? (
                    <Link href={c.href} title={c.title} className="hover:text-jade">
                      {c.label}
                    </Link>
                  ) : (
                    <span title={c.title}>{c.label}</span>
                  )}
                  <span className="text-text-dim text-meta">{c.cap ?? ''}</span>
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
                  <span className="text-text-dim text-meta">{g.kind}</span>
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
                  {cyclesLine(g, 'runs here · cycles not available')}
                </span>
              </span>
              <span role="cell" className="min-w-0 pr-3">
                {g.witness === null ? (
                  <span className="text-text-dim">—</span>
                ) : (
                  <button
                    type="button"
                    className="hover:text-jade text-left"
                    onClick={g.witness.onSelect}
                  >
                    #{g.witness.index}
                    {g.witness.detail && (
                      <span className="text-text-dim">
                        {' '}
                        · {g.witness.detail.kind}, {formatCount(g.witness.detail.size, 'B', 'B')}
                      </span>
                    )}
                  </button>
                )}
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
                {g.code ?? <span className="text-text-dim">—</span>}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * The narrow form (board T2-script-groups.dc.html): one entry per group — name, kind and cycles,
 * then one sentence for what the matrix draws as dots and lines ("runs in input 0, 1, 2 · reads
 * witness #0"), then the args and code dep the matrix also shows, quieter. A group that did not
 * run stays, dimmer, saying which outputs its lock guards. Each entry keeps the matrix's ● / ○ so
 * the section's legend still reads.
 */
function GroupList({
  inputs,
  outputs,
  groups,
  lit,
  dim,
  onHover,
}: {
  inputs: readonly MatrixColumn[];
  outputs: readonly MatrixColumn[];
  groups: readonly MatrixGroup[];
  lit: TxLinkSets | null;
  dim: (on: boolean) => boolean;
  onHover: (hover: TxHover | null) => void;
}) {
  return (
    <ul aria-label="Script groups" className="text-small">
      {groups.map((g, i) => {
        const on = lit?.groups.has(g.id) ?? false;
        return (
          <li
            key={g.id}
            tabIndex={0}
            data-group-row={g.id}
            data-ran={g.ran}
            data-lit={on}
            data-dimmed={dim(on)}
            onMouseEnter={() => onHover({ t: 'group', id: g.id })}
            onMouseLeave={() => onHover(null)}
            onFocus={() => onHover({ t: 'group', id: g.id })}
            onBlur={() => onHover(null)}
            className={cn(
              'flex flex-col gap-1 border-t py-2.5 transition-opacity',
              i === 0 ? 'border-base-border-subtle' : 'border-base-elevated',
              g.ran ? 'text-text' : 'text-text-dim',
              on && 'bg-base-surface',
              dim(on) && 'opacity-25'
            )}
          >
            <div
              data-line="head"
              className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5"
            >
              <span className="flex min-w-0 flex-wrap items-baseline gap-x-1.5">
                {g.mark && <ClassMark kind={g.mark} size={12} className="self-center" />}
                {g.href ? (
                  <Link
                    href={g.href}
                    className={cn(
                      'hover:text-jade break-words',
                      g.ran ? 'text-text-bright' : 'text-text-dim'
                    )}
                  >
                    {g.name}
                  </Link>
                ) : (
                  <span className={cn('break-words', g.ran ? 'text-text-bright' : 'text-text-dim')}>
                    {g.name}
                  </span>
                )}
                <span className="text-text-dim whitespace-nowrap">
                  {' '}
                  · {g.kind}
                  {g.ran ? '' : ' of the outputs'}
                </span>
              </span>
              <span className="text-meta whitespace-nowrap">
                {g.ran || g.status ? cyclesLine(g, 'cycles not available') : 'not run'}
              </span>
            </div>
            <p data-line="runs" className="text-meta">
              <span
                aria-hidden="true"
                data-dot={g.ran ? 'ran' : 'attached'}
                className={cn(
                  'mr-2 inline-block rounded-full align-[1px]',
                  g.ran
                    ? 'bg-jade h-[9px] w-[9px]'
                    : 'border-text-dim h-[9px] w-[9px] border-[1.2px]'
                )}
              />
              <button
                type="button"
                data-testid={g.focusTestId}
                onClick={g.onFocus}
                className="hover:text-jade text-left"
              >
                {g.ran ? 'runs in ' : 'guards '}
                <span className={g.ran ? 'text-text-bright' : 'text-text'}>
                  {cellsInWords(g.cellIds, inputs, outputs)}
                </span>
                {g.ran ? '' : ' for the next spend'}
              </button>
              {' · '}
              {g.witness === null ? (
                'no witness'
              ) : (
                <>
                  <button
                    type="button"
                    onClick={g.witness.onSelect}
                    className="hover:text-jade text-left"
                  >
                    reads <span className="text-text-bright">witness #{g.witness.index}</span>
                  </button>
                  {g.witness.detail && (
                    <span className="text-text-dim">
                      {' '}
                      ({g.witness.detail.kind} {formatCount(g.witness.detail.size, 'B', 'B')})
                    </span>
                  )}
                </>
              )}
            </p>
            <div
              data-line="evidence"
              className="text-text-dim text-meta flex flex-wrap gap-x-3 gap-y-0.5"
            >
              <span className="min-w-0 [overflow-wrap:anywhere]">{g.sub}</span>
              {g.code !== null && (
                <span className="min-w-0 [overflow-wrap:anywhere]">{g.code}</span>
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
