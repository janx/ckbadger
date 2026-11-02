'use client';
import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';
import { useState, type ReactNode } from 'react';
import { alignedPlotLeft, Chart, type ChartValueAxisInput } from '@/components/ui/chart';
import { DetailSection } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { AssetAmount } from '@/components/asset/inventory-table';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatTokenBalance } from '@/lib/format-asset';
import type { TokenSupplyEvent } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
const DAY = 86400000;
export interface AssetHistoryProps {
  daily: readonly { date: string; capacity: string; occupiedCapacity: string; supply?: string }[];
  /** Completed weekly totals, or why this page has none to draw. */
  weekly: readonly { weekStart: string; count: number }[] | { unavailable: string };
  /** What the weekly totals count. */
  weeklyNoun: 'transfers' | 'transactions';
  completeThrough: string;
  token?: { symbol: string | null; decimals: number | null };
  /** The newest supply events and how many there are in all. */
  supplyEvents?: { rows: readonly TokenSupplyEvent[]; total: number };
  // ── F2 objects (round 2, IV-17 / IV-27) ──
  /** A line under the charts (the Token board's "N mints · N burns · N transfers"). */
  footnote?: ReactNode;
  /** Paging controls for the supply events; replaces the "newest N are listed" note. */
  supplyPager?: ReactNode;
}
/** A small colour swatch for the readout (Token board), the same ink as its chart. */
function Swatch({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      className="mr-1.5 inline-block h-2 w-2 rounded-sm align-middle"
      style={{ backgroundColor: color }}
    />
  );
}
function time(date: string) {
  const value = Date.parse(`${date}T00:00:00+08:00`);
  if (
    !/^\d{4}-\d{2}-\d{2}$/.test(date) ||
    !Number.isFinite(value) ||
    new Date(value + 8 * 3600000).toISOString().slice(0, 10) !== date
  )
    throw new Error(`Invalid indexed history date ${date}`);
  return value;
}
function ckbDecimal(value: string) {
  return formatTokenBalance(value, 8).replace(/,/g, '');
}
export function AssetHistory({
  daily,
  weekly: weeklySource,
  weeklyNoun,
  completeThrough,
  token,
  supplyEvents,
  footnote,
  supplyPager,
}: AssetHistoryProps) {
  const [range, setRange] = useState<'1W' | '1M' | '1Y' | 'All'>('All');
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const end = time(completeThrough),
    from = range === 'All' ? null : end - { '1W': 6, '1M': 29, '1Y': 364 }[range] * DAY;
  let previous = -Infinity;
  for (const p of daily) {
    const t = time(p.date);
    if (t <= previous || t > end) throw new Error(`History ${p.date}: unordered or incomplete day`);
    previous = t;
    if (
      exactUnsignedInteger(p.occupiedCapacity, `${p.date} occupied`) >
      exactUnsignedInteger(p.capacity, `${p.date} capacity`)
    )
      throw new Error(`${p.date}: occupied exceeds held capacity`);
    if (token && p.supply === undefined) throw new Error(`${p.date}: token supply history missing`);
  }
  const weeklyUnavailable = 'unavailable' in weeklySource ? weeklySource.unavailable : null;
  const weekly = 'unavailable' in weeklySource ? [] : weeklySource;
  let previousWeek = -Infinity;
  for (const w of weekly) {
    const t = time(w.weekStart);
    if (t <= previousWeek || t + 6 * DAY > end || !Number.isSafeInteger(w.count) || w.count < 0)
      throw new Error(`History ${w.weekStart}: invalid or incomplete weekly ${weeklyNoun} total`);
    previousWeek = t;
  }
  const visible = daily.filter((p) => from === null || time(p.date) >= from);
  const domain: readonly [number, number] | undefined =
    visible.length > 1
      ? [time(visible[0].date), time(visible[visible.length - 1].date)]
      : undefined;
  const weeks = weekly.filter(
    (p) =>
      (from === null || time(p.weekStart) >= from) &&
      (!domain || (time(p.weekStart) >= domain[0] && time(p.weekStart) <= domain[1]))
  );
  const activeIndex =
    selectedDate === null ? null : visible.findIndex((p) => p.date === selectedDate);
  const active = activeIndex === null || activeIndex < 0 ? null : activeIndex;
  const point = visible[active === null ? visible.length - 1 : active];
  const week = point
    ? weekly.find(
        (w) =>
          time(w.weekStart) <= time(point.date) && time(point.date) < time(w.weekStart) + 7 * DAY
      )
    : undefined;
  const weekActive = week ? weeks.indexOf(week) : -1;
  const peak = daily.reduce<(typeof daily)[number] | undefined>(
    (best, p) => (!best || BigInt(p.capacity) > BigInt(best.capacity) ? p : best),
    undefined
  );
  const onDaily = (i: number | null) => setSelectedDate(i === null ? null : visible[i].date);
  const capacityPoints = visible.map((p) => ({
    label: p.date,
    x: time(p.date),
    values: {
      occupied: ckbDecimal(p.occupiedCapacity),
      free: ckbDecimal((BigInt(p.capacity) - BigInt(p.occupiedCapacity)).toString()),
    },
  }));
  // The three plots share one time axis, so one gutter: the one their longest tick needs.
  const capacityAxis: ChartValueAxisInput = {
    title: 'CKB held in its cells',
    data: capacityPoints,
    series: [
      { key: 'occupied', label: 'Occupied', color: DATA_COLOR },
      { key: 'free', label: 'Free', color: CLASS_COLORS.ckb },
    ],
    unit: 'CKB',
    kind: 'stacked',
  };
  const supplyAxis: ChartValueAxisInput | null = token
    ? {
        title: 'Supply',
        data: visible.map((p) => ({
          label: p.date,
          x: time(p.date),
          values: { supply: formatTokenBalance(p.supply!, token.decimals).replace(/,/g, '') },
        })),
        series: [{ key: 'supply', label: 'Supply', assetClass: 'tokens' }],
        unit: token.decimals === null ? 'raw' : (token.symbol ?? 'tokens'),
      }
    : null;
  const weeklyAxis: ChartValueAxisInput | null =
    weeklyUnavailable === null
      ? {
          title: `${weeklyNoun === 'transfers' ? 'Transfers' : 'Transactions'} per week`,
          data: weeks.map((w) => ({
            label: w.weekStart,
            x: time(w.weekStart),
            values: { count: String(w.count) },
          })),
          series: [
            {
              key: 'count',
              label: weeklyNoun === 'transfers' ? 'Transfers' : 'Transactions',
              assetClass: token ? 'tokens' : 'objects',
            },
          ],
          unit: weeklyNoun,
          kind: 'bars',
        }
      : null;
  const plotLeft = alignedPlotLeft(
    [capacityAxis, supplyAxis, weeklyAxis].filter((a): a is ChartValueAxisInput => a !== null)
  );
  const eventColumns: ListColumn<TokenSupplyEvent>[] = [
    {
      key: 'date',
      label: 'Date · UTC+8',
      minWidth: 110,
      render: (e) => formatUtc8Date(e.timestamp),
    },
    {
      key: 'kind',
      label: 'Change',
      minWidth: 65,
      render: (e) => (
        <span className={e.kind === 'mint' ? 'text-positive' : 'text-negative'}>
          {e.kind === 'mint' ? 'Mint' : 'Burn'}
        </span>
      ),
    },
    {
      key: 'block',
      label: 'Block / transaction',
      minWidth: 160,
      render: (e) => (
        <>
          <AppLink href={`/blocks/${e.blockNumber}`}>{formatBlockNumber(e.blockNumber)}</AppLink>
          <AppLink className="text-text-dim text-meta block" href={`/tx/${e.txHash}`}>
            tx {e.txIndex} · sequence {e.transferSequence}
          </AppLink>
        </>
      ),
    },
    {
      key: 'amount',
      label: 'Amount',
      minWidth: 160,
      numeric: true,
      className: '!whitespace-normal',
      render: (e) => (
        <>
          {e.kind === 'mint' ? '+' : '−'}
          <AssetAmount amount={e.amount} decimals={token!.decimals} />
        </>
      ),
    },
    {
      key: 'after',
      label: 'Supply after',
      minWidth: 160,
      numeric: true,
      className: '!whitespace-normal',
      render: (e) => <AssetAmount amount={e.supplyAfter} decimals={token!.decimals} />,
    },
    {
      key: 'party',
      label: 'To / from',
      minWidth: 160,
      render: (e) => (
        <>
          <AppLink href={`/address/${e.address ?? e.lockHash}`}>
            <Hash
              hash={e.address ?? e.lockHash}
              copyable={false}
              startChars={8}
              endChars={4}
              className="text-xs"
            />
          </AppLink>
          <span className="text-text-dim text-meta block">
            {e.lockKind ?? 'Lock kind unavailable'}
          </span>
        </>
      ),
    },
  ];
  return (
    <DetailSection
      title="History"
      actions={
        <div role="group" aria-label="History time range" className="flex gap-1">
          {(['1W', '1M', '1Y', 'All'] as const).map((r) => (
            <button
              key={r}
              aria-pressed={range === r}
              className={`border-base-border rounded border px-2 py-1 text-xs ${range === r ? 'bg-base-elevated text-text-bright' : 'text-text-dim'}`}
              onClick={() => {
                setRange(r);
                setSelectedDate(null);
              }}
            >
              {r}
            </button>
          ))}
        </div>
      }
    >
      <p className="text-text-dim text-meta mb-3">
        Complete UTC+8 days through {completeThrough} · indexed chain time
      </p>
      {point ? (
        <>
          <div
            aria-live="polite"
            aria-label="History readout"
            className="border-base-border mb-2 flex flex-wrap gap-x-5 gap-y-2 border-b pb-3 text-xs"
          >
            <span className="text-text-bright">{point.date}</span>
            <span>
              CKB held{' '}
              <ExactCkb shannons={BigInt(point.capacity)} signed={false} tone="plain" wrap />
            </span>
            <span>
              <Swatch color={DATA_COLOR} />
              occupied{' '}
              <ExactCkb
                shannons={BigInt(point.occupiedCapacity)}
                signed={false}
                tone="plain"
                wrap
              />
            </span>
            <span>
              <Swatch color={CLASS_COLORS.ckb} />
              free{' '}
              <ExactCkb
                shannons={BigInt(point.capacity) - BigInt(point.occupiedCapacity)}
                signed={false}
                tone="plain"
                wrap
              />
            </span>
            {token && (
              <span>
                <Swatch color={CLASS_COLORS.tokens} />
                Supply <AssetAmount amount={point.supply!} decimals={token.decimals} />{' '}
                {token.symbol}
              </span>
            )}
            <span>
              {weeklyUnavailable ??
                (week
                  ? `Week of ${week.weekStart}: ${week.count.toLocaleString('en-US')} ${weeklyNoun}`
                  : // F2 objects (OB-C6): before any hover the reading is today, whose week is not
                    // complete yet; it names the latest complete week instead of nothing.
                    selectedDate === null && weekly.length > 0
                    ? `Latest complete week, from ${weekly[weekly.length - 1].weekStart}: ${weekly[weekly.length - 1].count.toLocaleString('en-US')} ${weeklyNoun}`
                    : 'No completed weekly total for this day')}
            </span>
          </div>
          <Chart
            {...capacityAxis}
            subtitle="Occupied and free"
            height={token ? 150 : 200}
            formatValue={(value) => formatExactDecimal(value, 'asset CKB held')}
            plotLeft={plotLeft}
            xDomain={domain}
            activeIndex={active}
            onActiveIndexChange={onDaily}
            hideCaption
            hideXAxis
            cursorX={selectedDate === null ? null : time(selectedDate)}
          />
          {supplyAxis && (
            <Chart
              {...supplyAxis}
              subtitle="Exact total supply"
              formatValue={(value) => formatExactDecimal(value, 'token supply')}
              height={130}
              plotLeft={plotLeft}
              xDomain={domain}
              activeIndex={active}
              onActiveIndexChange={onDaily}
              hideCaption
              hideXAxis
              cursorX={selectedDate === null ? null : time(selectedDate)}
            />
          )}
          {weeklyAxis && (
            <Chart
              {...weeklyAxis}
              subtitle="Completed UTC+8 weeks"
              formatValue={(value) => formatExactDecimal(value, `weekly ${weeklyNoun}`)}
              height={120}
              plotLeft={plotLeft}
              xDomain={domain}
              activeIndex={selectedDate === null || weekActive < 0 ? null : weekActive}
              onActiveIndexChange={(i) => setSelectedDate(i === null ? null : weeks[i].weekStart)}
              hideCaption
              cursorX={selectedDate === null ? null : time(selectedDate)}
            />
          )}
        </>
      ) : (
        <p className="text-text-dim py-6 text-xs">No complete daily history in this range.</p>
      )}
      {footnote && <div className="text-text-dim mt-3 text-xs">{footnote}</div>}
      {peak && (
        <p className="text-text-dim mt-3 text-xs">
          CKB held peaked at{' '}
          <ExactCkb shannons={BigInt(peak.capacity)} signed={false} tone="plain" wrap /> on{' '}
          {peak.date}, among complete days.
        </p>
      )}
      <p className="text-text-dim mt-3 text-xs">
        Occupied pays for the state kept on CKB. Free is CKB held in the same cells beyond that byte
        cost.
      </p>
      {supplyEvents && token && (
        <details className="border-base-border mt-4 border-t pt-3">
          <summary className="cursor-pointer text-xs">
            Every change to the supply ({supplyEvents.total.toLocaleString('en-US')})
          </summary>
          {!supplyPager && supplyEvents.rows.length < supplyEvents.total && (
            <p className="text-text-dim text-meta mt-2">
              The newest {supplyEvents.rows.length.toLocaleString('en-US')} are listed.
            </p>
          )}
          <p className="text-text-dim text-meta my-3">
            Event dates use raw block timestamps in UTC+8. Daily charts use indexed chain time,
            which preserves time through backward block timestamps.
          </p>
          <ListTable
            label="Mints and burns"
            columns={eventColumns}
            rows={supplyEvents.rows}
            rowKey={(e) => `${e.txHash}:${e.transferSequence}`}
            empty="No supply changes."
            mobileRow={(e, wide) => {
              const change = (
                <span className="whitespace-nowrap">
                  {eventColumns[1].render(e)} {eventColumns[3].render(e)}
                </span>
              );
              const after = (
                <span>
                  supply after <AssetAmount amount={e.supplyAfter} decimals={token.decimals} />
                </span>
              );
              const where = [
                <AppLink key="b" href={`/blocks/${e.blockNumber}`}>
                  {formatBlockNumber(e.blockNumber)}
                </AppLink>,
                <AppLink key="t" href={`/tx/${e.txHash}`} className="whitespace-nowrap">
                  tx {e.txIndex} · sequence {e.transferSequence}
                </AppLink>,
              ];
              const party = (
                <span className="whitespace-nowrap">
                  {e.kind === 'mint' ? 'to ' : 'from '}
                  <AppLink href={`/address/${e.address ?? e.lockHash}`}>
                    <Hash
                      hash={e.address ?? e.lockHash}
                      copyable={false}
                      startChars={8}
                      endChars={4}
                      className="text-meta"
                    />
                  </AppLink>{' '}
                  {e.lockKind ?? 'Lock kind unavailable'}
                </span>
              );
              return (
                <PhoneRow
                  testId="supply-event-row-mobile"
                  lines={[
                    {
                      key: 'change',
                      tone: 'head',
                      left: change,
                      right: formatUtc8Date(e.timestamp),
                    },
                    !wide && { key: 'after', tone: 'body', left: after },
                    {
                      key: 'evidence',
                      tone: 'quiet',
                      left: <Dotted parts={wide ? [after, ...where] : where} />,
                      right: party,
                    },
                  ]}
                />
              );
            }}
          />
          {supplyPager}
        </details>
      )}
    </DetailSection>
  );
}
