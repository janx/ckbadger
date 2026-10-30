'use client';

import { useState } from 'react';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import type { ChartResponse, DaoEvidence, DaoStatistics } from '@/lib/api';
import { formatPercent, parseCkbDecimal, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { DaoSources } from '@/components/dao/dao-sources';
import { DaoHistory } from '@/components/dao/dao-history';
import { DaoGrows } from '@/components/dao/dao-grows';
import { formatAccumulatedRate } from '@/lib/dao-header';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatCapacity, formatDurationMs, formatUtc8DateTime, truncateHash } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { IciclePartition, type IcicleNode } from '@/components/ui/icicle-partition';
import { useElementWidth } from '@/hooks/useElementWidth';
import { DAO_ISSUANCE_COLORS } from '@/lib/design-tokens';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

/** R8: every chart response carries the FIRST point holding its greatest value (null when empty). */
export interface ChartPeakPoint {
  date: string;
  value: string;
}
/** `/dao/charts/total-deposit`; `peak` is optional in the type only until `ChartResponse` names it. */
export type DaoDepositHistory = ChartResponse & { peak?: ChartPeakPoint | null };

interface Props {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
  depositedHistory?: DaoDepositHistory;
  circulationHistory?: ChartResponse;
  historyError?: boolean;
  /** The page's title and summary, set at the top of the hero's left column (board grid). */
  intro?: ReactNode;
}

function Amount({ value, signed = false }: { value: string; signed?: boolean }) {
  const amount = parseShannons(value, 'DAO amount');
  const parts = splitCkbAmount(amount);
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {signed ? parts.sign : null}
      {parts.integer}
      <span className="text-text-dim">{parts.fraction}</span>
      <span className="text-text-dim/50">{parts.ghost}</span>
    </span>
  );
}

/** Chart / summary values arrive as CKB decimal strings, not shannons. */
function CkbDecimal({ value, signed = false }: { value: string; signed?: boolean }) {
  const parts = splitCkbAmount(parseCkbDecimal(value));
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {signed ? parts.sign : null}
      {parts.integer}
      <span className="text-text-dim">{parts.fraction}</span>
      <span className="text-text-dim/50">{parts.ghost}</span>
    </span>
  );
}

/** Header AR integers (×10¹⁶) as the exact rate, the same form the block page decodes. */
function formatAr(raw: string): string {
  if (!/^\d+$/.test(raw)) throw new Error(`DAO AR "${raw}" is not an unsigned integer`);
  return formatAccumulatedRate(BigInt(raw));
}

function Section({
  title,
  note,
  children,
}: {
  title: string;
  note?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0" aria-label={title}>
      <div className="mb-3.5 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-[15px] font-medium">{title}</h2>
        {note && <span className="text-text-dim text-[12px]">{note}</span>}
      </div>
      {children}
    </section>
  );
}

function Metric({
  label,
  children,
  detail,
  className,
}: {
  label: string;
  children: ReactNode;
  detail?: ReactNode;
  className?: string;
}) {
  return (
    <div className={`min-w-0 ${className ?? ''}`}>
      <dt className="text-text-dim text-xs">{label}</dt>
      <dd className="text-text-bright mt-1 text-[18px] leading-tight">{children}</dd>
      {detail && <dd className="text-text-dim mt-1 text-[11.5px]">{detail}</dd>}
    </div>
  );
}

/**
 * The hero (board V63): deposited, its share of circulation on the last complete day and its peak,
 * then the metrics. Deposited, deposits and depositors are the evidence anchor's phase-0 facts;
 * Estimated APC and the 24 h change are the statistics anchor's.
 */
function Hero({
  statistics,
  evidence,
  deposited,
  ratio,
  intro,
}: {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
  deposited?: DaoDepositHistory;
  ratio?: ChartResponse;
  intro?: ReactNode;
}) {
  if (deposited && deposited.peak === undefined)
    throw new Error('/dao/charts/total-deposit has no peak (R8 chart contract)');
  const share = ratio?.data.at(-1);
  const peak = deposited?.peak ?? null;
  if ((statistics.deposits24h === null) !== (statistics.withdrawalRequests24h === null))
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: 24 h deposits ${statistics.deposits24h} without its withdrawal-request count`
    );
  const change = statistics.depositChange24h;
  const changeShannons = change === null ? null : parseCkbDecimal(change);
  const year = evidence.ar.year365;
  return (
    <section
      aria-label="Nervos DAO summary"
      className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_460px] lg:items-end lg:gap-12"
    >
      <div className="flex min-w-0 flex-col gap-1">
        {intro && <div className="mb-5">{intro}</div>}
        <span className="text-text-dim text-xs">deposited</span>
        <span
          data-role="deposited"
          className="text-text-bright text-[24px] font-medium leading-tight sm:text-[30px]"
        >
          <Amount value={evidence.phase0.capacity} />{' '}
          <span className="text-text-dim text-[14px]">CKB</span>
        </span>
        {share && deposited && (
          <span className="text-text-dim text-[13px]">
            <span className="text-gold">{ratioText(share.value)}%</span> of circulating CKB on{' '}
            {share.date}
            {peak && (
              <>
                {' · '}peak{' '}
                <span className="text-text">
                  <CkbDecimal value={peak.value} />
                </span>{' '}
                CKB on {peak.date}
              </>
            )}
          </span>
        )}
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-7 gap-y-[18px]">
        <Metric label="Estimated APC" detail="at today's issuance and deposits">
          {statistics.estimatedApc}%
        </Metric>
        <Metric
          label="Compensation, last 365 days"
          detail={
            year ? "on free capacity, from the header's AR" : 'the chain is younger than 365 days'
          }
        >
          {year ? `${year.observedGrowthPercent}%` : '—'}
        </Metric>
        <Metric label="Depositors">
          {evidence.concentration.currentDepositors.toLocaleString('en-US')}
        </Metric>
        <Metric
          label="Deposits"
          detail={`+ ${evidence.phase1.count.toLocaleString('en-US')} withdrawals requested`}
        >
          {evidence.phase0.count.toLocaleString('en-US')}
        </Metric>
        <Metric
          label="Change in 24 h"
          className="col-span-2"
          detail={
            statistics.deposits24h === null || statistics.withdrawalRequests24h === null
              ? undefined
              : `${plural(statistics.deposits24h, 'deposit')} in, ${plural(statistics.withdrawalRequests24h, 'withdrawal request')} out`
          }
        >
          {changeShannons === null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <span
              data-role="change24h"
              className={
                changeShannons > BigInt(0)
                  ? 'text-positive'
                  : changeShannons < BigInt(0)
                    ? 'text-negative'
                    : 'text-text'
              }
            >
              <CkbDecimal value={change!} signed />
            </span>
          )}{' '}
          <span className="text-text-dim text-[12px]">CKB</span>
        </Metric>
      </dl>
    </section>
  );
}

function History({
  deposited,
  ratio,
  error,
}: {
  deposited?: ChartResponse;
  ratio?: ChartResponse;
  error?: boolean;
}) {
  return (
    <section aria-label="History" className="min-w-0">
      {error ? (
        <>
          <h2 className="text-text-bright mb-3 text-[15px] font-medium">History</h2>
          <div role="alert" className="text-rouge text-xs">
            DAO daily history could not be loaded from indexed snapshots.
          </div>
        </>
      ) : !deposited || !ratio ? (
        <>
          <h2 className="text-text-bright mb-3 text-[15px] font-medium">History</h2>
          <div className="text-text-dim text-xs">Loading canonical daily history…</div>
        </>
      ) : (
        <DaoHistory deposited={deposited} ratio={ratio} />
      )}
    </section>
  );
}

/** An exact millisecond string from the API as a safe integer for duration display. */
function exactSafeMs(value: string, context: string): number {
  const ms = exactUnsignedInteger(value, context);
  if (ms > BigInt(Number.MAX_SAFE_INTEGER))
    throw new Error(`${context}: ${value} ms is out of range`);
  return Number(ms);
}

/** The circulation ratio exactly as the API states it (a decimal percentage). */
function ratioText(value: string): string {
  return formatExactDecimal(value, 'DAO circulation ratio');
}

/** An exact CKB amount inside SVG text: whole part, then the significant decimals dimmed. */
function SvgCkb({ shannons }: { shannons: bigint }) {
  const parts = splitCkbAmount(shannons);
  return (
    <>
      {parts.integer}
      <tspan fillOpacity={0.7}>{parts.fraction}</tspan>
    </>
  );
}

const ICICLE_LABEL_MIN_PX = 1000;
const BURNT_FOR_GOOD =
  'No cell can ever claim it. A hardfork could only put future secondary issuance to use; what is burnt stays burnt.';

function Issuance({ statistics }: Pick<Props, 'statistics'>) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const mining = parseShannons(statistics.miningReward, 'miningReward');
  const compensation = parseShannons(statistics.depositCompensation, 'depositCompensation');
  const burnt = parseShannons(statistics.burnt, 'burnt');
  const claimed = parseShannons(statistics.totalCompensationPaid, 'totalCompensationPaid');
  const unclaimed = parseShannons(statistics.unclaimedCompensation, 'unclaimedCompensation');
  if (claimed + unclaimed !== compensation)
    throw new Error('DAO compensation parts do not equal total compensation');
  const total = mining + compensation + burnt;
  const issuanceEmpty = total === BigInt(0);
  const issued = parseShannons(statistics.tipTotalIssuance, 'DAO C at statistics anchor');
  const occupied = parseShannons(
    statistics.tipProtocolOccupiedCapacity,
    'DAO U at statistics anchor'
  );
  const depositorWeight = parseShannons(
    statistics.depositorAllocationWeight,
    'depositorAllocationWeight'
  );
  const other = issued - occupied - depositorWeight;
  if (
    issued <= BigInt(0) ||
    occupied < BigInt(0) ||
    depositorWeight < BigInt(0) ||
    other < BigInt(0)
  ) {
    throw new Error(
      `invalid DAO capacity-use partition at statistics block ${statistics.tipBlockNumber}`
    );
  }
  const liveCells = (
    statistics.depositCellsCount + statistics.withdrawalRequestCellsCount
  ).toLocaleString('en-US');
  const share = (v: bigint) => `${formatPercent(v, total, 2)}%`;
  const root: IcicleNode | null = issuanceEmpty
    ? null
    : {
        key: 'total',
        label: 'Secondary issuance since genesis',
        value: total,
        tone: 'ckb',
        color: DAO_ISSUANCE_COLORS.total,
        opacity: 0.55,
        ink: 'bright',
        align: 'inside',
        note: '1.344 billion CKB a year, split in every block',
        children: [
          {
            key: 'miners',
            label: 'to miners',
            value: mining,
            tone: 'ckb',
            color: DAO_ISSUANCE_COLORS.miners,
            ink: 'bright',
            note: `${share(mining)} · s × U ÷ C`,
            placeholder: 'paid in cellbases',
          },
          {
            key: 'compensation',
            label: 'compensation for depositors',
            value: compensation,
            tone: 'dao',
            align: 'end',
            note: `${share(compensation)} · via AR`,
            children: [
              {
                key: 'paid',
                label: 'paid out',
                value: claimed,
                tone: 'dao',
                opacity: 1,
                align: 'end',
                note: 'at withdrawals',
              },
              {
                key: 'owed',
                label: 'still owed',
                value: unclaimed,
                tone: 'dao',
                opacity: 0.4,
                note: `to the ${liveCells} live deposit cells`,
              },
            ],
          },
          {
            key: 'burnt',
            label: 'burnt',
            value: burnt,
            tone: 'other',
            color: DAO_ISSUANCE_COLORS.burnt,
            opacity: 1,
            ink: 'bright',
            align: 'inside',
            note: `${share(burnt)} · gone for good`,
            placeholder: BURNT_FOR_GOOD,
          },
        ],
      };
  const narrow = width !== null && width < ICICLE_LABEL_MIN_PX;
  const use = [
    { key: 'occupied', title: 'Occupied', value: occupied, color: DAO_ISSUANCE_COLORS.miners },
    {
      key: 'depositors',
      title: 'DAO depositors',
      value: depositorWeight,
      color: DAO_ISSUANCE_COLORS.compensation,
    },
    { key: 'other', title: 'Everything else', value: other, color: DAO_ISSUANCE_COLORS.burnt },
  ];
  return (
    <Section
      title="Where secondary issuance goes"
      note={`since genesis · exact to the shannon at block ${formatBlockNumber(statistics.tipBlockNumber)}`}
    >
      <div ref={ref} className="min-w-0">
        {root === null ? (
          <div className="text-text-dim text-[12px]">
            No secondary issuance has accrued at this indexed block; allocation shares are
            undefined.
          </div>
        ) : (
          width !== null && (
            <IciclePartition
              root={root}
              label="Secondary issuance since genesis, split three ways: to miners, compensation for depositors, burnt; the compensation is paid out or still owed to live deposits"
              formatValue={(v) => <SvgCkb shannons={v} />}
              width={width}
              hideLabels={narrow}
            />
          )
        )}
      </div>
      {root !== null && narrow && (
        <ul
          aria-label="Secondary issuance parts"
          className="mt-3 space-y-2 text-[12px] leading-relaxed"
        >
          <li className="text-text-bright">
            Secondary issuance since genesis <Amount value={total.toString()} /> CKB ·{' '}
            <span className="text-text-dim">1.344 billion CKB a year, split in every block</span>
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.miners} /> to miners{' '}
            <Amount value={mining.toString()} /> · {share(mining)} ·{' '}
            <span className="text-text-dim">s × U ÷ C, paid in cellbases</span>
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.compensation} />{' '}
            <span className="text-gold">compensation for depositors</span>{' '}
            <Amount value={compensation.toString()} /> · {share(compensation)} ·{' '}
            <span className="text-text-dim">via AR</span>
            <ul className="mt-1 space-y-1 pl-5">
              <li>
                paid out <Amount value={claimed.toString()} />{' '}
                <span className="text-text-dim">at withdrawals</span>
              </li>
              <li>
                still owed <Amount value={unclaimed.toString()} />{' '}
                <span className="text-text-dim">to the {liveCells} live deposit cells</span>
              </li>
            </ul>
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.burnt} /> burnt <Amount value={burnt.toString()} /> ·{' '}
            {share(burnt)} · <span className="text-text-dim">gone for good. {BURNT_FOR_GOOD}</span>
          </li>
        </ul>
      )}
      <div className="mt-6 grid gap-7 md:grid-cols-[300px_minmax(0,1fr)] md:items-start">
        <p className="text-text text-[12.5px] leading-relaxed">
          Every block splits its share the same way. The proportions are simply how all{' '}
          <Amount value={issued.toString()} /> CKB issued by block{' '}
          {formatBlockNumber(statistics.tipBlockNumber)} is used right now.
        </p>
        <div className="flex min-w-0 flex-col gap-2">
          <div className="text-text-dim text-[12px]">
            Current capacity use at statistics block {formatBlockNumber(statistics.tipBlockNumber)}
          </div>
          <div
            className="flex h-2.5 w-full gap-0.5 overflow-hidden rounded"
            aria-label="Current capacity use partition"
          >
            {use.map((part) => (
              <div
                key={part.key}
                title={`${part.title}: ${formatCapacity(part.value)}`}
                style={{
                  width: `${formatPercent(part.value, issued, 4)}%`,
                  background: part.color,
                }}
              />
            ))}
          </div>
          <div className="text-text-dim flex flex-wrap gap-x-7 gap-y-1.5 text-[12px]">
            <span>
              <Swatch color={DAO_ISSUANCE_COLORS.miners} />
              occupied by cells{' '}
              <span className="text-text-bright">{formatPercent(occupied, issued, 2)}%</span> →
              miners
            </span>
            <span>
              <Swatch color={DAO_ISSUANCE_COLORS.compensation} />
              phase-0 free capacity + compensation already accrued{' '}
              <span className="text-text-bright">
                {formatPercent(depositorWeight, issued, 2)}%
              </span>{' '}
              → depositors · withdrawal requests take no share
            </span>
            <span>
              <Swatch color={DAO_ISSUANCE_COLORS.burnt} />
              everything else{' '}
              <span className="text-text-bright">{formatPercent(other, issued, 2)}%</span> → burnt
            </span>
          </div>
          <span className="text-text-dim text-[11.5px]">
            So a deposit keeps its share of all CKB through secondary issuance. Free CKB outside the
            DAO receives nothing: its share of each block&apos;s issuance is burnt.
          </span>
          <span className="text-text-dim text-[11px]">
            Protocol occupied includes the genesis virtual occupied baseline; it is not the real
            occupied size of live cells.
          </span>
          <Link
            href={`/blocks/${statistics.tipBlockNumber}`}
            className="text-text hover:text-text-bright text-[11px] hover:underline"
          >
            Verify statistics anchor block {formatBlockNumber(statistics.tipBlockNumber)} →
          </Link>
        </div>
      </div>
    </Section>
  );
}

function Swatch({ color }: { color: string }) {
  return (
    <span
      aria-hidden="true"
      className="mr-1.5 inline-block h-[9px] w-[9px] rounded-sm align-middle"
      style={{ background: color }}
    />
  );
}

function plural(count: number, word: string): string {
  return `${count.toLocaleString('en-US')} ${word}${count === 1 ? '' : 's'}`;
}

function Windows({ evidence }: { evidence: DaoEvidence }) {
  const [selectedDay, setSelectedDay] = useState<number | null>(null);
  const max = evidence.windows.reduce((largest, row) => {
    const value = parseShannons(row.capacity, 'window capacity');
    return value > largest ? value : largest;
  }, BigInt(0));
  const first = evidence.windows[0];
  if (!first || first.nominalDay !== 1)
    throw new Error(
      `DAO evidence at ${formatBlockNumber(evidence.tipBlockNumber)} has no first-day cycle window`
    );
  const ready = evidence.phase1ReadyByEpochLock;
  const waiting = evidence.phase1WaitingForEpochLock;
  const readyShannons = parseShannons(ready.capacity, 'phase-1 ready capacity');
  const waitingShannons = parseShannons(waiting.capacity, 'phase-1 waiting capacity');
  const requested = parseShannons(evidence.phase1.capacity, 'phase-1 capacity');
  if (
    readyShannons + waitingShannons !== requested ||
    ready.count + waiting.count !== evidence.phase1.count
  )
    throw new Error(
      `DAO phase-1 readiness does not partition the requests at ${formatBlockNumber(evidence.tipBlockNumber)}`
    );
  return (
    <Section title="When deposits can leave" note={`at indexed epoch ${evidence.tipEpoch.display}`}>
      <p className="text-text mb-4 text-[13px] leading-relaxed">
        A deposit runs in cycles of 180 epochs, about 30 days, counted from the epoch it was made. A
        withdrawal completes only at the end of a cycle. Each bar is the CKB whose current cycle
        ends that day: six exact epochs from the indexed tip, epoch ranges rather than predicted
        calendar dates.
      </p>
      <div
        className="grid grid-cols-[repeat(30,minmax(0,1fr))] gap-[3px]"
        role="list"
        aria-label="Phase-0 cycle boundaries"
      >
        {evidence.windows.map((row) => {
          const value = parseShannons(row.capacity, 'window capacity');
          const height = max === BigInt(0) ? 0 : Number((value * BigInt(100)) / max);
          return (
            <div
              key={row.nominalDay}
              role="listitem"
              tabIndex={0}
              aria-label={`Nominal day ${row.nominalDay}, epochs +${row.fromEpochOffset} to +${row.throughEpochOffset}: ${row.count} deposits, ${row.capacity} shannons`}
              onFocus={() => setSelectedDay(row.nominalDay)}
              onMouseEnter={() => setSelectedDay(row.nominalDay)}
              className="focus:outline-interactive group flex h-28 flex-col justify-end focus:outline"
              title={`Nominal day ${row.nominalDay}, epochs +${row.fromEpochOffset} to +${row.throughEpochOffset}: ${row.count} deposits, ${row.capacity} shannons`}
            >
              <div
                data-window-bar
                className={`bg-gold w-full rounded-t-sm transition-opacity group-hover:opacity-100 ${row.nominalDay === 1 ? 'opacity-90' : 'opacity-50'}`}
                style={{ height: `${height}%` }}
              />
              <span className="text-text-dim mt-1 text-center font-mono text-[10px]">
                {row.nominalDay % 5 === 0 || row.nominalDay === 1 ? row.nominalDay : ''}
              </span>
            </div>
          );
        })}
      </div>
      <div className="text-text-dim mt-1 flex justify-between text-[11px]">
        <span>days from now · 6 epochs a day</span>
        <span>{plural(evidence.phase0.count, 'deposit')}</span>
      </div>
      <div className="text-text-bright mt-2 min-h-5 text-[12px]" aria-live="polite">
        {selectedDay === null
          ? null
          : (() => {
              const row = evidence.windows[selectedDay - 1];
              return (
                <>
                  Nominal day {selectedDay} · epochs +{row.fromEpochOffset} through +
                  {row.throughEpochOffset} · <Amount value={row.capacity} /> CKB in{' '}
                  {row.count.toLocaleString()} deposits
                </>
              );
            })()}
      </div>
      <p className="text-[12.5px]">
        <span className="text-text-bright">
          <Amount value={first.capacity} /> CKB
        </span>{' '}
        in {plural(first.count, 'deposit')} ends its cycle within a day.
      </p>
      <div className="border-base-border-subtle mt-3 flex flex-col gap-1.5 border-t pt-3">
        <span className="text-text-dim text-[12px]">
          withdrawals already requested · {plural(evidence.phase1.count, 'cell')} ·{' '}
          <span className="text-text">
            <Amount value={evidence.phase1.capacity} /> CKB
          </span>
        </span>
        {requested > BigInt(0) && (
          <div
            role="img"
            aria-label={`Withdrawal requests: past the window ${formatCapacity(readyShannons)}, waiting ${formatCapacity(waitingShannons)}`}
            className="flex h-2 w-full gap-0.5 overflow-hidden rounded"
          >
            <span
              className="bg-gold opacity-45"
              style={{ width: `${formatPercent(readyShannons, requested, 4)}%` }}
            />
            <span
              className="bg-gold"
              style={{ width: `${formatPercent(waitingShannons, requested, 4)}%` }}
            />
          </div>
        )}
        <span className="text-[12px] leading-relaxed">
          <span className="text-text-bright">
            <Amount value={ready.capacity} /> CKB
          </span>{' '}
          in {plural(ready.count, 'cell')} is past its window and can be claimed now; its
          compensation stopped growing at the request.{' '}
          <span className="text-text-bright">
            <Amount value={waiting.capacity} /> CKB
          </span>{' '}
          in {plural(waiting.count, 'cell')} waits for its window.
        </span>
        <span className="text-text-dim text-[11px]">
          Epoch-lock readiness alone does not prove a complete withdraw transaction; the required
          input, headers, witnesses and script execution still apply.
        </span>
      </div>
    </Section>
  );
}

type Owner = DaoEvidence['concentration']['owners'][number];

/**
 * The ranked groups of the concentration bar from the evidence's exact top-1 / 10 / 100 sums:
 * largest · next 9 · next 90 · the rest, naming only groups that have members.
 */
function concentrationGroups(evidence: DaoEvidence) {
  const c = evidence.concentration;
  const n = c.currentDepositors;
  const total = parseShannons(evidence.phase0.capacity, 'phase-0 capacity');
  const top = [
    parseShannons(c.top1Capacity, 'top1Capacity'),
    parseShannons(c.top10Capacity, 'top10Capacity'),
    parseShannons(c.top100Capacity, 'top100Capacity'),
    total,
  ];
  const bounds = [1, 10, 100, n];
  const groups: { label: string; value: bigint }[] = [];
  let before = 0;
  let previous = BigInt(0);
  bounds.forEach((bound, i) => {
    const members = Math.min(bound, n) - before;
    const value = top[i] - previous;
    if (members > 0) {
      const label =
        i === 0
          ? 'largest'
          : i === 3
            ? `the other ${members.toLocaleString('en-US')}`
            : `next ${members.toLocaleString('en-US')}`;
      groups.push({ label, value });
    } else if (value !== BigInt(0)) {
      throw new Error(
        `DAO concentration at ${formatBlockNumber(evidence.tipBlockNumber)}: ${value} shannons in an empty rank group`
      );
    }
    before = Math.max(before, Math.min(bound, n));
    previous = top[i];
  });
  return { total, groups };
}

function ownerLink(owner: Owner) {
  return (
    <Link href={`/address/${owner.address}`} className="text-text-bright hover:underline">
      {owner.address.slice(0, 12)}…{owner.address.slice(-7)}
    </Link>
  );
}

function lockLink(owner: Owner) {
  return (
    <Link
      href={`/script/${owner.lockCodeHash}?hashType=${owner.lockHashType}&kind=lock`}
      className="text-text-dim hover:text-text-bright font-mono hover:underline"
      title={`${owner.lockCodeHash} (${owner.lockHashType})`}
    >
      {owner.lockKind ?? `${owner.lockCodeHash.slice(0, 10)}…`} · {owner.lockHashType}
    </Link>
  );
}

const OWNER_COLUMNS: ListColumn<Owner>[] = [
  {
    key: 'rank',
    label: '#',
    minWidth: 40,
    render: (o) => <span className="text-text-dim">{o.rank}</span>,
  },
  { key: 'depositor', label: 'Depositor', minWidth: 190, render: ownerLink },
  { key: 'lock', label: 'Lock', minWidth: 170, render: lockLink },
  {
    key: 'deposits',
    label: 'Deposits',
    minWidth: 80,
    numeric: true,
    render: (o) => o.depositCount.toLocaleString('en-US'),
  },
  {
    key: 'capacity',
    label: 'CKB deposited',
    minWidth: 210,
    numeric: true,
    render: (o) => <Amount value={o.capacity} />,
  },
  {
    key: 'share',
    label: 'Share',
    minWidth: 70,
    numeric: true,
    render: (o) => <span className="text-text-dim">{o.sharePercent}%</span>,
  },
];

function Owners({ evidence }: { evidence: DaoEvidence }) {
  const { total, groups } = concentrationGroups(evidence);
  return (
    <Section
      title="Who deposits"
      note={
        <Link href="/dao/depositors" className="text-text hover:text-text-bright hover:underline">
          All {evidence.concentration.currentDepositors.toLocaleString('en-US')} depositors →
        </Link>
      }
    >
      {total > BigInt(0) && (
        <div className="mb-4">
          <ConcentrationBar
            label="Share of deposits"
            assetClass="dao"
            total={total}
            groups={groups}
          />
        </div>
      )}
      <ListTable
        label="Largest depositors"
        rows={evidence.concentration.owners}
        columns={OWNER_COLUMNS}
        rowKey={(o) => o.lockScriptHash}
        empty="No live deposits at this indexed tip."
        mobileRow={(o) => (
          <div className="space-y-1 text-[12px]">
            <div className="flex items-baseline justify-between gap-3">
              <span className="min-w-0">
                <span className="text-text-dim mr-2">{o.rank}</span>
                {ownerLink(o)}
              </span>
              <span className="text-text-dim shrink-0">{o.sharePercent}%</span>
            </div>
            <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
              {lockLink(o)}
              <span className="text-text-bright">
                <Amount value={o.capacity} /> CKB
                <span className="text-text-dim"> · {plural(o.depositCount, 'deposit')}</span>
              </span>
            </div>
          </div>
        )}
      />
    </Section>
  );
}

/** contracts W5-3: the block the statistics row was computed at; it must be the row's own tip. */
function statisticsAnchor(statistics: DaoStatistics): DaoStatistics['anchor'] {
  const { anchor } = statistics;
  if (!anchor || anchor.blockNumber !== statistics.tipBlockNumber)
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: anchor ${anchor ? formatBlockNumber(anchor.blockNumber) : 'missing'} is not the block the row was computed at`
    );
  if (!/^0x[0-9a-f]{64}$/.test(anchor.blockHash))
    throw new Error(
      `DAO statistics anchor ${formatBlockNumber(anchor.blockNumber)}: ${anchor.blockHash} is not a block hash`
    );
  return anchor;
}

/**
 * Tier 2 (board V63): the header's DAO field read in CKB with AR as its exact rate, and the DAO
 * script's rules. S = still owed + burnt is stated only when the statistics and the header are the
 * same block; across two blocks it would compare two different states (DAO-16).
 */
function RawEvidence({
  evidence,
  statistics,
}: {
  evidence: DaoEvidence;
  statistics: DaoStatistics;
}) {
  const header = evidence.header;
  // DAO-16 (R2b-a 44): S, still owed and burnt from ONE statistics row at ONE block, so the
  // identity is stated whatever block the header table below comes from; contracts W5-3: that
  // block, number and hash, is the row's own `anchor`.
  const anchor = statisticsAnchor(statistics);
  const statsS = parseShannons(statistics.tipSecondaryPool, 'tipSecondaryPool');
  const owed = parseShannons(statistics.unclaimedCompensation, 'unclaimedCompensation');
  const burnt = parseShannons(statistics.burnt, 'burnt');
  if (owed + burnt !== statsS)
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: S ${statsS} is not still owed + burnt ${owed + burnt}`
    );
  const fields = [
    {
      key: 'C',
      bytes: '0..8',
      value: (
        <>
          <Amount value={header.c} /> CKB issued in total
        </>
      ),
    },
    {
      key: 'AR',
      bytes: '8..16',
      value: <>{formatAr(header.ar)} accumulated rate (genesis 1.0)</>,
    },
    {
      key: 'S',
      bytes: '16..24',
      value: (
        <>
          <Amount value={header.s} /> CKB: compensation still owed to live deposits + the burnt
          share, which no cell can ever claim
        </>
      ),
    },
    {
      key: 'U',
      bytes: '24..32',
      value: (
        <>
          <Amount value={header.u} /> CKB occupied (incl. genesis virtual occupied)
        </>
      ),
    },
  ];
  return (
    <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
      <DetailSection
        title={`The DAO field in every block header · ${formatBlockNumber(header.blockNumber)}`}
      >
        <div className="text-[12px]">
          {fields.map((field) => (
            <div
              key={field.key}
              className="border-base-elevated grid grid-cols-[44px_96px_minmax(0,1fr)] gap-3 border-t py-1.5"
            >
              <span className="text-text-bright">{field.key}</span>
              <span className="text-text-dim">bytes {field.bytes}</span>
              <span className="min-w-0">{field.value}</span>
            </div>
          ))}
        </div>
        <div className="text-text-dim mt-3 break-all font-mono text-[11px]">{header.daoHex}</div>
        <p className="text-text-dim mt-3 text-[12px] leading-relaxed">
          Since genesis, AR grew from {formatAr(evidence.ar.genesisAr)} at genesis to{' '}
          {formatAr(evidence.ar.tipAr)} at block {formatBlockNumber(evidence.tipBlockNumber)}: free
          capacity deposited in block 0 would have grown by +{evidence.ar.sinceGenesisGrowthPercent}
          %. Compensation freezes at the withdrawal request AR. At block{' '}
          <Link href={`/blocks/${anchor.blockNumber}`} className="hover:text-text-bright underline">
            {formatBlockNumber(anchor.blockNumber)}
          </Link>{' '}
          (<span title={anchor.blockHash}>{truncateHash(anchor.blockHash)}</span>), S (
          <Amount value={statsS.toString()} /> CKB) equals the compensation still owed (
          <Amount value={owed.toString()} />) plus the burnt share (
          <Amount value={burnt.toString()} />) to the shannon: S only counts the burnt share, it
          does not hold it for anyone. Compensation paid + still owed equals the depositors’ share.
        </p>
        {evidence.ar.year365 && (
          <div className="text-text-dim mt-2 font-mono text-[11px]">
            365-day anchor: block {formatBlockNumber(evidence.ar.year365.anchorBlockNumber)} · chain
            time {formatUtc8DateTime(evidence.ar.year365.anchorChainTimeMs)} · AR{' '}
            {formatAr(evidence.ar.year365.anchorAr)} · observed span{' '}
            {formatDurationMs(
              exactSafeMs(evidence.ar.year365.elapsedChainTimeMs, 'DAO 365-day observed span')
            )}{' '}
            · the last canonical block at or before the 365-day chain-time cutoff, behind
            &ldquo;Compensation, last 365 days&rdquo;
          </div>
        )}
        <Link
          href={`/blocks/${header.blockNumber}`}
          className="text-text hover:text-text-bright mt-2 inline-block text-[12px] hover:underline"
        >
          Open source block →
        </Link>
      </DetailSection>
      <DetailSection title="The DAO script">
        <dl className="flex flex-col gap-2.5 text-[12px]">
          <div>
            <dt className="text-text-dim">Type script · type</dt>
            <dd className="mt-0.5">
              <Link
                href="/script/0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e?hashType=type&kind=type"
                className="text-text-bright break-all font-mono hover:underline"
              >
                0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e
              </Link>
            </dd>
            <dd className="text-text-dim mt-0.5">
              deployed in the genesis block; its code cell can never be spent
            </dd>
          </div>
          <div>
            <dt className="text-text-dim">Cell data, 8 bytes</dt>
            <dd className="mt-0.5">
              0 for a deposit · the deposit&apos;s block number (u64 LE) for a withdrawal request
            </dd>
          </div>
          <div>
            <dt className="text-text-dim">Withdrawing</dt>
            <dd className="mt-0.5 leading-relaxed">
              header deps: the deposit block and the request block · input since: the epoch where
              the cycle ends · paid out: (capacity − occupied) × AR(request) ÷ AR(deposit) +
              occupied
            </dd>
          </div>
          <div>
            <dt className="text-text-dim">A standard deposit cell</dt>
            <dd className="mt-0.5">102 bytes occupied with a secp256k1 lock: 8 + 53 + 33 + 8</dd>
          </div>
        </dl>
      </DetailSection>
    </DetailTier2>
  );
}

export function DaoBoard({
  statistics,
  evidence,
  depositedHistory,
  circulationHistory,
  historyError,
  intro,
}: Props) {
  if (
    evidence.phase0.capacity !== statistics.totalDeposited &&
    evidence.tipBlockNumber === statistics.tipBlockNumber
  ) {
    throw new Error(
      `DAO phase-0 capacity disagrees with indexed statistics at block ${evidence.tipBlockNumber}`
    );
  }
  return (
    <div>
      <div className="space-y-12">
        <Hero
          statistics={statistics}
          evidence={evidence}
          deposited={depositedHistory}
          ratio={circulationHistory}
          intro={intro}
        />
        <History deposited={depositedHistory} ratio={circulationHistory} error={historyError} />
        <Issuance statistics={statistics} />
        <div className="grid min-w-0 items-start gap-12 lg:grid-cols-2 lg:gap-14">
          <DaoGrows evidence={evidence} />
          <Windows evidence={evidence} />
        </div>
        <Owners evidence={evidence} />
        <DaoSources />
      </div>
      <RawEvidence evidence={evidence} statistics={statistics} />
    </div>
  );
}
