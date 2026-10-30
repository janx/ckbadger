'use client';

import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Chart } from '@/components/ui/chart';
import { api, type DaoArHistory, type DaoEvidence } from '@/lib/api';
import { formatPercent, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatAccumulatedRate } from '@/lib/dao-header';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date } from '@/lib/utils';

/** The board's example deposit: 10,000 CKB, in shannons. */
const EXAMPLE_DEPOSIT = '1000000000000';
const DAY_MS = 86_400_000;
/** A hovered day asks the calculator once the pointer rests this long, not on every pixel. */
const SETTLE_MS = 150;

type ArDay = DaoArHistory['days'][number];

function ar(value: string, context: string): bigint {
  const n = exactUnsignedInteger(value, context);
  if (n === BigInt(0)) throw new Error(`${context}: AR is zero`);
  return n;
}

/** The daily series as served: consecutive complete UTC+8 days, AR never falling. */
function checkedDays(history: DaoArHistory): ArDay[] {
  const { days } = history;
  days.forEach((day, i) => {
    ar(day.ar, `DAO AR on ${day.date}`);
    if (i === 0) return;
    const prev = days[i - 1];
    if (Date.parse(`${day.date}T00:00:00Z`) - Date.parse(`${prev.date}T00:00:00Z`) !== DAY_MS)
      throw new Error(`DAO AR history skips from ${prev.date} to ${day.date}`);
    if (BigInt(day.ar) < BigInt(prev.ar))
      throw new Error(`DAO AR history falls from ${prev.date} to ${day.date}`);
  });
  return days;
}

function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

/**
 * The AR curve over every complete day and the one reading on it: what 10,000 CKB deposited on the
 * hovered day (at that day's closing block) is owed by the evidence tip — from the API calculator,
 * the one compensation path, never a client formula.
 */
function ArByDay({ days, evidence }: { days: ArDay[]; evidence: DaoEvidence }) {
  const [active, setActive] = useState<number | null>(null);
  const index = active ?? 0;
  const settled = useSettled(index, SETTLE_MS);
  const tip = evidence.tipBlockNumber;
  const tipAr = ar(evidence.ar.tipAr, 'DAO tip AR');
  const day = days[index];
  const dayAr = ar(day.ar, `DAO AR on ${day.date}`);
  const owed = useQuery({
    queryKey: ['dao-calculator', EXAMPLE_DEPOSIT, days[settled].blockNumber, tip],
    queryFn: () => api.calculateDaoCompensation(EXAMPLE_DEPOSIT, days[settled].blockNumber, tip),
    staleTime: Infinity,
  });
  const current = settled === index ? owed : null;
  const free = current?.data
    ? splitCkbAmount(parseShannons(current.data.freeCapacity, 'DAO calculator freeCapacity'))
    : null;
  return (
    <>
      <div
        aria-live="polite"
        aria-label="Deposit reading"
        className="border-base-border-subtle flex min-h-10 flex-wrap items-baseline gap-x-1 gap-y-1 border-b pb-2 pt-1.5 text-[12px]"
      >
        <span className="text-text-bright">10,000 CKB deposited on {day.date}</span>{' '}
        <span className="text-text-dim">is owed</span>{' '}
        {current?.isError ? (
          <span role="alert" className="text-negative">
            (the calculator failed: {String(current.error)})
          </span>
        ) : current?.data ? (
          <span data-role="owed" className="text-gold">
            <OwedAmount
              shannons={parseShannons(
                current.data.estimatedCompensation,
                `DAO calculator compensation for ${formatBlockNumber(day.blockNumber)}`
              )}
            />{' '}
            CKB compensation
          </span>
        ) : (
          <span className="text-text-dim">…</span>
        )}
        {' · '}
        <span className="text-text-dim">
          {dayAr > tipAr
            ? `AR on ${day.date} is above the tip's`
            : `${formatPercent(tipAr - dayAr, dayAr, 4)}%`}{' '}
          on its {free ? `${free.integer}${free.fraction}` : '…'} free CKB, by{' '}
          {formatBlockNumber(tip)}
        </span>
      </div>
      {days.length > 1 && (
        <Chart
          title="AR by date"
          subtitle="AR at the close of every complete day since genesis"
          data={days.map((d) => ({ label: d.date, values: { ar: d.arIndex } }))}
          series={[{ key: 'ar', label: 'AR', assetClass: 'dao' }]}
          unit=""
          kind="line"
          height={170}
          activeIndex={active}
          onActiveIndexChange={setActive}
          hideCaption
          formatValue={(value) => formatExactDecimal(value, 'DAO accumulated rate')}
        />
      )}
    </>
  );
}

/**
 * "How a deposit grows" (board V63, DAO-8): AR by date from `/dao/ar-history` (one point per
 * complete UTC+8 day), the owed reading for any day, and the compensation of each whole year since
 * launch from the evidence's anniversary milestones (the year in progress is not a bar).
 */
export function DaoGrows({ evidence }: { evidence: DaoEvidence }) {
  const milestones = evidence.ar.milestones;
  if (milestones.length === 0) throw new Error('DAO evidence has no AR milestones');
  const tipAr = ar(evidence.ar.tipAr, 'DAO tip AR');
  const history = useQuery({
    queryKey: ['dao-ar-history'],
    queryFn: api.getDaoArHistory,
    select: checkedDays,
    staleTime: 5 * 60_000,
  });
  const years = milestones.slice(0, -2).map((from, k) => {
    const to = milestones[k + 1];
    const a = ar(from.ar, `DAO AR at ${formatBlockNumber(from.blockNumber)}`);
    const b = ar(to.ar, `DAO AR at ${formatBlockNumber(to.blockNumber)}`);
    if (b < a)
      throw new Error(
        `DAO AR fell from ${formatBlockNumber(from.blockNumber)} to ${formatBlockNumber(to.blockNumber)}`
      );
    return { from, percent: formatPercent(b - a, a, 2) };
  });
  const yearMax = Math.max(...years.map((y) => Number(y.percent)), 0);
  const year365 = evidence.ar.year365;

  return (
    <section aria-label="How a deposit grows" className="flex min-w-0 flex-col gap-3">
      <h2 className="text-text-bright text-[15px] font-medium">How a deposit grows</h2>
      <p className="text-text text-[13px] leading-relaxed">
        AR, in every block header, is the DAO&apos;s growth index:{' '}
        {formatAccumulatedRate(ar(evidence.ar.genesisAr, 'DAO genesis AR'))} at genesis,{' '}
        {formatAccumulatedRate(tipAr)} now. A deposit grows by AR at its withdrawal request ÷ AR at
        its deposit, on the capacity it does not occupy.
      </p>
      {history.isError ? (
        <div role="alert" className="text-rouge text-xs">
          AR history could not be read: {String(history.error)}
        </div>
      ) : history.data === undefined ? (
        <div className="text-text-dim text-xs">Loading the daily AR history…</div>
      ) : history.data.length === 0 ? (
        <div className="text-text-dim text-xs">No complete day has closed yet.</div>
      ) : (
        <ArByDay days={history.data} evidence={evidence} />
      )}
      {years.length > 0 && (
        <div className="mt-1.5 flex flex-col gap-1">
          <span className="text-text-dim text-[12px]">compensation in each year since launch</span>
          <div
            role="img"
            aria-label="Compensation on free capacity in each year since launch"
            className="flex items-end gap-1.5 pt-1.5"
          >
            {years.map((y) => {
              const share = yearMax === 0 ? 0 : Number(y.percent) / yearMax;
              return (
                <div
                  key={y.from.blockNumber}
                  data-year={y.from.blockNumber}
                  className="flex min-w-0 flex-1 flex-col items-center gap-1"
                >
                  <span className="text-text-bright text-[11px]">{y.percent}%</span>
                  <span
                    aria-hidden="true"
                    className="bg-gold w-[70%] rounded-t-sm"
                    style={{ height: `${Math.round(54 * share)}px`, opacity: 0.35 + 0.65 * share }}
                  />
                  <span className="text-text-dim text-[10.5px]">
                    from {formatUtc8Date(y.from.chainTimeMs).slice(0, 7)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
      {years.length > 0 && year365 && (
        <span className="text-text-dim text-[11.5px]">
          The curve flattens as more CKB exists: the first year compensated {years[0].percent}%, the
          last 365 days {year365.observedGrowthPercent}%. Hover a date to see the compensation owed
          to a deposit made then.
        </span>
      )}
    </section>
  );
}

function OwedAmount({ shannons }: { shannons: bigint }) {
  const parts = splitCkbAmount(shannons);
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      +{parts.integer}
      <span className="opacity-70">{parts.fraction}</span>
      {parts.ghost}
    </span>
  );
}
