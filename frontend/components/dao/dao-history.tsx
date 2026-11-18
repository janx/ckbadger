'use client';

import { useId, type MouseEvent, type PointerEvent, type ReactNode } from 'react';
import type { ChartResponse } from '@/lib/api';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { PinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { ckbAmountText, ckbShortenedTitle } from '@/lib/amount-form';
import { formatSignedCkbInteger, parseCkbDecimal } from '@/lib/ckb-decimal';
import { formatTick, niceCeil, niceTicks, tickGutter } from '@/lib/chart-ticks';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';
import { usePinWord } from '@/hooks/usePinWord';
import { formatCount } from '@/lib/format-count';
import { unitNode } from '@/components/ui/units';
import { SVG_TEXT, centreShift, lineAscentPx, wrapPhrases } from '@/lib/chart-type';
import { descentPx, textPx, tightLinePx } from '@/lib/type-metrics';

/**
 * The DAO page's picture (hierarchy v3, R13; board V63, DAO-11): CKB deposited on every complete
 * day, large, right under the deposited figure; its share of circulating CKB and the net change of
 * each whole week as two small multiples under it; one reading row for all three. Every name and
 * scale is written inside its plot — no axis column, no caption. Weeks end on the last complete day
 * and are named by the day they start; weeks beyond p95 × 1.25 are cut at the cap and marked, so
 * the 2019–2020 launch weeks do not flatten every later week.
 */

const DAY_MS = 86_400_000;
const TICK_GAP_PX = 56;
/** The plots' names and time ticks at the label size, their end values at the reading size (lib/chart-type.ts). */
const NAME_PX = SVG_TEXT.label;
const END_PX = SVG_TEXT.reading;
/*
 * The bands follow from the text set in them (readability plan 2026-11-18, lane F3): each line is
 * a tight line of its role, a band is its lines and its stated padding, and a plot grows by a line
 * where its name takes two.
 */
const LINE = tightLinePx(NAME_PX);
/** The band at the top of the main plot that holds its name, 5 px under the top and 10 px over the data. */
const mainNameBand = (lines: number) => 5 + lines * LINE + 10;
const MAIN_NAME_Y = 5 + lineAscentPx(NAME_PX);
/** R13: the page's main picture is at least 280 px tall: 270 px of data under its name band. */
const MAIN_DATA_H = 270;
/** The main plot's foot under a one-line name (300 px). */
export const MAIN_PLOT_H = mainNameBand(1) + MAIN_DATA_H;
/** Under the main plot, the year (or month / day) labels: 3 px, their line and 1 px: 19 px. */
const AXIS_GAP = 3;
const AXIS_H = AXIS_GAP + LINE + 1;
/**
 * A small multiple's top row: its name and its end value on one baseline, the row the value's
 * tight line (17 px at 14), 1 px over and under it: 19 px; a scale note that takes a line of its
 * own adds a label line under it.
 */
const ROW_LINE = tightLinePx(END_PX);
const ROW_Y = 1 + lineAscentPx(END_PX);
const multipleBand = (lines: number) => 1 + ROW_LINE + (lines - 1) * LINE + 1;
/**
 * The note's line under the row: its baseline's step from the row's, the row's descent and the
 * note line's ascent (15.6 px), to the hundredth of a pixel the attribute is written in.
 */
const NOTE_DY = Math.round((descentPx(END_PX) + lineAscentPx(NAME_PX)) * 100) / 100;
/** A small multiple's data: 54 px (board) under its band. */
export const MULTIPLE_DATA_H = 54;
/** A small multiple with a one-line band: 73 px (was 72, its band 18). */
export const MULTIPLE_H = multipleBand(1) + MULTIPLE_DATA_H;

export type HistoryRange = '1M' | '1Y' | 'All';
export const HISTORY_RANGES: readonly HistoryRange[] = ['1M', '1Y', 'All'];

interface Day {
  date: string;
  shannons: bigint;
  depositors: string;
  ratio: string;
}
interface Week {
  start: number;
  end: number;
  net: bigint;
}
/** The complete days of one range and the whole weeks among them. */
export interface HistorySeries {
  days: readonly Day[];
  weeks: readonly Week[];
}

function dayTime(date: string): number {
  const t = Date.parse(`${date}T00:00:00Z`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !Number.isFinite(t))
    throw new Error(`DAO history date ${JSON.stringify(date)} is not a calendar day`);
  return t;
}

/** Consecutive complete days with their share of circulation; a gap or a missing share throws. */
function joinDays(deposited: ChartResponse, ratio: ChartResponse): Day[] {
  const shares = new Map(ratio.data.map((p) => [p.date, p.value]));
  return deposited.data.map((row, i) => {
    if (i > 0 && dayTime(row.date) - dayTime(deposited.data[i - 1].date) !== DAY_MS)
      throw new Error(
        `DAO deposit history skips from ${deposited.data[i - 1].date} to ${row.date}`
      );
    const share = shares.get(row.date);
    if (share === undefined) throw new Error(`missing DAO circulation-ratio day ${row.date}`);
    if (row.value2 === undefined || row.value2 === null)
      throw new Error(`DAO total-deposit day ${row.date} has no current depositor count`);
    return {
      date: row.date,
      shannons: parseCkbDecimal(row.value),
      depositors: exactUnsignedInteger(row.value2, `DAO depositors on ${row.date}`).toLocaleString(
        'en-US'
      ),
      ratio: formatExactDecimal(share, 'DAO circulation ratio'),
    };
  });
}

/** Whole weeks ending on the last day, newest last; the first `start` days have no week. */
function wholeWeeks(days: readonly Day[]): Week[] {
  const weeks: Week[] = [];
  for (let end = days.length - 1; end >= 7; end -= 7)
    weeks.push({ start: end - 6, end, net: days[end].shannons - days[end - 7].shannons });
  return weeks.reverse();
}

/** The days of `range` (the last 30, the last 365, or all) and their whole weeks. */
export function historySeries(
  deposited: ChartResponse,
  ratio: ChartResponse,
  range: HistoryRange
): HistorySeries {
  const all = joinDays(deposited, ratio);
  const days = all.slice(-(range === '1M' ? 30 : range === '1Y' ? 365 : all.length));
  return { days, weeks: wholeWeeks(days) };
}

/** The weekly axis cap: p95 of |net| × 1.25, rounded up to a round number (CKB, geometry only). */
function weeklyCap(weeks: readonly Week[]): number {
  const mags = weeks.map((w) => Math.abs(Number(w.net) / 1e8)).sort((a, b) => a - b);
  const basis = mags[Math.floor(0.95 * mags.length)] || mags[mags.length - 1] || 0;
  return basis === 0 ? 1 : niceCeil(basis * 1.25);
}

/** A share as the API states it, cut (never rounded) to two decimals for a label at a line's end. */
export function cutShare(ratio: string): string {
  const m = /^([\d,]+)(?:\.(\d+))?$/.exec(ratio);
  if (!m) throw new Error(`DAO circulation ratio ${JSON.stringify(ratio)} is not a decimal`);
  return m[2] === undefined ? m[1] : `${m[1]}.${m[2].slice(0, 2)}`;
}

/** A reading's CKB, in its summary form (R5): its line names the unit; a cut figure is titled. */
function ReadingCkb({ shannons, signed = false }: { shannons: bigint; signed?: boolean }) {
  return (
    <CkbAmount
      form="summary"
      shannons={shannons}
      signed={signed}
      tone={signed ? 'sign' : 'inherit'}
      unit={false}
    />
  );
}

const textWidth = textPx;

/** Ticks on the time axis, thinned so labels keep `TICK_GAP_PX` apart. */
function timeTicks(days: readonly Day[], x: (i: number) => number) {
  const span = days.length;
  const all = days.flatMap((d, i) =>
    span > 400
      ? d.date.endsWith('-01-01')
        ? [{ i, label: d.date.slice(0, 4) }]
        : []
      : span > 40
        ? d.date.endsWith('-01')
          ? [{ i, label: d.date.slice(0, 7) }]
          : []
        : (days.length - 1 - i) % 7 === 0
          ? [{ i, label: d.date.slice(5) }]
          : []
  );
  const kept: typeof all = [];
  for (const t of all)
    if (!kept.length || x(t.i) - x(kept[kept.length - 1].i) >= TICK_GAP_PX) kept.push(t);
  return kept;
}

/**
 * The pointer's day over a plot `el` whose first day sits `left` px in (after its value gutter)
 * and whose last day sits at its right edge.
 */
function dayAt(clientX: number, el: Element, last: number, left: number): number {
  const r = el.getBoundingClientRect();
  const f = r.width > left ? (clientX - r.left - left) / (r.width - left) : 1;
  return Math.round(Math.min(1, Math.max(0, f)) * last);
}

/** Hover, tap (pin) and leave on one plot of the picture, shared by its three panels. */
function plotHandlers(pin: PinnedReadout<number>, last: number, left = 0) {
  return {
    onPointerMove: (e: PointerEvent<SVGSVGElement>) =>
      pin.hover(dayAt(e.clientX, e.currentTarget, last, left)),
    onPointerLeave: () => pin.hover(null),
    onClick: (e: MouseEvent<SVGSVGElement>) =>
      pin.toggle(dayAt(e.clientX, e.currentTarget, last, left)),
  };
}

/** The range buttons: one inline group, so the three sit on one line as one control. */
export function HistoryRangeToggle({
  range,
  onChange,
}: {
  range: HistoryRange;
  onChange: (range: HistoryRange) => void;
}) {
  return (
    <div role="group" aria-label="History range" className="whitespace-nowrap">
      {HISTORY_RANGES.map((choice, i) => (
        <button
          key={choice}
          type="button"
          aria-pressed={range === choice}
          onClick={() => onChange(choice)}
          className={cn(
            'text-small inline-flex h-7 items-center border px-2.5 font-mono',
            i > 0 && 'ml-1',
            range === choice
              ? 'border-base-border bg-base-border-subtle text-text-bright'
              : 'border-base-border-subtle text-text hover:text-text-bright'
          )}
        >
          {choice}
        </button>
      ))}
    </div>
  );
}

/**
 * The picture: the deposited area chart (≥ 280 px) with its name and round values inside the plot
 * and its time labels under it, then the share line and the weekly bars side by side (stacked
 * below sm), each named inside and carrying its latest value at its end.
 */
export function DaoHistoryFigure({
  series,
  pin,
}: {
  series: HistorySeries;
  pin: PinnedReadout<number>;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { days } = series;
  if (days.length < 2)
    return <p className="text-text-dim text-small mt-[18px]">Not enough complete days to draw.</p>;
  return (
    <div className="mt-[18px] min-w-0">
      <div ref={ref} className="min-w-0">
        {width !== null && <DepositedPlot series={series} pin={pin} width={width} />}
      </div>
      <div className="mt-2.5 grid min-w-0 grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-2">
        <SharePanel series={series} pin={pin} />
        <WeeklyPanel series={series} pin={pin} />
      </div>
    </div>
  );
}

function DepositedPlot({
  series,
  pin,
  width,
}: {
  series: HistorySeries;
  pin: PinnedReadout<number>;
  width: number;
}) {
  const { days } = series;
  const fillId = `dao-deposited-${useId().replace(/:/g, '')}`;
  const last = days.length - 1;
  const ckb = (v: bigint) => Number(v) / 1e8;
  const ticks = niceTicks(0, Math.max(...days.map((d) => ckb(d.shannons))), 4);
  const top = ticks.values[ticks.values.length - 1];
  // The values' gutter is measured before the band (it is as wide as the widest of the round
  // values the plot can write); the name, 8 px in from the plot's sides, takes one line where it
  // fits, else breaks after "CKB deposited" (390 px) and then between words (a 320 px window: its
  // date on a third line; review F-B, logic finding 1), and the plot grows a line at a time.
  const left = tickGutter(ticks.values.filter((v) => v > 0).map((v) => formatTick(v, ticks.step)));
  const nameLines = wrapPhrases(
    ['CKB deposited', `every complete day since ${days[0].date}`],
    NAME_PX,
    width - left - 16
  );
  const band = mainNameBand(nameLines.length);
  // the foot of a plot whose name takes one line, and a line lower where it takes two
  const bottom = MAIN_PLOT_H + (band - mainNameBand(1));
  const y = (v: number) => bottom - (v / top) * MAIN_DATA_H;
  // U-19: the round values sit in a gutter left of the plot, in the dim ink (as every chart's),
  // never inside it, where the line and its fill cross them; a value whose line would reach into
  // the name's band is not written.
  const shown = ticks.values.filter(
    (v) => v > 0 && y(v) + centreShift(SVG_TEXT.tick) - lineAscentPx(SVG_TEXT.tick) >= band
  );
  const x = (i: number) => left + (i / last) * (width - left);
  const path = days.map((d, i) => `${i ? 'L' : 'M'}${x(i)},${y(ckb(d.shannons))}`).join(' ');
  const active = pin.active !== null && pin.active <= last ? pin.active : null;
  return (
    <svg
      role="img"
      aria-label={`CKB deposited in the Nervos DAO on every complete day from ${days[0].date} to ${days[last].date}`}
      data-panel="deposited"
      width={width}
      height={bottom + AXIS_H}
      viewBox={`0 0 ${width} ${bottom + AXIS_H}`}
      className="block cursor-crosshair overflow-visible font-mono"
      {...plotHandlers(pin, last, left)}
    >
      {shown.map((v) => (
        <g key={v}>
          {/* Visual finish V13: the grid is solid; a dash means a reference. */}
          <line x1={left} x2={width} y1={y(v)} y2={y(v)} className="stroke-row-rule" />
          <text
            data-role="y-tick"
            x={left - 8}
            y={y(v) + centreShift(SVG_TEXT.tick)}
            textAnchor="end"
            fontSize={SVG_TEXT.tick}
            className="fill-text-dim"
          >
            {formatTick(v, ticks.step)}
          </text>
        </g>
      ))}
      {/* Visual finish V13: the gold is light, not paint — .28 at the top of the fill fading to 0
          at the baseline (the DAO gold the line's stroke-gold is). */}
      <defs>
        <linearGradient id={fillId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={CLASS_COLORS.dao} stopOpacity="0.28" />
          <stop offset="1" stopColor={CLASS_COLORS.dao} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={`${path} L${x(last)},${bottom} L${x(0)},${bottom} Z`} fill={`url(#${fillId})`} />
      <path d={path} fill="none" className="stroke-gold" strokeWidth={1.5} />
      <line x1={left} x2={width} y1={bottom} y2={bottom} className="stroke-base-border" />
      <text
        data-role="chart-name"
        x={left + 8}
        y={MAIN_NAME_Y}
        fontSize={NAME_PX}
        className="fill-text-dim"
      >
        {nameLines[0]}
        {nameLines.slice(1).map((line, i) => (
          <tspan key={`${i}:${line}`} x={left + 8} dy={LINE}>
            {line}
          </tspan>
        ))}
      </text>
      {timeTicks(days, x).map((t) => {
        // A label that would run past the plot's right edge ends at its tick instead.
        const atEnd = x(t.i) + 3 + textWidth(t.label.length, NAME_PX) > width;
        return (
          <g key={t.i}>
            <line
              x1={x(t.i)}
              x2={x(t.i)}
              y1={bottom}
              y2={bottom + 4}
              className="stroke-text-ghost"
            />
            <text
              data-role="time-tick"
              x={atEnd ? x(t.i) - 3 : x(t.i) + 3}
              y={bottom + AXIS_GAP + lineAscentPx(NAME_PX)}
              textAnchor={atEnd ? 'end' : undefined}
              fontSize={NAME_PX}
              className="fill-text-dim"
            >
              {t.label}
            </text>
          </g>
        );
      })}
      {active !== null && (
        <line
          data-cursor
          x1={x(active)}
          x2={x(active)}
          y1={band}
          y2={bottom}
          className="stroke-text-bright"
          strokeOpacity={0.45}
        />
      )}
    </svg>
  );
}

/** A label at the top-right end of a small multiple, haloed in the page colour over the plot. */
function EndLabel({
  width,
  className,
  title,
  children,
}: {
  width: number;
  className: string;
  /** The exact value a shortened label stands for (R5), as the label's hover title. */
  title?: string;
  children: ReactNode;
}) {
  const label = (
    <text
      data-role="end-label"
      x={width}
      y={ROW_Y}
      textAnchor="end"
      fontSize={END_PX}
      className={cn('stroke-base-bg', className)}
      strokeWidth={4}
      paintOrder="stroke"
    >
      {children}
    </text>
  );
  if (title === undefined) return label;
  return (
    <g>
      <title>{title}</title>
      {label}
    </g>
  );
}

function SharePanel({ series, pin }: { series: HistorySeries; pin: PinnedReadout<number> }) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { days } = series;
  const last = days.length - 1;
  const shares = days.map((d) => Number(d.ratio.replace(/,/g, '')));
  const ticks = niceTicks(0, Math.max(...shares), 3);
  const top = ticks.values[ticks.values.length - 1];
  const active = pin.active !== null && pin.active <= last ? pin.active : null;
  return (
    <div ref={ref} className="min-w-0">
      {width !== null &&
        (() => {
          const x = (i: number) => (i / last) * width;
          const band = multipleBand(1);
          const height = MULTIPLE_H;
          const y = (v: number) => height - 2 - (v / top) * (MULTIPLE_DATA_H - 2);
          const path = days.map((d, i) => `${i ? 'L' : 'M'}${x(i)},${y(shares[i])}`).join(' ');
          return (
            <svg
              role="img"
              aria-label={`Share of circulating CKB locked in the Nervos DAO on every complete day from ${days[0].date} to ${days[last].date}`}
              data-panel="share"
              width={width}
              height={height}
              viewBox={`0 0 ${width} ${height}`}
              className="block cursor-crosshair overflow-visible font-mono"
              {...plotHandlers(pin, last)}
            >
              {/* The half-scale guide is grid, not a reference: solid (visual finish V13). */}
              <line x1={0} x2={width} y1={y(top / 2)} y2={y(top / 2)} className="stroke-row-rule" />
              <path d={path} fill="none" className="stroke-text" strokeWidth={1.25} />
              <text
                data-role="chart-name"
                x={8}
                y={ROW_Y}
                fontSize={NAME_PX}
                className="fill-text-dim"
              >
                share of circulating CKB
              </text>
              <EndLabel width={width} className="fill-text-bright">
                {cutShare(days[last].ratio)}%
              </EndLabel>
              {active !== null && (
                <line
                  data-cursor
                  x1={x(active)}
                  x2={x(active)}
                  y1={band}
                  y2={height}
                  className="stroke-text-bright"
                  strokeOpacity={0.45}
                />
              )}
            </svg>
          );
        })()}
    </div>
  );
}

function WeeklyPanel({ series, pin }: { series: HistorySeries; pin: PinnedReadout<number> }) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { days, weeks } = series;
  const last = days.length - 1;
  const active = pin.active !== null && pin.active <= last ? pin.active : null;
  const cap = weeks.length ? weeklyCap(weeks) : 1;
  const ckb = (v: bigint) => Number(v) / 1e8;
  const clipped = weeks.filter((w) => Math.abs(ckb(w.net)) > cap).length;
  const note = `±${formatTick(cap, cap)} scale${clipped === 0 ? '' : `, ${formatCount(clipped, 'week')} cut`}`;
  const latest = weeks.at(-1) ?? null;
  // The label at the plot's end: the week's net in its summary form (R5), one ink with its sign.
  const end = latest ? ckbAmountText(latest.net, 'summary', true) : null;
  const biggest = weeks.reduce<Week | null>(
    (best, w) =>
      !best || (w.net < 0 ? -w.net : w.net) > (best.net < 0 ? -best.net : best.net) ? w : best,
    null
  );
  return (
    <div ref={ref} className="min-w-0">
      {width !== null &&
        (() => {
          const name = 'net change per week';
          const endChars = end ? end.length : 0;
          // The scale note follows the name on its line when both fit beside the end label.
          const inline =
            textWidth(name.length + 3 + note.length, NAME_PX) + textWidth(endChars, END_PX) + 24 <=
            width;
          const band = multipleBand(inline ? 1 : 2);
          const height = band + MULTIPLE_DATA_H;
          const x = (i: number) => (i / last) * width;
          const mid = band + MULTIPLE_DATA_H / 2;
          const half = MULTIPLE_DATA_H / 2 - 1;
          // A week's bar spans its first to its last day (six day steps), 1 px apart.
          const bw = Math.max(1.2, (6 / last) * width - 1);
          return (
            <svg
              role="img"
              aria-label={`Net change in deposited CKB in each whole week from ${days[0].date} to ${days[last].date}${biggest ? `; the largest was ${formatSignedCkbInteger(biggest.net)} CKB in the week of ${days[biggest.start].date}` : ''}`}
              data-panel="weekly"
              width={width}
              height={height}
              viewBox={`0 0 ${width} ${height}`}
              className="block cursor-crosshair overflow-visible font-mono"
              {...plotHandlers(pin, last)}
            >
              <line x1={0} x2={width} y1={mid} y2={mid} className="stroke-base-border" />
              {weeks.map((w) => {
                const v = ckb(w.net);
                const over = Math.abs(v) > cap;
                const h = (Math.min(Math.abs(v), cap) / cap) * half;
                const bx = x(w.start);
                return (
                  <g key={w.start}>
                    <rect
                      data-week={days[w.start].date}
                      data-clipped={over ? 'true' : undefined}
                      x={bx}
                      y={v > 0 ? mid - h : mid}
                      width={bw}
                      height={Math.max(0.6, h)}
                      className={v < 0 ? 'fill-negative' : 'fill-positive'}
                      fillOpacity={0.75}
                    />
                    {over && (
                      <path
                        data-clip-mark
                        d={`M${bx - 1} ${(v > 0 ? mid - half + 2 : mid + half - 6) + 3}l${bw + 2} -3M${bx - 1} ${(v > 0 ? mid - half + 2 : mid + half - 6) + 6}l${bw + 2} -3`}
                        className="stroke-base-bg"
                        strokeWidth={1.2}
                      />
                    )}
                  </g>
                );
              })}
              <text
                data-role="chart-name"
                x={8}
                y={ROW_Y}
                fontSize={NAME_PX}
                className="fill-text-dim"
              >
                {name}
                {inline ? (
                  <tspan data-role="scale-note">{`\u00a0·\u00a0${note}`}</tspan>
                ) : (
                  <tspan data-role="scale-note" x={8} dy={NOTE_DY}>
                    {note}
                  </tspan>
                )}
              </text>
              {end && latest && (
                <EndLabel
                  width={width}
                  className={
                    latest.net > BigInt(0)
                      ? 'fill-positive'
                      : latest.net < BigInt(0)
                        ? 'fill-negative'
                        : 'fill-text'
                  }
                  title={ckbShortenedTitle(latest.net, 'summary', true)}
                >
                  {end}
                </EndLabel>
              )}
              {active !== null && (
                <line
                  data-cursor
                  x1={x(active)}
                  x2={x(active)}
                  y1={band}
                  y2={height}
                  className="stroke-text-bright"
                  strokeOpacity={0.45}
                />
              )}
            </svg>
          );
        })()}
    </div>
  );
}

/**
 * The one reading of the picture (board v3): the hovered or tapped day — else the last complete
 * day — deposited, its share, its depositors, the whole week it falls in, and the all-time peak.
 * Its amounts are in the summary form (R5), a cut one titled with its exact value, as the
 * line end's label is.
 */
export function DaoHistoryReading({
  series,
  pin,
  peak,
}: {
  series: HistorySeries;
  pin: PinnedReadout<number>;
  peak: { date: string; value: string } | null;
}) {
  const pinWord = usePinWord();
  const { days, weeks } = series;
  const last = days.length - 1;
  if (last < 0) return null;
  const index = pin.active === null || pin.active > last ? last : pin.active;
  const day = days[index];
  const week = weeks.find((w) => w.start <= index && index <= w.end);
  return (
    // ReadoutRow's own "selected " prefix would stand on a line of its own before this row of
    // items, so the row opens with it itself (same word, same ink: `usePinWord`, C11).
    <ReadoutRow label="History reading" pinned={false}>
      {/* One row of facts, each its own item: a narrower column wraps between them. */}
      <span className="text-text-dim flex flex-wrap gap-x-[0.6em]">
        {pin.showingPin && <span>{pinWord} </span>}
        <span className="text-text-bright">{unitNode(day.date)}</span>
        <span>
          {' '}
          · deposited{' '}
          <span className="text-text">
            <ReadingCkb shannons={day.shannons} />
          </span>
        </span>
        <span>
          {' '}
          · share <span className="text-text">{day.ratio}%</span>
        </span>
        <span>
          {' '}
          · depositors <span className="text-text">{day.depositors}</span>
        </span>
        {week && (
          <span>
            {' '}
            · week from <span className="whitespace-nowrap">{days[week.start].date}</span>{' '}
            <span
              data-role="week-net"
              className={
                week.net > BigInt(0)
                  ? 'text-positive'
                  : week.net < BigInt(0)
                    ? 'text-negative'
                    : 'text-text'
              }
            >
              <ReadingCkb shannons={week.net} signed />
            </span>
          </span>
        )}
        {peak && (
          <span>
            {' '}
            · peak{' '}
            <span className="text-text">
              <ReadingCkb shannons={parseCkbDecimal(peak.value)} />
            </span>{' '}
            on <span className="whitespace-nowrap">{peak.date}</span>
          </span>
        )}
      </span>
    </ReadoutRow>
  );
}
