'use client';

import { useState } from 'react';
import { useQueries } from '@tanstack/react-query';
import { Chart } from '@/components/ui/chart';
import { api, type DaoEvidence } from '@/lib/api';
import { formatPercent, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatAccumulatedRate } from '@/lib/dao-header';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date } from '@/lib/utils';

/** The board's example deposit: 10,000 CKB, in shannons. */
const EXAMPLE_DEPOSIT = '1000000000000';

type Milestone = DaoEvidence['ar']['milestones'][number];

function ar(value: string, context: string): bigint {
  const n = exactUnsignedInteger(value, context);
  if (n === BigInt(0)) throw new Error(`${context}: AR is zero`);
  return n;
}

function chainMs(m: Milestone): number {
  const ms = exactUnsignedInteger(
    m.chainTimeMs,
    `DAO AR milestone ${formatBlockNumber(m.blockNumber)} chain time`
  );
  if (ms > BigInt(Number.MAX_SAFE_INTEGER))
    throw new Error(
      `DAO AR milestone ${formatBlockNumber(m.blockNumber)} chain time ${m.chainTimeMs} is out of range`
    );
  return Number(ms);
}

/**
 * "How a deposit grows" (board V63, DAO-8): AR by date from the evidence's yearly chain-time
 * anchors and the tip, a reading of what 10,000 CKB deposited on that date is owed by the tip (the
 * API calculator's one compensation path, never a client formula), and the compensation of each
 * whole year since launch (anniversary to anniversary; the year in progress is not a bar).
 */
export function DaoGrows({ evidence }: { evidence: DaoEvidence }) {
  const milestones = evidence.ar.milestones;
  if (milestones.length === 0) throw new Error('DAO evidence has no AR milestones');
  const [active, setActive] = useState<number | null>(null);
  const tip = evidence.tipBlockNumber;
  const tipAr = ar(evidence.ar.tipAr, 'DAO tip AR');
  const owed = useQueries({
    queries: milestones.map((m) => ({
      queryKey: ['dao-calculator', EXAMPLE_DEPOSIT, m.blockNumber, tip],
      queryFn: () => api.calculateDaoCompensation(EXAMPLE_DEPOSIT, m.blockNumber, tip),
      staleTime: Infinity,
    })),
  });
  const index = active ?? 0;
  const point = milestones[index];
  const pointAr = ar(point.ar, `DAO AR at ${formatBlockNumber(point.blockNumber)}`);
  if (pointAr > tipAr)
    throw new Error(
      `DAO AR at ${formatBlockNumber(point.blockNumber)} exceeds the tip AR at ${formatBlockNumber(tip)}`
    );
  const result = owed[index];
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
  const times = milestones.map(chainMs);

  return (
    <section aria-label="How a deposit grows" className="flex min-w-0 flex-col gap-3">
      <h2 className="text-text-bright text-[15px] font-medium">How a deposit grows</h2>
      <p className="text-text text-[13px] leading-relaxed">
        AR, in every block header, is the DAO&apos;s growth index:{' '}
        {formatAccumulatedRate(ar(evidence.ar.genesisAr, 'DAO genesis AR'))} at genesis,{' '}
        {formatAccumulatedRate(tipAr)} now. A deposit grows by AR at its withdrawal request ÷ AR at
        its deposit, on the capacity it does not occupy.
      </p>
      <div
        aria-live="polite"
        aria-label="Deposit reading"
        className="border-base-border-subtle flex min-h-10 flex-wrap items-baseline gap-x-1 gap-y-1 border-b pb-2 pt-1.5 text-[12px]"
      >
        <span className="text-text-bright">
          10,000 CKB deposited on {formatUtc8Date(point.chainTimeMs)}
        </span>{' '}
        <span className="text-text-dim">is owed</span>{' '}
        {result.isError ? (
          <span role="alert" className="text-negative">
            (the calculator failed: {String(result.error)})
          </span>
        ) : result.data ? (
          <span data-role="owed" className="text-gold">
            <OwedAmount
              shannons={parseShannons(
                result.data.estimatedCompensation,
                `DAO calculator compensation for ${formatBlockNumber(point.blockNumber)}`
              )}
            />{' '}
            CKB compensation
          </span>
        ) : (
          <span className="text-text-dim">…</span>
        )}
        {' · '}
        <span className="text-text-dim">
          {formatPercent(tipAr - pointAr, pointAr, 4)}% on its free capacity, by{' '}
          {formatBlockNumber(tip)}
        </span>
      </div>
      {milestones.length > 1 && (
        <Chart
          title="AR by date"
          subtitle="AR on each anniversary of genesis and at the indexed tip"
          data={milestones.map((m, i) => ({
            label: formatUtc8Date(m.chainTimeMs),
            x: times[i],
            values: { ar: m.arIndex },
          }))}
          series={[{ key: 'ar', label: 'AR', assetClass: 'dao' }]}
          unit=""
          kind="area"
          height={170}
          xDomain={[times[0], times[times.length - 1]]}
          activeIndex={active}
          onActiveIndexChange={setActive}
          hideCaption
          formatValue={(value) => formatExactDecimal(value, 'DAO accumulated rate')}
        />
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
