'use client';
import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';
import { useState, type ReactNode } from 'react';
import { Chart, alignedPlotLeft, type ChartValueAxisInput } from '@/components/ui/chart';
import { DetailSection } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { LockKind } from '@/components/ui/lock-kind';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatTokenBalance } from '@/lib/format-asset';
import type { TokenSupplyEvent } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { percentOf } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { widestTokenAmount } from '@/components/asset/asset-feed';
import { SHORT_FORMS } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { WIDEST_BLOCK_NUMBER, WIDEST_COUNT_DIGITS, shortChars } from '@/lib/list-bounds';

/** A run of characters at the cells' 13 px (readability F3-e: every column budgets what it holds). */
const cell = (chars: number) => textPx(chars, fontPx('small'));

const DAY = 86400000;
/** The weekly noun for one (the one plural rule, U-15). */
const WEEKLY_ONE = { transfers: 'transfer', transactions: 'transaction' } as const;
/** The main history plot's height (R13: the hero picture is at least 280 px; Main.dc.html). */
export const HISTORY_PLOT_HEIGHT = 280;
/**
 * The name of every CKB-held history plot — a token's, a collection's, a script's — and its unit,
 * said once (the ticks are bare). A fixed form: SVG text does not wrap, so a name carries no free
 * length part (the page's h1 says whose CKB); its fit at 328 px is pinned by plot-name-fit.test.
 */
export const HELD_PLOT_NAME = 'CKB held · occupied, free';
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
  /** PB-12: the page's class, whose colour its weekly bars are drawn in. */
  assetClass: 'tokens' | 'objects' | 'identities';
}
/** A small colour swatch for the readout (Token board), the same ink as its chart. */
function Swatch({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      className="mr-1.5 inline-block h-2 w-2 align-middle"
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
  assetClass,
}: AssetHistoryProps) {
  if (token && assetClass !== 'tokens')
    throw new Error(`A token's history is drawn in the token class, not ${assetClass}`);
  const [range, setRange] = useState<'1W' | '1M' | '1Y' | 'All'>('All');
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  // the supply events' phone rows are drawn at 1024 too, beside the holders' table (R9)
  const phoneSize = useTightBelowSm();
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
    // U-19 (lane A5): bare values in the gutter; the plot's name says the unit once.
    bareTicks: true,
  };
  const supplyUnit = token ? (token.decimals === null ? 'raw' : (token.symbol ?? 'tokens')) : null;
  const supplyAxis: ChartValueAxisInput | null = token
    ? {
        title: 'Supply',
        data: visible.map((p) => ({
          label: p.date,
          x: time(p.date),
          values: { supply: formatTokenBalance(p.supply!, token.decimals).replace(/,/g, '') },
        })),
        series: [{ key: 'supply', label: 'Supply', assetClass: 'tokens' }],
        unit: supplyUnit!,
        bareTicks: true,
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
              assetClass,
            },
          ],
          unit: weeklyNoun,
          kind: 'bars',
          bareTicks: true,
          // a count of transfers or transactions: whole-number ticks only
          integerTicks: true,
        }
      : null;
  // U-19 / DY-05 (UI review 2026-11-14): the round values sit in one gutter left of the plots, in
  // the dim ink — inside a plot the line crossed them, the bars stood in front of them and, by day,
  // the filled area buried them — and the three plots share it, as they share the time axis.
  const plotLeft = alignedPlotLeft(
    [capacityAxis, supplyAxis, weeklyAxis].filter((a): a is ChartValueAxisInput => a !== null)
  );
  // Readability F3-e: every column budgeted from what it holds, the 13 px cells and 12 px notes at
  // 0.6 em a glyph and the 16 px gap after every column but the last. The amounts hold one line
  // under a billion whole units at the token's own decimals; a larger one breaks after a thousands
  // separator (TokenAmount).
  const eventAmount = token ? widestTokenAmount(token.decimals) : '';
  const eventColumns: ListColumn<TokenSupplyEvent>[] = [
    {
      key: 'date',
      label: 'Date · UTC+8',
      // a date, "2024-01-02" (78 at 13 px), wider than the head's words, and the gap; it was 110
      minWidth: columnPx(cell('2024-01-02'.length) + CELL_GAP_PX),
      render: (e) => formatUtc8Date(e.timestamp),
    },
    {
      key: 'kind',
      label: 'Change',
      // the head, "Change" (43.2 at 12 px), wider than "Mint" or "Burn", and the gap; it was 65
      minWidth: columnPx(
        Math.max(textPx('Change'.length, fontPx('meta')), cell('Burn'.length)) + CELL_GAP_PX
      ),
      render: (e) => (
        <span className={e.kind === 'mint' ? 'text-positive' : 'text-negative'}>
          {e.kind === 'mint' ? 'Mint' : 'Burn'}
        </span>
      ),
    },
    {
      key: 'block',
      label: 'Block / transaction',
      // "#99,999,999" over "tx 9999 · sequence 9999" at 12 px (a block's transactions and a
      // transaction's transfers under 10,000; 165.6, the wider), and the gap; it was 160
      minWidth: columnPx(
        Math.max(
          cell(WIDEST_BLOCK_NUMBER.length),
          textPx(
            `tx ${WIDEST_COUNT_DIGITS} · sequence ${WIDEST_COUNT_DIGITS}`.length,
            fontPx('meta')
          )
        ) + CELL_GAP_PX
      ),
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
      // its sign and the widest amount on one line, and the gap; it was 160
      minWidth: columnPx(cell(`+${eventAmount}`.length) + CELL_GAP_PX),
      numeric: true,
      className: '!whitespace-normal',
      render: (e) => (
        <>
          {e.kind === 'mint' ? '+' : '−'}
          <TokenAmount form="column" amount={e.amount} decimals={token!.decimals} />
        </>
      ),
    },
    {
      key: 'after',
      label: 'Supply after',
      // the widest amount on one line and the gap; it was 160
      minWidth: columnPx(cell(eventAmount.length) + CELL_GAP_PX),
      numeric: true,
      className: '!whitespace-normal',
      render: (e) => (
        <TokenAmount form="column" amount={e.supplyAfter} decimals={token!.decimals} />
      ),
    },
    {
      key: 'party',
      label: 'To / from',
      // the party's lock hash at the row size (17 characters at 13 px; an address is 13) over its
      // lock, "lock 0x43434343…434343" at 12 px (158.4, the wider); the last column, no gap after
      // it. It was 160.
      minWidth: columnPx(
        Math.max(
          cell(shortChars(SHORT_FORMS.row.hash)),
          textPx('lock '.length + shortChars(SHORT_FORMS.row.hash), fontPx('meta'))
        )
      ),
      render: (e) => (
        <>
          <AppLink href={`/address/${e.address ?? e.lockHash}`}>
            <Hash
              hash={e.address ?? e.lockHash}
              kind={e.address ? 'address' : 'hash'}
              size="row"
              copyable={false}
              className="text-small"
            />
          </AppLink>
          {/* U-15: a lock the registry does not name reads "lock <code hash>". */}
          <LockKind
            lockKind={e.lockKind}
            lockCodeHash={e.lockCodeHash}
            size="row"
            className="text-text-dim text-meta block"
          />
        </>
      ),
    },
  ];
  const peakItem = peak && (
    <span title="the most CKB its cells held on any complete day">
      peak{' '}
      <CkbAmount form="column" shannons={BigInt(peak.capacity)} signed={false} tone="plain" wrap />{' '}
      on {unitNode(peak.date)}
    </span>
  );
  const weekText =
    weeklyUnavailable ??
    (week
      ? `Week of ${week.weekStart}: ${formatCount(week.count, WEEKLY_ONE[weeklyNoun], weeklyNoun)}`
      : // F2 objects (OB-C6): before any hover the reading is today, whose week is not
        // complete yet; it names the latest complete week instead of nothing.
        selectedDate === null && weekly.length > 0
        ? `Latest complete week, from ${weekly[weekly.length - 1].weekStart}: ${formatCount(weekly[weekly.length - 1].count, WEEKLY_ONE[weeklyNoun], weeklyNoun)}`
        : 'No completed weekly total for this day');
  const readoutItems: ReactNode[] = point
    ? [
        // Lane P (Main.dc.html): the scope is the hover of the day, not a line of its own.
        <span
          key="date"
          className="text-text-bright"
          title={`complete UTC+8 days through ${completeThrough} · indexed chain time`}
        >
          {unitNode(point.date)}
        </span>,
        <>
          held{' '}
          <CkbAmount
            form="column"
            shannons={BigInt(point.capacity)}
            signed={false}
            tone="plain"
            wrap
          />
        </>,
        <>
          <Swatch color={DATA_COLOR} />
          <span title="Occupied pays for the state kept on CKB.">occupied</span>{' '}
          <CkbAmount
            form="column"
            shannons={BigInt(point.occupiedCapacity)}
            signed={false}
            tone="plain"
            wrap
          />
          {BigInt(point.capacity) > BigInt(0) && (
            <span className="text-text-dim">
              {' '}
              {percentOf(BigInt(point.occupiedCapacity), BigInt(point.capacity))}
            </span>
          )}
        </>,
        <>
          <Swatch color={CLASS_COLORS.ckb} />
          <span title="Free is CKB held in the same cells beyond that byte cost.">free</span>{' '}
          <CkbAmount
            form="column"
            shannons={BigInt(point.capacity) - BigInt(point.occupiedCapacity)}
            signed={false}
            tone="plain"
            wrap
          />
        </>,
        ...(token
          ? [
              <>
                <Swatch color={CLASS_COLORS.tokens} />
                Supply{' '}
                <TokenAmount form="column" amount={point.supply!} decimals={token.decimals} />{' '}
                {token.symbol}
              </>,
            ]
          : []),
        weekText,
        ...(peakItem ? [peakItem] : []),
      ]
    : [];
  return (
    <DetailSection
      title="History"
      actions={
        <div role="group" aria-label="History time range" className="flex gap-1">
          {(['1W', '1M', '1Y', 'All'] as const).map((r) => (
            <button
              key={r}
              aria-pressed={range === r}
              className={`border-base-border text-small border px-2 py-1 ${range === r ? 'bg-base-elevated text-text-bright' : 'text-text-dim'}`}
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
      {point ? (
        <>
          <Chart
            {...capacityAxis}
            subtitle="Occupied and free"
            // The page's picture (R13): at least 280 px, its name inside it.
            height={HISTORY_PLOT_HEIGHT}
            // SVG text cannot wrap: the name fits beside the gutter on a 360 px phone (U-19).
            plotName={HELD_PLOT_NAME}
            plotLeft={plotLeft}
            formatValue={(value) => formatExactDecimal(value, 'asset CKB held')}
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
              // the unit once, as the ticks are bare (U-19)
              // a fixed form: the symbol has no length bound, the page's h1 names the token
              plotName={supplyUnit === 'raw' ? 'supply in raw units' : 'supply in tokens'}
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
              // the unit once, as the ticks are bare (U-19)
              plotName={`${weeklyNoun} per week`}
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
          {/* Lane P (Main.dc.html): the reading is one row under the plots — the day, held,
              occupied with its share, free, the week, then the peak among complete days. */}
          <div
            aria-live="polite"
            aria-label="History readout"
            className="border-row-rule text-small mt-2 border-t pt-2"
          >
            {/* PB-23: one separated list, so a wrapped item never opens its line with a "·" */}
            <Dotted className="gap-y-1" parts={readoutItems} />
          </div>
        </>
      ) : (
        <>
          <p
            className="text-text-dim text-small py-6"
            title={`complete UTC+8 days through ${completeThrough} · indexed chain time`}
          >
            No complete daily history in this range.
          </p>
          {peakItem && <p className="text-text-dim text-small max-w-measure">{peakItem}</p>}
        </>
      )}
      {footnote && <div className="text-text-dim text-small mt-3">{footnote}</div>}
      {supplyEvents && token && (
        <details className="border-base-border mt-4 border-t pt-3">
          <summary className="text-small cursor-pointer">
            Every change to the supply ({supplyEvents.total.toLocaleString('en-US')})
          </summary>
          {!supplyPager && supplyEvents.rows.length < supplyEvents.total && (
            <p className="text-text-dim text-meta max-w-measure mt-2">
              The newest {supplyEvents.rows.length.toLocaleString('en-US')} are listed.
            </p>
          )}
          <p className="text-text-dim text-meta max-w-measure my-3">
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
                  {eventColumns[1].render(e)} {e.kind === 'mint' ? '+' : '−'}
                  <TokenAmount form="summary" amount={e.amount} decimals={token.decimals} />
                </span>
              );
              const after = (
                <span>
                  supply after{' '}
                  <TokenAmount form="summary" amount={e.supplyAfter} decimals={token.decimals} />
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
              // the address is the unit (Hash, shortened); the lock name is words (A4 Chrome
              // sweep). The rows are drawn at 1024 too, beside the holders' table: tight only
              // below sm (R9).
              const party = (
                <span>
                  {e.kind === 'mint' ? 'to ' : 'from '}
                  <AppLink href={`/address/${e.address ?? e.lockHash}`}>
                    <Hash
                      hash={e.address ?? e.lockHash}
                      kind={e.address ? 'address' : 'hash'}
                      size={phoneSize}
                      copyable={false}
                      className="text-meta"
                    />
                  </AppLink>{' '}
                  <LockKind
                    lockKind={e.lockKind}
                    lockCodeHash={e.lockCodeHash}
                    size={phoneSize}
                    className="text-text-dim text-meta"
                  />
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
