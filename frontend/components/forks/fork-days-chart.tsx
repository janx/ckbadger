'use client';

import { useEffect } from 'react';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout, type PinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';
import { percentOf } from '@/lib/format-unit';
import { UNCLE_RATE_TARGET } from '@/lib/fork-format';
import { cutScaledInteger } from '@/lib/exact-integer';
import { SVG_TEXT, centreShift, lineAscentPx } from '@/lib/chart-type';
import { advancePx, descentPx, tightLinePx } from '@/lib/type-metrics';

interface Day {
  date: string;
  canonicalBlocks: number;
  uncles: number;
  nodeSwitches: number;
}

/** The board's drawing width, used until the box is measured. */
const DESIGN_WIDTH = 1210;
/*
 * The plot's words are set at the label size, each a tight line (lib/chart-type.ts), and its bands
 * follow from them (readability plan 2026-11-18, lane F3).
 */
const LINE = tightLinePx(SVG_TEXT.label);
/** The band at the top that holds the key: 4 px, its line and 9 px over the plot: 28 px. */
const NAME_PAD = 4;
const NAME_GAP = 9;
const TOP = NAME_PAD + LINE + NAME_GAP;
const KEY_BASELINE = NAME_PAD + lineAscentPx(SVG_TEXT.label);
/** A key's 10 px swatch, centred where the key's digits centre. */
const SWATCH = 10;
/** The rates' bars: 200 px under the key. */
const RATE_H = 200;
/**
 * Under the rates (lane P4), this node's branch switches in a panel of their own: 10 px under the
 * rates' baseline, the panel's name on its line and 4 px, then its 46 px of bars on their own scale.
 */
const PANEL_GAP = 10;
const PANEL_NAME_GAP = 4;
const SWITCHES_H = 46;
/** Under the switches: the first and last day's labels, 5 px under the axis and 2 px over the foot: 22 px. */
const AXIS_GAP = 5;
const AXIS = AXIS_GAP + LINE + 2;
/** A value stands a pixel over its gridline; the switches' one value a pixel under its own. */
const tickBaseline = (gridline: number) => gridline - 1 - descentPx(SVG_TEXT.label);
const tickUnder = (gridline: number) => gridline + 1 + lineAscentPx(SVG_TEXT.label);
/** A character of the plot's text, all of it set at the label size: 7.2 px at 12 px. */
export const CHAR = advancePx(SVG_TEXT.label);
const RATE_KEY = 'uncle rate, per day';
const SWITCHES_NAME = 'this node’s branch switches, per day';
/** The target the difficulty aims the uncle rate at, one block in 40: 2.5 %, for the geometry. */
const TARGET_PERCENT = 100 / 40;
// the words say the same target, one in forty cut at two places (lane exactness, part A, A8: the
// check wrote the geometry's float with toFixed)
if (`${cutScaledInteger(BigInt(100), BigInt(40), 2)}%` !== UNCLE_RATE_TARGET)
  throw new Error(`forks chart: one block in 40 is not ${UNCLE_RATE_TARGET}`);
const TARGET_WORDS = `target ${UNCLE_RATE_TARGET}`;

/** The smallest 1-2-5 step that covers `max` in at most four gridlines. */
function scale(max: number): { cap: number; step: number } {
  if (max <= 0) return { cap: 1, step: 1 };
  for (let magnitude = 1; ; magnitude *= 10)
    for (const unit of [1, 2, 5]) {
      const step = unit * magnitude;
      if (Math.ceil(max / step) <= 4) return { cap: Math.ceil(max / step) * step, step };
    }
}

/**
 * A day's uncle rate in percent, for the geometry: uncles ÷ the day's canonical blocks; none on a
 * day the chain made no block (a stalled test chain), and an uncle on such a day is refused.
 */
function ratePercent(day: Day): number | null {
  if (day.canonicalBlocks === 0) {
    if (day.uncles !== 0)
      throw new Error(`Fork days: ${day.date} has ${day.uncles} uncles and no canonical block`);
    return null;
  }
  return (100 * day.uncles) / day.canonicalBlocks;
}

/**
 * A day as the reading writes it: its rate (one percentage, `percentOf`, from the day's own exact
 * counts) and the counts it is computed from, then this node's switches that day.
 */
const reading = (day: Day) =>
  `${day.date} · ${
    day.canonicalBlocks === 0
      ? 'no canonical block'
      : `uncle rate ${percentOf(BigInt(day.uncles), BigInt(day.canonicalBlocks))} · ${formatCount(day.uncles, 'uncle')} in ${formatCount(day.canonicalBlocks, 'block')}`
  } · this node switched branches ${formatCount(day.nodeSwitches, 'time')}`;

/** The hover / tap state of the days, by date, held by the page so the reading can sit under the hero. */
export function useForkDaysPin(): PinnedReadout<string> {
  return usePinnedReadout<string>();
}

/** The active day's index, or null; a pinned day that left the window is let go. */
function activeDay(days: readonly Day[], pin: PinnedReadout<string>): number | null {
  const found = pin.active === null ? -1 : days.findIndex((d) => d.date === pin.active);
  return found < 0 ? null : found;
}

/**
 * The Forks page's picture (hierarchy v3, R13; lane P4: the rate against its target): each day's
 * uncle rate — the uncles the chain recorded that day over its canonical blocks — as a grey bar,
 * against a dashed line at the 2.50 % target the difficulty aims at (a dash is a reference, V13),
 * and under it, in a panel of its own on its own scale, this node's branch switches that day in
 * jade (they were a sliver of a few pixels at the foot of the uncles' bars). ≥ 280 px, its key
 * inside the plot at the top left, the values inside in the dim ink, the first and last day under
 * it. Hover, the arrow keys or a tap choose a day for `ForkDaysReading` (R6).
 */
export function ForkDaysPlot({ days, pin }: { days: readonly Day[]; pin: PinnedReadout<string> }) {
  // Days are named by date, so a refetched window keeps the same day (or lets a gone day go).
  const active = activeDay(days, pin);
  const gone = pin.active !== null && active === null;
  const { clear } = pin;
  useEffect(() => {
    if (gone) clear();
  }, [gone, clear]);
  // Drawn 1:1 in its box, so the 12 px text stays 12 px at phone width (visual pass, 390).
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const W = measured ?? DESIGN_WIDTH;
  const rates = days.map(ratePercent);
  const { cap, step } = scale(
    Math.max(TARGET_PERCENT, ...rates.filter((r): r is number => r !== null))
  );
  const ticks = Array.from({ length: Math.round(cap / step) }, (_, i) => (i + 1) * step);
  const tickText = (v: number) => `${v.toLocaleString('en-US')}%`;
  // the switches' own scale: the panel's height is the window's most switches in a day, its one
  // value written at its top, so the busiest day fills it and every other stands to it
  const switches = { cap: Math.max(1, ...days.map((d) => d.nodeSwitches)) };
  // The values sit in a lane at the plot's left, inside its gridlines; the bars start after it.
  const lane =
    Math.max(...ticks.map((v) => tickText(v).length), switches.cap.toLocaleString('en-US').length) *
      CHAR +
    8;
  const bw = (W - lane) / Math.max(days.length, 1);
  const base = TOP + RATE_H;
  const y = (v: number) => TOP + RATE_H - (v / cap) * RATE_H;
  // A value whose label would reach into the key's band is not written (as the DAO charts).
  const written = (v: number) => tickBaseline(y(v)) - lineAscentPx(SVG_TEXT.label) >= TOP;
  const panelName = base + PANEL_GAP + lineAscentPx(SVG_TEXT.label);
  const panelTop = base + PANEL_GAP + LINE + PANEL_NAME_GAP;
  const panelBase = panelTop + SWITCHES_H;
  const sy = (v: number) => panelBase - (v / switches.cap) * SWITCHES_H;
  const H = panelBase + AXIS;
  const target = y(TARGET_PERCENT);
  return (
    <div ref={box} className="min-w-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`Each day's uncle rate against the ${UNCLE_RATE_TARGET} target, and this node's branch switches; ← → step through the days`}
        tabIndex={0}
        className="block h-auto w-full"
        onKeyDown={(event) => {
          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
          event.preventDefault();
          const last = days.length - 1;
          const next =
            active === null
              ? last
              : Math.min(last, Math.max(0, active + (event.key === 'ArrowRight' ? 1 : -1)));
          pin.hover(days[next].date);
        }}
        onMouseLeave={() => pin.hover(null)}
        onBlur={() => pin.hover(null)}
      >
        <g data-role="chart-name">
          <rect
            x="0"
            y={KEY_BASELINE - centreShift(SVG_TEXT.label) - SWATCH / 2}
            width={SWATCH}
            height={SWATCH}
            fill={FORK_COLORS.uncles}
            opacity=".5"
          />
          <text x="14" y={KEY_BASELINE} fontSize={SVG_TEXT.label} fill={FORK_COLORS.tick}>
            {RATE_KEY}
          </text>
        </g>
        {ticks.map((v) => (
          <g key={v}>
            {/* The scale's lines are grid: solid (visual finish V13 b). */}
            <line data-role="grid" x1="0" x2={W} y1={y(v)} y2={y(v)} stroke={FORK_COLORS.grid} />
            {written(v) && (
              <text
                data-role="y-tick"
                x="0"
                y={tickBaseline(y(v))}
                fontSize={SVG_TEXT.label}
                fill={FORK_COLORS.tick}
              >
                {tickText(v)}
              </text>
            )}
          </g>
        ))}
        <line x1="0" x2={W} y1={base} y2={base} stroke={FORK_COLORS.axis} />
        <text
          data-role="switches-name"
          x="0"
          y={panelName}
          fontSize={SVG_TEXT.label}
          fill={FORK_COLORS.tick}
        >
          {SWITCHES_NAME}
        </text>
        <g>
          <line
            data-role="grid"
            x1="0"
            x2={W}
            y1={panelTop}
            y2={panelTop}
            stroke={FORK_COLORS.grid}
          />
          <text
            data-role="switches-tick"
            x="0"
            y={tickUnder(panelTop)}
            fontSize={SVG_TEXT.label}
            fill={FORK_COLORS.tick}
          >
            {switches.cap.toLocaleString('en-US')}
          </text>
        </g>
        <line x1="0" x2={W} y1={panelBase} y2={panelBase} stroke={FORK_COLORS.axis} />
        {days.map((day, i) => {
          const gap = Math.min(6, bw * 0.3);
          const x = lane + i * bw + gap / 2;
          const width = Math.max(bw - gap, 1);
          const first = i === 0;
          const last = i === days.length - 1;
          return (
            <g
              key={day.date}
              data-day={day.date}
              onMouseEnter={() => pin.hover(day.date)}
              onClick={() => pin.toggle(day.date)}
              className="cursor-pointer"
              opacity={active === null || active === i ? 1 : 0.55}
            >
              <title>{reading(day)}</title>
              <rect
                x={lane + i * bw}
                y={TOP}
                width={bw}
                height={panelBase - TOP}
                fill="transparent"
              />
              {rates[i] !== null && (
                <rect
                  data-rate
                  x={x}
                  y={y(rates[i])}
                  width={width}
                  height={base - y(rates[i])}
                  fill={FORK_COLORS.uncles}
                  fillOpacity=".35"
                />
              )}
              {day.nodeSwitches > 0 && (
                <rect
                  data-switches
                  x={x}
                  y={sy(day.nodeSwitches)}
                  width={width}
                  height={panelBase - sy(day.nodeSwitches)}
                  fill={FORK_COLORS.nodeSwitches}
                />
              )}
              {/* R14: the first and the last day only. */}
              {(first || last) && (
                <text
                  x={last ? x + width : x}
                  y={panelBase + AXIS_GAP + lineAscentPx(SVG_TEXT.label)}
                  textAnchor={last ? 'end' : 'start'}
                  fontSize={SVG_TEXT.label}
                  fill={FORK_COLORS.tick}
                >
                  {day.date.slice(5)}
                </text>
              )}
            </g>
          );
        })}
        {/* The target: a reference, so dashed (V13), over the bars, its words at its right end. */}
        <line
          data-role="target"
          x1={lane}
          x2={W}
          y1={target}
          y2={target}
          stroke={FORK_COLORS.ink}
          strokeDasharray="4 3"
        />
        <text
          data-role="target-label"
          x={W}
          y={tickBaseline(target)}
          textAnchor="end"
          fontSize={SVG_TEXT.label}
          fill={FORK_COLORS.ink}
          stroke={FORK_COLORS.halo}
          strokeWidth={4}
          paintOrder="stroke"
        >
          {TARGET_WORDS}
        </text>
      </svg>
    </div>
  );
}

/**
 * The plot's reading row, under the hero section: the hovered or tapped day, else the latest day
 * (as the DAO page's history), with the section's one "tap or hover" (R14).
 */
export function ForkDaysReading({
  days,
  pin,
}: {
  days: readonly Day[];
  pin: PinnedReadout<string>;
}) {
  const active = activeDay(days, pin);
  return (
    <div data-testid="fork-day-reading">
      <ReadoutRow
        label="Day readout"
        pinned={active !== null && pin.showingPin}
        idle={reading(days[days.length - 1])}
      >
        {active === null ? null : reading(days[active])}
      </ReadoutRow>
    </div>
  );
}
