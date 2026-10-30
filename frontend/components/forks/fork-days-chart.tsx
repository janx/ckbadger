'use client';

import { useState } from 'react';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatCount } from '@/lib/format-count';

interface Day {
  date: string;
  uncles: number;
  nodeSwitches: number;
}

const W = 1160;
const H0 = 120;
const LEFT = 44;

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

/**
 * Uncles the chain recorded each day (grey) with this node's branch switches drawn inside them in
 * jade (Forks V67, FF-L2). One reading at a time, by hover or the arrow keys.
 */
export function ForkDaysChart({ days }: { days: readonly Day[] }) {
  const [active, setActive] = useState<number | null>(null);
  const { cap, step } = scale(Math.max(...days.map((d) => Math.max(d.uncles, d.nodeSwitches))));
  const bw = W / Math.max(days.length, 1);
  const y = (v: number) => H0 - (v / cap) * H0;
  const ticks = Array.from({ length: cap / step }, (_, i) => (i + 1) * step);
  return (
    <div className="min-w-0">
      <p data-testid="fork-day-reading" className="text-text mb-1 min-h-[18px] text-[12px]">
        {active === null ? (
          <span className="text-text-dim">hover a day, or focus the chart and use ← →</span>
        ) : (
          reading(days[active])
        )}
      </p>
      <svg
        viewBox={`-${LEFT} -10 ${W + LEFT + 6} ${H0 + 34}`}
        role="img"
        aria-label="Uncles the chain recorded each day, and this node's branch switches"
        tabIndex={0}
        className="block h-auto w-full outline-none"
        onKeyDown={(event) => {
          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
          event.preventDefault();
          const last = days.length - 1;
          setActive((current) =>
            current === null
              ? last
              : Math.min(last, Math.max(0, current + (event.key === 'ArrowRight' ? 1 : -1)))
          );
        }}
        onMouseLeave={() => setActive(null)}
        onBlur={() => setActive(null)}
      >
        {ticks.map((v) => (
          <g key={v}>
            <line x1="0" x2={W} y1={y(v)} y2={y(v)} stroke={FORK_COLORS.grid} />
            <text x="-8" y={y(v) + 4} textAnchor="end" fontSize="11" fill={FORK_COLORS.tick}>
              {v.toLocaleString('en-US')}
            </text>
          </g>
        ))}
        <line x1="0" x2={W} y1={H0} y2={H0} stroke={FORK_COLORS.axis} />
        <text x="-8" y={H0 + 4} textAnchor="end" fontSize="11" fill={FORK_COLORS.tick}>
          0
        </text>
        {days.map((day, i) => {
          const x = i * bw + 3;
          const width = Math.max(bw - 6, 1);
          return (
            <g
              key={day.date}
              data-day={day.date}
              onMouseEnter={() => setActive(i)}
              opacity={active === null || active === i ? 1 : 0.55}
            >
              <title>{reading(day)}</title>
              <rect x={i * bw} y="-10" width={bw} height={H0 + 10} fill="transparent" />
              <rect
                data-uncles
                x={x}
                y={y(day.uncles)}
                width={width}
                height={H0 - y(day.uncles)}
                rx="1.5"
                fill={FORK_COLORS.uncles}
                fillOpacity=".35"
              />
              {day.nodeSwitches > 0 && (
                <rect
                  data-switches
                  x={x}
                  y={H0 - Math.max(H0 - y(day.nodeSwitches), 1)}
                  width={width}
                  height={Math.max(H0 - y(day.nodeSwitches), 1)}
                  rx="1"
                  fill={FORK_COLORS.nodeSwitches}
                />
              )}
              {(i % 5 === 0 || i === days.length - 1) && (
                <text x={x} y={H0 + 16} fontSize="11" fill={FORK_COLORS.tick}>
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
