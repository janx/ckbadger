'use client';

import { useState, type ReactNode } from 'react';
import type { ChartResponse } from '@/lib/api';
import { useElementWidth } from '@/hooks/useElementWidth';
import { formatSignedCkbInteger, parseCkbDecimal, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatTick, niceCeil, niceTicks } from '@/lib/chart-ticks';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';

/**
 * History (board V63, DAO-11): one figure with three panels on one time axis — CKB deposited, its
 * share of circulating CKB, and the net change of each whole week — read by one hover reading.
 * Weeks end on the last complete day and are named by the day they start; weeks beyond p95 × 1.25
 * are cut at the cap and marked, so the 2019–2020 launch weeks do not flatten every later week.
 */

const DAY_MS = 86_400_000;
const LEFT = 64;
const RIGHT = 8;
const A_Y = 16;
const A_H = 150;
const B_Y = A_Y + A_H + 44;
const B_H = 70;
const C_Y = B_Y + B_H + 44;
const C_H = 90;
const AXIS_Y = C_Y + C_H + 20;
const HEIGHT = AXIS_Y + 8;
const TICK_GAP_PX = 56;

type Range = '1M' | '1Y' | 'All';

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

/** The weekly axis cap: p95 of |net| × 1.25, rounded up to a round number (CKB, geometry only). */
function weeklyCap(weeks: readonly Week[]): number {
  const mags = weeks.map((w) => Math.abs(Number(w.net) / 1e8)).sort((a, b) => a - b);
  const basis = mags[Math.floor(0.95 * mags.length)] || mags[mags.length - 1] || 0;
  return basis === 0 ? 1 : niceCeil(basis * 1.25);
}

function ExactCkbText({ shannons, signed = false }: { shannons: bigint; signed?: boolean }) {
  const parts = splitCkbAmount(shannons);
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {signed ? parts.sign : null}
      {parts.integer}
      <span className="opacity-70">{parts.fraction}</span>
      <span className="text-text-ghost">{parts.ghost}</span>
    </span>
  );
}

/** Ticks on the time axis, thinned so labels keep `TICK_GAP_PX` apart. */
function timeTicks(days: readonly Day[], range: Range, x: (i: number) => number) {
  const all = days.flatMap((d, i) =>
    range === 'All'
      ? d.date.endsWith('-01-01')
        ? [{ i, label: d.date.slice(0, 4) }]
        : []
      : range === '1Y'
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

export function DaoHistory({
  deposited,
  ratio,
}: {
  deposited: ChartResponse;
  ratio: ChartResponse;
}) {
  const [range, setRange] = useState<Range>('All');
  const [hover, setHover] = useState<number | null>(null);
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const allDays = joinDays(deposited, ratio);
  const count = range === '1M' ? 30 : range === '1Y' ? 365 : allDays.length;
  const days = allDays.slice(-count);
  const weeks = wholeWeeks(days);
  const last = days.length - 1;
  const index = hover === null || hover > last ? last : hover;
  const day = days[index];
  const week = weeks.find((w) => w.start <= index && index <= w.end);

  const reading = day ? (
    <div
      aria-live="polite"
      aria-label="History reading"
      className="border-base-border-subtle flex min-h-10 flex-wrap items-baseline gap-x-[26px] gap-y-1.5 border-b pb-2.5 pt-2 text-[12px]"
    >
      <span className="text-text-bright text-[13px]">
        {day.date}
        {index === last && <span className="text-text-dim text-[12px]"> · last full day</span>}
      </span>
      <span className="flex items-baseline gap-1.5">
        <span aria-hidden="true" className="bg-gold inline-block h-2 w-2 rounded-sm" />
        <span className="text-text-dim">deposited</span>
        <span className="text-text-bright text-[13px]">
          <ExactCkbText shannons={day.shannons} />
        </span>
      </span>
      <span className="flex items-baseline gap-1.5">
        <span className="text-text-dim">share of circulation</span>
        <span className="text-text-bright">{day.ratio}%</span>
      </span>
      {week && (
        <span className="flex items-baseline gap-1.5">
          <span className="text-text-dim">net change, week from {days[week.start].date}</span>
          <span
            className={
              week.net > BigInt(0)
                ? 'text-positive'
                : week.net < BigInt(0)
                  ? 'text-negative'
                  : 'text-text'
            }
          >
            <ExactCkbText shannons={week.net} signed /> CKB
          </span>
        </span>
      )}
      <span className="flex items-baseline gap-1.5">
        <span className="text-text-dim">depositors</span>
        <span className="text-text">{day.depositors}</span>
      </span>
    </div>
  ) : (
    <p className="text-text-dim text-xs">No complete daily history yet.</p>
  );

  let figure: ReactNode = null;
  if (width !== null && days.length > 1) {
    const plotW = width - LEFT - RIGHT;
    const x = (i: number) => LEFT + (i / last) * plotW;
    const ckb = (v: bigint) => Number(v) / 1e8;
    const aMax = Math.max(...days.map((d) => ckb(d.shannons)));
    const aTicks = niceTicks(0, aMax, 4);
    const aTop = aTicks.values[aTicks.values.length - 1];
    const ay = (v: number) => A_Y + A_H - (v / aTop) * A_H;
    const shares = days.map((d) => Number(d.ratio.replace(/,/g, '')));
    const bTicks = niceTicks(0, Math.max(...shares), 3);
    const bTop = bTicks.values[bTicks.values.length - 1];
    const by = (v: number) => B_Y + B_H - (v / bTop) * B_H;
    const cap = weeks.length ? weeklyCap(weeks) : 1;
    const mid = C_Y + C_H / 2;
    const bw = Math.max(1.2, (7 / last) * plotW - 1);
    const clipped = weeks.filter((w) => Math.abs(ckb(w.net)) > cap);
    const biggest = weeks.reduce<Week | null>(
      (best, w) =>
        !best || (w.net < 0 ? -w.net : w.net) > (best.net < 0 ? -best.net : best.net) ? w : best,
      null
    );
    const path = days.map((d, i) => `${i ? 'L' : 'M'}${x(i)},${ay(ckb(d.shannons))}`).join(' ');
    const sharePath = days.map((d, i) => `${i ? 'L' : 'M'}${x(i)},${by(shares[i])}`).join(' ');
    const ticks = timeTicks(days, range, x);
    // Monospace advance is 0.6 em: what fits is decided by character counts, before drawing.
    const TITLE = 'Net change, per week';
    const clipNote =
      clipped.length === 0
        ? null
        : clipped.length === 1
          ? '1 week beyond the scale is cut and marked'
          : `${clipped.length} weeks beyond the scale are cut and marked`;
    const noteInline = clipNote !== null && (TITLE.length + 3 + clipNote.length) * 6.6 <= plotW;
    const largest = biggest
      ? `${formatSignedCkbInteger(biggest.net)} · week of ${days[biggest.start].date}`
      : null;
    // The largest week's label: right of its bar, else left of it, else ending at the plot's edge.
    const largestPlace = (() => {
      if (!biggest || !largest) return null;
      const w = largest.length * 6.3;
      const bx = x(biggest.start);
      if (bx + bw + 6 + w <= LEFT + plotW) return { x: bx + bw + 6, anchor: 'start' as const };
      if (bx - 6 - w >= LEFT) return { x: bx - 6, anchor: 'end' as const };
      return { x: LEFT + plotW, anchor: 'end' as const };
    })();
    figure = (
      <div className="relative">
        <svg
          role="img"
          aria-label={`Nervos DAO from ${days[0].date} to ${days[last].date}: CKB deposited, its share of circulating CKB, and the net change each week`}
          width={width}
          height={HEIGHT}
          viewBox={`0 0 ${width} ${HEIGHT}`}
          className="block overflow-visible font-mono"
        >
          <g data-panel="deposited">
            {aTicks.values.map((v) => (
              <g key={v}>
                <line
                  x1={LEFT}
                  x2={LEFT + plotW}
                  y1={ay(v)}
                  y2={ay(v)}
                  className="stroke-base-elevated"
                />
                <text
                  x={LEFT - 8}
                  y={ay(v) + 4}
                  textAnchor="end"
                  fontSize="11"
                  className="fill-text-dim"
                >
                  {formatTick(v, aTicks.step)}
                </text>
              </g>
            ))}
            <path
              d={`${path} L${x(last)},${A_Y + A_H} L${x(0)},${A_Y + A_H} Z`}
              className="fill-gold"
              fillOpacity={0.22}
            />
            <path d={path} fill="none" className="stroke-gold" strokeWidth={1.3} />
            <text x={LEFT} y={A_Y - 4} fontSize="11" className="fill-text">
              CKB deposited
            </text>
          </g>
          <g data-panel="share">
            {bTicks.values.map((v) => (
              <g key={v}>
                <line
                  x1={LEFT}
                  x2={LEFT + plotW}
                  y1={by(v)}
                  y2={by(v)}
                  className="stroke-base-elevated"
                />
                <text
                  x={LEFT - 8}
                  y={by(v) + 4}
                  textAnchor="end"
                  fontSize="11"
                  className="fill-text-dim"
                >
                  {formatTick(v, bTicks.step)}%
                </text>
              </g>
            ))}
            <path d={sharePath} fill="none" className="stroke-text" strokeWidth={1.2} />
            <text x={LEFT} y={B_Y - 6} fontSize="11" className="fill-text">
              Share of circulating CKB
            </text>
          </g>
          <g data-panel="weekly">
            <line x1={LEFT} x2={LEFT + plotW} y1={mid} y2={mid} className="stroke-base-border" />
            <text x={LEFT - 8} y={C_Y + 4} textAnchor="end" fontSize="11" className="fill-text-dim">
              +{formatTick(cap, cap)}
            </text>
            <text
              x={LEFT - 8}
              y={C_Y + C_H + 4}
              textAnchor="end"
              fontSize="11"
              className="fill-text-dim"
            >
              −{formatTick(cap, cap)}
            </text>
            {weeks.map((w) => {
              const v = ckb(w.net);
              const over = Math.abs(v) > cap;
              const h = (Math.min(Math.abs(v), cap) / cap) * (C_H / 2);
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
                      d={`M${bx - 1} ${(v > 0 ? C_Y + 2 : C_Y + C_H - 6) + 3}l${bw + 2} -3M${bx - 1} ${(v > 0 ? C_Y + 2 : C_Y + C_H - 6) + 6}l${bw + 2} -3`}
                      className="stroke-base-bg"
                      strokeWidth={1.2}
                    />
                  )}
                </g>
              );
            })}
            {biggest && largest && largestPlace && (
              <text
                data-role="largest-week"
                x={largestPlace.x}
                y={biggest.net > 0 ? C_Y + 10 : C_Y + C_H - 2}
                textAnchor={largestPlace.anchor}
                fontSize="10.5"
                className="fill-text"
              >
                {largest}
              </text>
            )}
            <text
              x={LEFT}
              y={clipNote !== null && !noteInline ? C_Y - 20 : C_Y - 6}
              fontSize="11"
              className="fill-text"
            >
              {TITLE}
              {clipNote !== null && noteInline && (
                <tspan data-role="clip-note" className="fill-text-dim">
                  {`\u00a0·\u00a0${clipNote}`}
                </tspan>
              )}
            </text>
            {clipNote !== null && !noteInline && (
              <text
                data-role="clip-note"
                x={LEFT}
                y={C_Y - 6}
                fontSize="11"
                className="fill-text-dim"
              >
                {clipNote}
              </text>
            )}
          </g>
          {ticks.map((t) => (
            <g key={t.i}>
              <line
                x1={x(t.i)}
                x2={x(t.i)}
                y1={C_Y + C_H}
                y2={C_Y + C_H + 4}
                className="stroke-text-ghost"
              />
              <text x={x(t.i) + 3} y={AXIS_Y + 4} fontSize="11" className="fill-text-dim">
                {t.label}
              </text>
            </g>
          ))}
          {hover !== null && (
            <line
              data-cursor
              x1={x(index)}
              x2={x(index)}
              y1={A_Y}
              y2={C_Y + C_H}
              className="stroke-text-bright"
              strokeOpacity={0.45}
            />
          )}
        </svg>
        <div
          aria-hidden="true"
          data-hover-overlay
          className="absolute bottom-0 top-0 cursor-crosshair"
          style={{ left: LEFT, width: plotW }}
          onPointerMove={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
            setHover(Math.round(f * last));
          }}
          onPointerLeave={() => setHover(null)}
        />
      </div>
    );
  }

  return (
    <div>
      <div className="mb-1 flex flex-wrap items-center justify-between gap-4">
        <h2 className="text-text-bright text-[15px] font-medium">History</h2>
        <div role="group" aria-label="History range" className="flex gap-1">
          {(['1M', '1Y', 'All'] as const).map((choice) => (
            <button
              key={choice}
              type="button"
              aria-pressed={range === choice}
              onClick={() => {
                setRange(choice);
                setHover(null);
              }}
              className={`h-7 rounded-md border px-2.5 font-mono text-[12px] ${range === choice ? 'border-base-border bg-base-border-subtle text-text-bright' : 'border-base-border-subtle text-text hover:text-text-bright'}`}
            >
              {choice}
            </button>
          ))}
        </div>
      </div>
      {reading}
      <div ref={ref} className="mt-2 min-w-0">
        {figure}
      </div>
    </div>
  );
}
