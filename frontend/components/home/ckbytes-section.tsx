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
  SECTION_HEADING_CLASS,
  SMALL_MULTIPLES_CLASS,
  bodyOf,
  derive,
  viewOf,
} from '@/components/home/section';
import { api, type ChartResponse, type DaoStatistics, type NetworkStats } from '@/lib/api';
import { formatCompactSigned } from '@/lib/chart-ticks';
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

/** Both change charts show the last 30 complete days. */
const CHANGE_DAYS = 30;

/** CKBytes in circulation: the exact total, where it sits, and how knowledge and the DAO move. */
export function CkbytesSection() {
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });
  return (
    <section aria-labelledby="home-ckbytes-heading" className="flex flex-col gap-[18px]">
      <h2 id="home-ckbytes-heading" className={SECTION_HEADING_CLASS}>
        <Link href="/charts/total-supply" className="hover:text-jade transition-colors">
          CKBytes in circulation
        </Link>
      </h2>
      <Allocation stats={stats} />
      <div className="mt-3.5 grid grid-cols-[repeat(auto-fit,minmax(min(100%,520px),1fr))] gap-x-10 gap-y-8">
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
    </section>
  );
}

// ---------------------------------------------------------------------------
// Allocation: circulating = knowledge + free + Nervos DAO
// ---------------------------------------------------------------------------

interface AllocationView {
  circulating: bigint;
  knowledge: bigint;
  free: bigint;
  dao: bigint;
}

type AllocationState =
  | { state: 'loading' }
  | { state: 'unavailable' }
  | { state: 'error'; message: string }
  | { state: 'ready'; view: AllocationView };

/**
 * Circulating supply split into its three parts, in exact shannons. Free is the remainder and is
 * never clamped: knowledge + DAO above circulation means the API's figures contradict each other.
 */
export function allocationOf(stats: NetworkStats | undefined): AllocationState {
  if (!stats) return { state: 'loading' };
  if (
    stats.circulatingSupply === null ||
    stats.knowledgeSize === null ||
    stats.daoLocked === null
  ) {
    return { state: 'unavailable' };
  }
  try {
    const circulating = parseShannons(stats.circulatingSupply, 'circulatingSupply');
    const knowledge = parseShannons(stats.knowledgeSize, 'knowledgeSize');
    const dao = parseShannons(stats.daoLocked, 'daoLocked');
    if (knowledge < BigInt(0) || dao < BigInt(0) || circulating <= BigInt(0)) {
      return {
        state: 'error',
        message: `Inconsistent supply data: circulating ${circulating}, knowledge ${knowledge}, DAO ${dao} shannons.`,
      };
    }
    const free = circulating - knowledge - dao;
    if (free < BigInt(0)) {
      const f = (s: bigint) => formatCkbAmount(s).full;
      return {
        state: 'error',
        message: `Inconsistent supply data: knowledge ${f(knowledge)} CKB + DAO ${f(dao)} CKB exceed circulating ${f(circulating)} CKB.`,
      };
    }
    return { state: 'ready', view: { circulating, knowledge, free, dao } };
  } catch (e) {
    return { state: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}

const FREE_COLOR = 'bg-[#3a4258]';

function Allocation({ stats }: { stats: NetworkStats | undefined }) {
  const a = allocationOf(stats);
  if (a.state === 'loading') {
    return (
      <div aria-hidden="true" className="flex flex-col gap-3.5">
        <div className="bg-base-surface h-9 w-2/3 animate-pulse rounded" />
        <div className="bg-base-surface h-2.5 animate-pulse rounded-[5px]" />
      </div>
    );
  }
  if (a.state === 'unavailable') {
    return <p className="text-text-dim m-0 text-xs">Supply figures are not available yet.</p>;
  }
  if (a.state === 'error') {
    return (
      <div
        role="alert"
        data-testid="ckbytes-allocation-error"
        className="border-negative/40 text-negative rounded border px-3 py-2 text-xs"
      >
        {a.message}
      </div>
    );
  }
  return <AllocationReady view={a.view} />;
}

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
  const { circulating, knowledge, free, dao } = view;
  const { active, handlers, focusHandlers } = useHighlight();
  const pct = (part: bigint) => formatPercent(part, circulating, 2);
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
    <div className="flex flex-col gap-3.5">
      <div
        data-testid="ckbytes-circulating"
        className="text-text-bright text-[clamp(22px,3.2vw,34px)] font-medium tabular-nums tracking-[-0.01em] [overflow-wrap:anywhere]"
      >
        {total.integer}
        <span className="text-text-dim">.{total.decimal}</span>
        <span className="text-text-dim text-[0.5em]"> CKB</span>
      </div>
      <div
        role="img"
        aria-label={`Knowledge ${pct(knowledge)} percent, free ${pct(free)} percent, Nervos DAO ${pct(dao)} percent`}
        className="bg-base-surface flex h-2.5 gap-0.5 overflow-hidden rounded-[5px]"
      >
        <span
          {...segment('knowledge')}
          className={cn(SEGMENT, 'bg-jade min-w-[3px] shrink-0', dim('knowledge'))}
          style={{ width: `${pct(knowledge)}%` }}
        />
        <span
          {...segment('free')}
          className={cn(SEGMENT, FREE_COLOR, dim('free'))}
          style={{ width: `${pct(free)}%` }}
        />
        <span {...segment('dao')} className={cn(SEGMENT, 'bg-gold flex-1', dim('dao'))} />
      </div>
      <div className="flex flex-wrap gap-x-8 gap-y-1 text-xs">
        <LegendItem
          swatch="bg-jade"
          name="Knowledge"
          value={knowledge}
          note={`${formatDecimalBytes(knowledge / SHANNONS_PER_CKB)} · ${pct(knowledge)}%`}
          {...legend('knowledge')}
        />
        <LegendItem
          swatch={FREE_COLOR}
          name="Free"
          value={free}
          note={`${pct(free)}%`}
          {...legend('free')}
        />
        <LegendItem
          swatch="bg-gold"
          name="Nervos DAO"
          value={dao}
          note={`${pct(dao)}%`}
          {...legend('dao')}
        />
      </div>
    </div>
  );
}

const SEGMENT = 'transition-opacity duration-150';

interface LegendItemProps {
  swatch: string;
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
function LegendItem({ swatch, name, value, note, active, ...events }: LegendItemProps) {
  const compact = formatCkbCompact(value);
  return (
    <button
      type="button"
      data-legend={name}
      data-active={active || undefined}
      className="focus-visible:ring-base-border -mx-1 flex items-baseline gap-2 rounded bg-transparent px-1 py-1 text-left outline-none focus-visible:ring-1"
      {...events}
    >
      <span aria-hidden="true" className={cn('h-2 w-2 self-center rounded-sm', swatch)} />
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
        <span className="text-text-dim text-xs">{text.slice(dot)}</span>
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

/** The DAO block's figures; the 24 h changes are absent until a previous day's snapshot exists. */
export function daoView(stats: DaoStatistics): DaoView {
  return {
    deposited: parseShannons(stats.totalDeposited, 'totalDeposited'),
    apc: stats.estimatedApc,
    depositors: stats.totalDepositors,
    depositorsChange: stats.depositorsChange24h ?? null,
    activeDeposits: stats.activeDeposits,
    change: stats.depositChange24h === undefined ? null : parseCkbDecimal(stats.depositChange24h),
  };
}

function DaoStat({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-text-dim text-xs">{term}</dt>
      <dd className="text-text-bright m-0 text-[15px] tabular-nums">{children}</dd>
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
        <span className="text-text-bright text-[13px]">Nervos DAO</span>
        <span className="text-text-dim text-xs">deposited</span>
      </div>
      {d.state === 'loading' && (
        <div aria-hidden="true" className="bg-base-surface h-24 animate-pulse rounded" />
      )}
      {d.state === 'error' && (
        <div role="alert" className="text-rouge text-xs">
          {d.message}
        </div>
      )}
      {d.state === 'empty' && <div className="text-text-dim text-xs">{d.message}</div>}
      {d.state === 'ready' && <DaoFigures view={d.view} />}
      <Link
        href="/dao"
        className="text-interactive hover:text-jade w-fit text-xs transition-colors"
      >
        DAO details →
      </Link>
    </div>
  );
}

function DaoFigures({ view }: { view: DaoView }) {
  const deposited = formatCkbAmount(view.deposited);
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
        className="text-text-bright text-[22px] font-medium tabular-nums [overflow-wrap:anywhere]"
      >
        {deposited.integer}
        <span className="text-text-dim">.{deposited.decimal}</span>
        <span className="text-text-dim text-xs"> CKB</span>
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-10 gap-y-3.5">
        <DaoStat term="Estimated APC">{view.apc}%</DaoStat>
        <DaoStat term="Depositors">
          {view.depositors.toLocaleString('en-US')}
          {view.depositorsChange !== null && (
            <span className="text-text-dim text-xs">
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
