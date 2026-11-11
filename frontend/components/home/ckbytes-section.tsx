'use client';

import {
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import {
  ChartFigure,
  SignedBarChart,
  type ReadoutValue,
  type SignedPoint,
} from '@/components/home/mini-chart';
import {
  SectionHeading,
  SECTION_HEAD_CLASS,
  SECTION_HINT_CLASS,
  SMALL_MULTIPLES_CLASS,
  bodyOf,
  derive,
  viewOf,
  type Derived,
} from '@/components/home/section';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { usePhosphor } from '@/hooks/usePhosphor';
import { api, type ChartResponse, type CkbytesAllocation, type DaoStatistics } from '@/lib/api';
import { formatCompactSigned, MINUS } from '@/lib/chart-ticks';
import {
  formatDecimalBytes,
  formatPercent,
  formatSignedCkbExact,
  formatSignedCkbInteger,
  formatSignedCount,
  parseCkbDecimal,
  parseShannons,
  SHANNONS_PER_CKB,
  shannonsToCkbNumber,
} from '@/lib/ckb-decimal';
import { dailyDeltas, dayLabel } from '@/lib/daily-series';
import { cn, formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { BlockLink } from '@/components/ui/block-link';
import { Comment } from '@/components/ui/comment';

/** Both change charts show the last 30 complete days. */
const CHANGE_DAYS = 30;

/**
 * The supply: CKBytes in circulation, the exact total, where it sits, and how knowledge and the DAO
 * move. Rubbing × Song print, phase 5 (H3, the Main board): the heading is `■ supply` with the
 * block the allocation was read at as its note; from `lg` the body is the board's 7 / 3 grid — the
 * figure, the bar, the legend and the change figures at the left, the comment and the formula as
 * an aside at the right; below `lg` the aside follows the legend, as the comment did.
 */
export function CkbytesSection() {
  // The number, the bar, the comment and the heading's block come from one response, so they
  // describe one block.
  const allocation = useQuery({
    queryKey: ['ckbytes-allocation'],
    queryFn: () => api.getCkbytesAllocation(),
    staleTime: 0,
    refetchInterval: 10000,
  });
  const d = useMemo(
    () =>
      derive({ data: allocation.data, error: allocation.error }, allocationOf, 'No allocation yet'),
    [allocation.data, allocation.error]
  );
  // No note while the allocation loads or fails: the heading never names a block it has not read.
  const view = viewOf(d);
  return (
    <section aria-labelledby="home-ckbytes-heading" className="flex flex-col gap-[18px]">
      {/* C10: one "tap or hover" for the two daily-change figures, by the heading. */}
      <div className={SECTION_HEAD_CLASS}>
        {/* grows, so the note stands at the row's right end */}
        <div className="min-w-0 flex-1">
          <SectionHeading
            id="home-ckbytes-heading"
            command={
              <Link href="/charts/total-supply" className="hover:text-jade transition-colors">
                supply
              </Link>
            }
            note={
              view === null ? undefined : (
                <span data-testid="ckbytes-block" className="tabular-nums">
                  at <BlockLink number={view.blockNumber} />
                </span>
              )
            }
          />
        </div>
        <TapOrHoverHint className={SECTION_HINT_CLASS} />
      </div>
      <div
        data-testid="ckbytes-body"
        className="grid grid-cols-1 gap-y-3.5 lg:grid-cols-[minmax(0,7fr)_minmax(0,3fr)] lg:gap-x-12"
      >
        <Allocation d={d} />
        {view !== null && <SupplyNote view={view} />}
        <ChangeFigures />
      </div>
    </section>
  );
}

/**
 * The two daily-change figures and the DAO block, under the allocation: in the left column from
 * `lg`, the section's last row below it (32 px under the allocation or its note, as before).
 */
function ChangeFigures() {
  return (
    <div className="mt-[18px] grid grid-cols-[repeat(auto-fit,minmax(min(100%,520px),1fr))] gap-x-10 gap-y-8 lg:col-start-1 lg:row-start-2">
      <div className={SMALL_MULTIPLES_CLASS}>
        <ChangeFigure
          testId="home-knowledge-change"
          title="Knowledge size"
          href="/charts/knowledge-size"
          what="knowledge size"
          queryKey="knowledge-size-chart"
          queryFn={() => api.getKnowledgeSizeChart()}
        />
        <ChangeFigure
          testId="home-dao-change"
          title="DAO deposits"
          href="/charts/total-deposit"
          what="Nervos DAO deposits"
          queryKey="dao-total-deposit-chart"
          queryFn={() => api.getDaoTotalDepositChart()}
        />
      </div>
      <DaoBlock />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Allocation: circulating = knowledge + DAO free + free, as the API serves it
// ---------------------------------------------------------------------------

interface AllocationView {
  blockNumber: number;
  circulating: bigint;
  knowledge: bigint;
  daoFree: bigint;
  free: bigint;
  /** The DAO cells' whole capacity: context, not a part of the split. */
  daoCapacity: bigint;
}

/**
 * The served allocation in exact shannons. The three parts must partition the circulation exactly
 * (the endpoint's contract) and none may be negative; anything else is an error, never a bar.
 * Nothing is derived here: `free` is the served part, not a remainder.
 */
export function allocationOf(a: CkbytesAllocation): AllocationView {
  const v = {
    blockNumber: a.blockNumber,
    circulating: parseShannons(a.circulating, 'circulating'),
    knowledge: parseShannons(a.knowledge, 'knowledge'),
    daoFree: parseShannons(a.daoFree, 'daoFree'),
    free: parseShannons(a.free, 'free'),
    daoCapacity: parseShannons(a.daoCapacity, 'daoCapacity'),
  };
  const g = (n: bigint) => n.toLocaleString('en-US');
  const at = `The allocation at ${formatBlockNumber(v.blockNumber)}`;
  const negative = (['circulating', 'knowledge', 'daoFree', 'free', 'daoCapacity'] as const).find(
    (k) => v[k] < BigInt(0)
  );
  if (negative) throw new Error(`${at} has a negative part: ${negative} ${v[negative]} shannons.`);
  if (v.circulating === BigInt(0)) throw new Error(`${at} has no circulation.`);
  if (v.knowledge + v.daoFree + v.free !== v.circulating) {
    throw new Error(
      `${at} does not partition circulation: ${g(v.circulating)} ≠ knowledge ${g(v.knowledge)} + DAO free ${g(v.daoFree)} + free ${g(v.free)} shannons.`
    );
  }
  return v;
}

/**
 * The supply comment (the Main board's note, in the voice): what the three parts are, then what
 * the chain holds at the block the allocation was read at. The last sentence is true by
 * construction: `allocationOf` has just checked, to the shannon, that the parts add up.
 */
export function supplyComment(view: AllocationView): string[] {
  return [
    'common knowledge is what live cells occupy.',
    'the rest is free, or in the dao.',
    `at ${formatBlockNumber(view.blockNumber)} they add up to every ckbyte in circulation.`,
  ];
}

/**
 * How the circulation is read: protocol circulation is the DAO header's C (total issuance) less
 * the genesis burn and S (the unissued secondary pool), at the block the allocation was read at.
 */
export function supplyFormula(view: AllocationView): string {
  return `C ${MINUS} burnt ${MINUS} S · from the DAO header of ${formatBlockNumber(view.blockNumber)}`;
}

/**
 * The supply's note (the Main board's aside): the comment, then the formula line in the ghost ink,
 * set in under the comment's words. Rendered only for a ready allocation.
 */
function SupplyNote({ view }: { view: AllocationView }) {
  return (
    <aside
      aria-label="Note"
      data-testid="ckbytes-note"
      className="lg:col-start-2 lg:row-span-2 lg:row-start-1 lg:self-start lg:pt-1"
    >
      <Comment lines={supplyComment(view)} className="max-w-[60ch]" />
      <p
        data-testid="ckbytes-formula"
        className="text-text-ghost text-meta m-0 mt-3 pl-[18px] font-mono"
      >
        {supplyFormula(view)}
      </p>
    </aside>
  );
}

/** Free CKB is plain CKB held: the neutral slate of a CKB mark. */
const FREE_COLOR = CLASS_COLORS.ckb;

/** The allocation's figure, bar and legend: the left column from `lg`. */
function Allocation({ d }: { d: Derived<AllocationView> }) {
  if (d.state === 'loading') {
    return (
      <div aria-hidden="true" className={cn(ALLOCATION_CLASS, 'flex flex-col gap-3.5')}>
        <div className="bg-base-surface h-9 w-2/3 animate-pulse rounded" />
        <div className="bg-base-surface h-2.5 animate-pulse rounded-full" />
      </div>
    );
  }
  if (d.state === 'error' || d.state === 'empty') {
    return (
      <div
        role="alert"
        data-testid="ckbytes-allocation-error"
        className={cn(
          ALLOCATION_CLASS,
          'border-negative/40 text-negative text-small rounded border px-3 py-2'
        )}
      >
        {d.message}
      </div>
    );
  }
  return <AllocationReady view={d.view} />;
}

/** The allocation's place in the section's grid: the first row of the left column from `lg`. */
const ALLOCATION_CLASS = 'lg:col-start-1 lg:row-start-1';

type Part = 'knowledge' | 'free' | 'dao';

/**
 * Which part of the allocation is highlighted: the hovered or focused one, else the one a tap
 * pinned. A mouse hovers and never pins; a touch tap toggles the pin; focus works like hover.
 */
function useHighlight() {
  const [hovered, setHovered] = useState<Part | null>(null);
  const [pinned, setPinned] = useState<Part | null>(null);
  const touch = useRef(false);
  const handlers = (part: Part) => ({
    onPointerDown: (e: ReactPointerEvent) => {
      touch.current = e.pointerType === 'touch';
    },
    onPointerEnter: (e: ReactPointerEvent) => {
      if (e.pointerType !== 'touch') setHovered(part);
    },
    onPointerLeave: (e: ReactPointerEvent) => {
      if (e.pointerType !== 'touch') setHovered(null);
    },
    onClick: () => {
      if (!touch.current) return;
      touch.current = false;
      setPinned((p) => (p === part ? null : part));
    },
  });
  const focusHandlers = (part: Part) => ({
    onFocus: () => setHovered(part),
    onBlur: () => setHovered(null),
  });
  return { active: hovered ?? pinned, handlers, focusHandlers };
}

function AllocationReady({ view }: { view: AllocationView }) {
  const { circulating, knowledge, free, daoFree, daoCapacity } = view;
  const { active, handlers, focusHandlers } = useHighlight();
  // The page's lit figure flares when the served total changes (material pass v2, move 4).
  const flare = usePhosphor(circulating);
  const pct = (part: bigint) => formatPercent(part, circulating, 2);
  // Geometry follows the unrounded served amounts; Number is used only for CSS layout.
  const width = (part: bigint) => `${(Number(part) / Number(circulating)) * 100}%`;
  const total = formatCkbAmount(circulating);
  const segment = (part: Part) => ({
    'data-segment': part,
    'data-active': active === part || undefined,
    ...handlers(part),
  });
  const dim = (part: Part) => active !== null && active !== part && 'opacity-35';
  const legend = (part: Part) => ({
    active: active === part,
    ...handlers(part),
    ...focusHandlers(part),
  });
  return (
    <div className={cn(ALLOCATION_CLASS, 'flex flex-col gap-3.5')}>
      <div
        data-testid="ckbytes-circulating"
        // 300 from 640 up (.sm\:text-display, globals.css move 3); the halo is move 1b's
        className={cn(
          'text-text-bright text-index sm:text-display tabular-nums tracking-[-0.01em] [overflow-wrap:anywhere]',
          flare
        )}
      >
        {total.integer}
        <span className="text-text-dim">.{total.decimal}</span>
        <span className="text-text-dim text-[0.5em]"> CKB</span>
        {/* what the figure is, now the heading says `supply` (the board's label); the block it
            is read at is the heading's note, said once */}
        <span
          data-testid="ckbytes-label"
          className="text-text-dim text-small ml-3 align-middle font-normal"
        >
          in circulation
        </span>
      </div>
      <div
        role="img"
        aria-label={`Knowledge ${pct(knowledge)} percent, free ${pct(free)} percent, Nervos DAO ${pct(daoFree)} percent`}
        className="bg-base-surface flex h-2.5 overflow-hidden rounded-full"
      >
        {knowledge > BigInt(0) && (
          <span
            {...segment('knowledge')}
            className={cn(SEGMENT, 'bg-jade shrink-0', dim('knowledge'))}
            style={{ width: width(knowledge) }}
          />
        )}
        {free > BigInt(0) && (
          <span
            {...segment('free')}
            className={cn(SEGMENT, 'shrink-0', dim('free'))}
            style={{ width: width(free), backgroundColor: FREE_COLOR }}
          />
        )}
        {daoFree > BigInt(0) && (
          <span
            {...segment('dao')}
            className={cn(SEGMENT, 'bg-gold shrink-0', dim('dao'))}
            style={{ width: width(daoFree) }}
          />
        )}
      </div>
      <div className="text-small flex flex-wrap gap-x-8 gap-y-1">
        <LegendItem
          swatch="bg-jade"
          name="Knowledge"
          value={knowledge}
          note={`${formatDecimalBytes(knowledge / SHANNONS_PER_CKB)} · ${pct(knowledge)}%`}
          {...legend('knowledge')}
        />
        <LegendItem
          swatchColor={FREE_COLOR}
          name="Free"
          value={free}
          note={`${pct(free)}%`}
          {...legend('free')}
        />
        <LegendItem
          swatch="bg-gold"
          name="Nervos DAO"
          value={daoFree}
          note={`${pct(daoFree)}% · ${formatCkbCompact(daoCapacity).value} locked`}
          {...legend('dao')}
        />
      </div>
    </div>
  );
}

const SEGMENT = 'transition-opacity duration-150';

interface LegendItemProps {
  /** The swatch's theme class (`bg-jade`), or … */
  swatch?: string;
  /** … its colour, for an ink with no theme class (free CKB's slate). */
  swatchColor?: string;
  name: string;
  value: bigint;
  note: string;
  /** Highlighted: the name and amount brighten and the amount reads out exactly. */
  active: boolean;
  onPointerDown: (e: ReactPointerEvent) => void;
  onPointerEnter: (e: ReactPointerEvent) => void;
  onPointerLeave: (e: ReactPointerEvent) => void;
  onClick: () => void;
  onFocus: () => void;
  onBlur: () => void;
}

/** One legend item: a button, so it is focusable; hovered, focused or tapped, it highlights. */
function LegendItem({
  swatch,
  swatchColor,
  name,
  value,
  note,
  active,
  ...events
}: LegendItemProps) {
  const compact = formatCkbCompact(value);
  return (
    <button
      type="button"
      data-legend={name}
      data-active={active || undefined}
      className="focus-visible:ring-base-border -mx-1 flex items-baseline gap-2 rounded bg-transparent px-1 py-1 text-left outline-none focus-visible:ring-1"
      {...events}
    >
      <span
        aria-hidden="true"
        className={cn('h-2 w-2 self-center rounded-sm', swatch)}
        style={swatchColor === undefined ? undefined : { backgroundColor: swatchColor }}
      />
      <span className={active ? 'text-text-bright' : 'text-text'}>{name}</span>
      <span className="text-text-bright tabular-nums">
        {active ? formatCkbAmount(value).full : compact.value} CKB
      </span>
      <span className={cn('tabular-nums', active ? 'text-text' : 'text-text-dim')}>{note}</span>
    </button>
  );
}

// ---------------------------------------------------------------------------
// Daily change charts
// ---------------------------------------------------------------------------

interface ChangeView {
  points: SignedPoint[];
  first: string;
  last: { label: string; delta: bigint };
  min: number;
  max: number;
}

/** An exact signed change for the caption: whole CKB bright, the 8 decimals dim. */
function exactSignedReadout(delta: bigint): ReadoutValue {
  const text = formatSignedCkbExact(delta);
  const dot = text.indexOf('.');
  return {
    value: (
      <>
        {text.slice(0, dot)}
        <span className="text-text-dim text-meta">{text.slice(dot)}</span>
      </>
    ),
    text: `${text} CKB`,
  };
}

/** The last 30 days' changes of a cumulative CKB chart, exact; geometry in float CKB. */
export function changeView(chart: ChartResponse): ChangeView | null {
  const deltas = dailyDeltas(chart.data, CHANGE_DAYS);
  if (deltas.length === 0) return null;
  const points = deltas.map((d) => ({
    label: dayLabel(d.date),
    value: shannonsToCkbNumber(d.delta),
    readout: exactSignedReadout(d.delta),
  }));
  const values = points.map((p) => p.value);
  const last = deltas[deltas.length - 1];
  return {
    points,
    first: points[0].label,
    last: { label: dayLabel(last.date), delta: last.delta },
    min: Math.min(...values),
    max: Math.max(...values),
  };
}

function ChangeFigure({
  testId,
  title,
  href,
  what,
  queryKey,
  queryFn,
}: {
  testId: string;
  title: string;
  href: string;
  what: string;
  queryKey: string;
  queryFn: () => Promise<ChartResponse>;
}) {
  const { data, error } = useQuery({
    queryKey: [queryKey],
    queryFn,
    staleTime: 300_000,
    refetchInterval: 300_000,
  });
  const d = useMemo(
    () => derive({ data, error }, changeView, 'No daily history yet'),
    [data, error]
  );
  const v = viewOf(d);
  return (
    <ChartFigure
      hint={false}
      testId={testId}
      title={title}
      href={href}
      subtitle="daily change, CKB"
      headline={v ? formatSignedCkbInteger(v.last.delta) : undefined}
      body={bodyOf(d, (view) => (
        <SignedBarChart
          points={view.points}
          ariaLabel={`Daily change of ${what}, ${view.first} to ${view.last.label}, between ${formatCompactSigned(view.min)} and ${formatCompactSigned(view.max)} CKB a day, ${formatSignedCkbInteger(view.last.delta)} CKB on ${view.last.label}`}
        />
      ))}
    />
  );
}

// ---------------------------------------------------------------------------
// Nervos DAO block
// ---------------------------------------------------------------------------

interface DaoView {
  deposited: bigint;
  apc: string;
  depositors: number;
  depositorsChange: number | null;
  activeDeposits: number;
  change: bigint | null;
}

/** The DAO block's figures; the 24 h changes are null until chain history spans 24 hours. */
export function daoView(stats: DaoStatistics): DaoView {
  return {
    deposited: parseShannons(stats.totalDeposited, 'totalDeposited'),
    apc: stats.estimatedApc,
    depositors: stats.totalDepositors,
    depositorsChange: stats.depositorsChange24h ?? null,
    activeDeposits: stats.activeDeposits,
    change: stats.depositChange24h === null ? null : parseCkbDecimal(stats.depositChange24h),
  };
}

function DaoStat({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-text-dim text-small">{term}</dt>
      <dd className="text-text-bright text-body m-0 tabular-nums">{children}</dd>
    </div>
  );
}

function DaoBlock() {
  const { data, error } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });
  const d = useMemo(() => derive({ data, error }, daoView, 'No DAO data yet'), [data, error]);
  return (
    <div data-testid="home-dao" className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-col gap-0.5">
        <span className="text-text-bright text-body">Nervos DAO</span>
        <span className="text-text-dim text-small">deposited</span>
      </div>
      {d.state === 'loading' && (
        <div aria-hidden="true" className="bg-base-surface h-24 animate-pulse rounded" />
      )}
      {d.state === 'error' && (
        <div role="alert" className="text-rouge text-small">
          {d.message}
        </div>
      )}
      {d.state === 'empty' && <div className="text-text-dim text-small">{d.message}</div>}
      {d.state === 'ready' && <DaoFigures view={d.view} />}
      <Link
        href="/dao"
        className="text-interactive hover:text-jade text-small w-fit transition-colors"
      >
        DAO details →
      </Link>
    </div>
  );
}

function DaoFigures({ view }: { view: DaoView }) {
  const deposited = formatCkbAmount(view.deposited);
  const flare = usePhosphor(view.deposited);
  const change = view.change;
  const changeTone =
    change === null || change === BigInt(0)
      ? 'text-text-bright'
      : change > BigInt(0)
        ? 'text-positive'
        : 'text-negative';
  return (
    <>
      <div
        data-testid="home-dao-deposited"
        className={cn(
          'text-text-bright text-hero font-medium tabular-nums [overflow-wrap:anywhere]',
          flare
        )}
      >
        {deposited.integer}
        <span className="text-text-dim">.{deposited.decimal}</span>
        <span className="text-text-dim text-small"> CKB</span>
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-10 gap-y-3.5">
        <DaoStat term="Estimated APC">{view.apc}%</DaoStat>
        <DaoStat term="Depositors">
          {view.depositors.toLocaleString('en-US')}
          {view.depositorsChange !== null && (
            <span className="text-text-dim text-small">
              {` ${formatSignedCount(view.depositorsChange)} in 24 h`}
            </span>
          )}
        </DaoStat>
        <DaoStat term="Active deposits">{view.activeDeposits.toLocaleString('en-US')}</DaoStat>
        <DaoStat term="Change in 24 h">
          {change === null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <span
              data-testid="home-dao-change-24h"
              className={changeTone}
              title={`${formatSignedCkbExact(change)} CKB`}
            >
              {formatSignedCkbInteger(change)} CKB
            </span>
          )}
        </DaoStat>
      </dl>
    </>
  );
}
