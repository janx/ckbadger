'use client';

import { useEffect } from 'react';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout, type PinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';

interface Day {
  date: string;
  uncles: number;
  nodeSwitches: number;
}

/** The board's drawing width, used until the box is measured. */
const DESIGN_WIDTH = 1210;
/** The bars' height: with the name band and the axis the plot is ≥ 280 px (hierarchy v3, R13). */
const H0 = 230;
/** The band at the top that holds the name and the key: one 11 px line, two on a narrow column. */
const NAME_LINE = 28;
const NAME_LINE_2 = 16;
/** Under the bars: the first and last day's labels. */
const AXIS = 22;
/** Ghost ink for the values written inside the plot (as the DAO page's charts, v3). */
const GHOST = 'var(--c-text-ghost)';
/** About 6.6 px a character at 11 px in the app's mono face. */
const CHAR = 6.6;
const UNCLES_KEY = 'uncles on chain, per day';
const SWITCHES_KEY = 'this node’s branch switches';

/** The smallest 1-2-5 step that covers `max` in at most four gridlines. */
function scale(max: number): { cap: number; step: number } {
  if (max <= 0) return { cap: 1, step: 1 };
  for (let magnitude = 1; ; magnitude *= 10)
    for (const unit of [1, 2, 5]) {
      const step = unit * magnitude;
      if (Math.ceil(max / step) <= 4) return { cap: Math.ceil(max / step) * step, step };
    }
}

const reading = (day: Day) =>
  `${day.date} · ${formatCount(day.uncles, 'uncle')} on chain · this node switched branches ${formatCount(day.nodeSwitches, 'time')}`;

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
 * Uncles the chain recorded each day (grey) with this node's branch switches drawn inside them in
 * jade (Forks V67, FF-L2) — the Forks page's picture (hierarchy v3, R13): ≥ 280 px, its name and
 * key inside the plot at the top left, the values inside in ghost ink, the first and last day
 * under it. Hover, the arrow keys or a tap choose a day for `ForkDaysReading` (R6).
 */
export function ForkDaysPlot({ days, pin }: { days: readonly Day[]; pin: PinnedReadout<string> }) {
  // Days are named by date, so a refetched window keeps the same day (or lets a gone day go).
  const active = activeDay(days, pin);
  const gone = pin.active !== null && active === null;
  const { clear } = pin;
  useEffect(() => {
    if (gone) clear();
  }, [gone, clear]);
  // Drawn 1:1 in its box, so the 11 px text stays 11 px at phone width (visual pass, 390).
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const W = measured ?? DESIGN_WIDTH;
  // The key's second series moves to a second line where one line would not hold both.
  const keyGap = 16;
  const secondX = 14 + UNCLES_KEY.length * CHAR + keyGap;
  const oneLine = secondX + 14 + SWITCHES_KEY.length * CHAR <= W;
  const top = NAME_LINE + (oneLine ? 0 : NAME_LINE_2);
  const H = top + H0 + AXIS;
  const { cap, step } = scale(Math.max(...days.map((d) => Math.max(d.uncles, d.nodeSwitches))));
  const ticks = Array.from({ length: cap / step }, (_, i) => (i + 1) * step);
  // The values sit in a lane at the plot's left, inside its gridlines; the bars start after it.
  const lane = Math.max(...ticks.map((v) => v.toLocaleString('en-US').length)) * CHAR + 8;
  const bw = (W - lane) / Math.max(days.length, 1);
  const y = (v: number) => top + H0 - (v / cap) * H0;
  // A value whose label would reach into the name's band is not written (as the DAO charts).
  const written = (v: number) => y(v) - 4 - 11 >= top;
  const base = top + H0;
  const keyY2 = oneLine ? 0 : NAME_LINE_2;
  return (
    <div ref={box} className="min-w-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label="Uncles the chain recorded each day, and this node's branch switches; ← → step through the days"
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
          <rect x="0" y="6" width="10" height="10" rx="2" fill={FORK_COLORS.uncles} opacity=".5" />
          <text x="14" y="15" fontSize="11" fill={FORK_COLORS.tick}>
            {UNCLES_KEY}
          </text>
          <rect
            x={oneLine ? secondX : 0}
            y={6 + keyY2}
            width="10"
            height="10"
            rx="2"
            fill={FORK_COLORS.nodeSwitches}
          />
          <text
            x={(oneLine ? secondX : 0) + 14}
            y={15 + keyY2}
            fontSize="11"
            fill={FORK_COLORS.tick}
          >
            {SWITCHES_KEY}
          </text>
        </g>
        {ticks.map((v) => (
          <g key={v}>
            {/* The scale's lines are grid: solid (visual finish V13 b). */}
            <line data-role="grid" x1="0" x2={W} y1={y(v)} y2={y(v)} stroke={FORK_COLORS.grid} />
            {written(v) && (
              <text data-role="y-tick" x="0" y={y(v) - 4} fontSize="11" fill={GHOST}>
                {v.toLocaleString('en-US')}
              </text>
            )}
          </g>
        ))}
        <line x1="0" x2={W} y1={base} y2={base} stroke={FORK_COLORS.axis} />
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
              <rect x={lane + i * bw} y={top} width={bw} height={H0} fill="transparent" />
              <rect
                data-uncles
                x={x}
                y={y(day.uncles)}
                width={width}
                height={base - y(day.uncles)}
                rx="1.5"
                fill={FORK_COLORS.uncles}
                fillOpacity=".35"
              />
              {day.nodeSwitches > 0 && (
                <rect
                  data-switches
                  x={x}
                  y={base - Math.max(base - y(day.nodeSwitches), 1)}
                  width={width}
                  height={Math.max(base - y(day.nodeSwitches), 1)}
                  rx="1"
                  fill={FORK_COLORS.nodeSwitches}
                />
              )}
              {/* R14: the first and the last day only. */}
              {(first || last) && (
                <text
                  x={last ? x + width : x}
                  y={base + 16}
                  textAnchor={last ? 'end' : 'start'}
                  fontSize="11"
                  fill={FORK_COLORS.tick}
                >
                  {day.date.slice(5)}
                </text>
              )}
            </g>
          );
        })}
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
