'use client';

import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Chart } from '@/components/ui/chart';
import { ReadoutRow } from '@/components/ui/readout-row';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { api, type DaoArHistory, type DaoEvidence } from '@/lib/api';
import { formatPercent, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
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
 * The AR curve over every complete day, named inside its 200 px plot with the latest AR at its end
 * (hierarchy v3), and the one reading under it: what 10,000 CKB deposited on the hovered or tapped
 * day (at that day's closing block) is owed by the evidence tip — from the API calculator, the one
 * compensation path, never a client formula. A tap pins the day (R6).
 */
function ArByDay({ days, evidence }: { days: ArDay[]; evidence: DaoEvidence }) {
  const pin = usePinnedReadout<number>();
  const index = pin.active ?? 0;
  const settled = useSettled(index, SETTLE_MS);
  const tip = evidence.tipBlockNumber;
  const day = days[index];
  const owed = useQuery({
    queryKey: ['dao-calculator', EXAMPLE_DEPOSIT, days[settled].blockNumber, tip],
    queryFn: () => api.calculateDaoCompensation(EXAMPLE_DEPOSIT, days[settled].blockNumber, tip),
    staleTime: Infinity,
  });
  const current = settled === index ? owed : null;
  return (
    <>
      {days.length > 1 && (
        <Chart
          title="AR by date"
          subtitle="AR at the close of every complete day since genesis"
          data={days.map((d) => ({ label: d.date, values: { ar: d.arIndex } }))}
          series={[{ key: 'ar', label: 'AR', assetClass: 'dao' }]}
          unit=""
          kind="line"
          height={200}
          pin={pin}
          plotName="AR, the accumulated rate · 1.0 at genesis"
          insideYTicks
          endLabel={(value) => arAtFourPlaces(formatExactDecimal(value, 'DAO accumulated rate'))}
          formatValue={(value) => formatExactDecimal(value, 'DAO accumulated rate')}
        />
      )}
      <div className="mt-2.5">
        <ReadoutRow label="Deposit reading" pinned={pin.showingPin}>
          <span className="text-text-dim">
            10,000 CKB deposited {day.date} →{' '}
            {current?.isError ? (
              <span role="alert" className="text-negative">
                (the calculator failed: {String(current.error)})
              </span>
            ) : current?.data ? (
              <span data-role="owed" className="text-text-bright">
                <OwedAmount
                  shannons={parseShannons(
                    current.data.estimatedCompensation,
                    `DAO calculator compensation for ${formatBlockNumber(day.blockNumber)}`
                  )}
                />{' '}
                CKB
              </span>
            ) : (
              <span>…</span>
            )}{' '}
            compensation by {formatBlockNumber(tip)}
          </span>
        </ReadoutRow>
      </div>
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

  // Hierarchy v3: the figure first — the AR plot named inside, its reading, 72 px year bars with one
  // axis line — then one caption on one line; what AR is, and why the curve flattens, read in tier 2.
  return (
    <section aria-label="How a deposit grows" className="flex min-w-0 flex-col">
      <h2 className="text-text-bright text-section mb-[18px] font-medium">How a deposit grows</h2>
      {history.isError ? (
        <div role="alert" className="text-rouge text-small">
          AR history could not be read: {String(history.error)}
        </div>
      ) : history.data === undefined ? (
        <div className="text-text-dim text-small">Loading the daily AR history…</div>
      ) : history.data.length === 0 ? (
        <div className="text-text-dim text-small">No complete day has closed yet.</div>
      ) : (
        <ArByDay days={history.data} evidence={evidence} />
      )}
      {years.length > 0 && (
        <div className="mt-5 flex flex-col">
          <div
            role="img"
            aria-label="Compensation on free capacity in each year since launch"
            className="flex h-[72px] items-end gap-2"
          >
            {years.map((y) => {
              const share = yearMax === 0 ? 0 : Number(y.percent) / yearMax;
              return (
                <div
                  key={y.from.blockNumber}
                  data-year={y.from.blockNumber}
                  title={`from ${formatUtc8Date(y.from.chainTimeMs)}: ${y.percent}%`}
                  className="flex min-w-0 flex-1 flex-col items-center gap-[3px]"
                >
                  <span className="text-text-bright text-small tabular-nums">{y.percent}%</span>
                  <span
                    aria-hidden="true"
                    className="bg-gold w-full rounded-sm"
                    style={{ height: `${Math.round(48 * share)}px`, opacity: 0.35 + 0.45 * share }}
                  />
                </div>
              );
            })}
          </div>
          <div className="text-text-dim text-meta mt-1 flex justify-between gap-4">
            <span>
              compensation in each year since launch ·{' '}
              {formatUtc8Date(years[0].from.chainTimeMs).slice(0, 7)} →
            </span>
            <span className="whitespace-nowrap">
              → {formatUtc8Date(years[years.length - 1].from.chainTimeMs).slice(0, 7)}
            </span>
          </div>
        </div>
      )}
      <p className="text-text text-body mt-3.5 max-w-[640px]">
        A deposit grows by AR at its withdrawal request ÷ AR at its deposit.
      </p>
    </section>
  );
}

/** AR cut (not rounded) to four decimal places for the line's end (the exact rate is in tier 2). */
function arAtFourPlaces(exact: string): string {
  const m = /^(\d+\.\d{4})\d*$/.exec(exact);
  if (!m) throw new Error(`DAO AR ${exact} is not a decimal rate`);
  return m[1];
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
