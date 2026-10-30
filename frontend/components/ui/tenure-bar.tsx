import { useLayoutEffect, useRef, useState } from 'react';
import { CLASS_COLORS, EXPIRED_COLOR } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';

export interface TenurePeriod {
  label: string;
  /** Milliseconds since the epoch. */
  start: number;
  end: number;
}
export interface TenureBarProps {
  name: string;
  /** Registration then each renewal, contiguous and in order. */
  periods: readonly TenurePeriod[];
  /** End of a grace period after expiry (`.cell`: 30 days), drawn dashed. */
  graceEnd?: number;
  now: number;
  /** F2 identities (round 2): a caption beside the title, e.g. "3 years paid · 2 renewals". */
  note?: string;
}

/** Drawing width before (or without) a measurement: the board's 900 px column. */
const DEFAULT_W = 900;
const PAD = 20;
const DAY = 86_400_000;
/** Advance of one monospace character at the label sizes (11 / 10.5 px), for fitting labels. */
const CHAR_11 = 6.7;
const CHAR_10 = 6.4;

/**
 * F2 identities (round 2, visual pass): the bar draws in its own measured width, so 1 unit is
 * 1 px and its 11 px labels stay 11 px in a 358 px column instead of shrinking to 4 px.
 */
function useDrawingWidth() {
  const ref = useRef<SVGSVGElement>(null);
  const [width, setWidth] = useState(DEFAULT_W);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver((entries) => {
      const w = Math.round(entries[0]?.contentRect.width ?? 0);
      if (w > 0) setWidth(w);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

/**
 * "Held until / Expired on" (ID-C7): the paid term as registration + renewals, the elapsed part
 * dim and the remaining part bright, a dashed grace period, and a now marker.
 */
export function TenureBar({ name, periods, graceEnd, now, note }: TenureBarProps) {
  const [svgRef, W] = useDrawingWidth();
  if (!periods.length) throw new Error(`${name}: tenure has no paid period`);
  periods.forEach((p, i) => {
    if (!(p.end > p.start)) throw new Error(`${name}: period ${p.label} ends before it starts`);
    if (i > 0 && p.start !== periods[i - 1].end)
      throw new Error(`${name}: period ${p.label} does not continue the previous one`);
  });
  const first = periods[0].start,
    end = periods[periods.length - 1].end;
  if (graceEnd !== undefined && graceEnd <= end)
    throw new Error(`${name}: grace ends before the term does`);
  const last = Math.max(graceEnd ?? end, now);
  const x = (t: number) =>
    Math.round((PAD + ((t - first) / (last - first)) * (W - 2 * PAD)) * 10) / 10;
  const expired = now >= end;
  // Expired terms are neutral grey: gold is DAO-only, rose is a live identity (FINAL_REVIEW).
  const ink = expired ? EXPIRED_COLOR : CLASS_COLORS.identities;
  const days = Math.floor(Math.abs(end - now) / DAY).toLocaleString('en-US');
  const endDate = formatUtc8Date(end);
  // A label that does not fit its period keeps its first word, or is left out (narrow columns).
  const fit = (label: string, room: number) =>
    label.length * CHAR_11 <= room
      ? label
      : label.split(' ')[0].length * CHAR_11 <= room
        ? label.split(' ')[0]
        : null;
  const nowText = expired ? `expired ${days} days ago` : `now · ${days} days left`;
  const nowX = x(now),
    nowHalf = (nowText.length * CHAR_11) / 2;
  const nowAnchor = nowX + nowHalf > W - 2 ? 'end' : nowX - nowHalf < 2 ? 'start' : 'middle';
  const lapsed = expired && graceEnd === undefined && now > end;
  return (
    <section className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-[15px] font-medium">
          {expired ? `Expired on ${endDate}` : `Held until ${endDate}`}
        </h2>
        {note && <span className="text-text-dim text-[12px]">{note}</span>}
      </div>
      <svg
        role="img"
        aria-label={`${name} is held from ${formatUtc8Date(first)} to ${endDate}${graceEnd ? `, then a grace period to ${formatUtc8Date(graceEnd)}` : ''}; ${expired ? `expired ${days} days ago` : `${days} days remain`}`}
        ref={svgRef}
        viewBox={`0 0 ${W} 92`}
        width="100%"
        className="block overflow-visible font-mono"
      >
        {periods.map((p, i) => {
          const x1 = x(p.start),
            x2 = x(p.end),
            split = Math.min(Math.max(now, p.start), p.end),
            xs = x(split);
          const label = fit(p.label, x2 - x1 - 6);
          // The last period shares its row with the end date, anchored at its right end.
          const dateRoom = x2 - x1 - 6 - (i === periods.length - 1 ? 10 * CHAR_10 + 8 : 0);
          return (
            <g key={p.label}>
              {xs > x1 && (
                <rect
                  data-elapsed
                  x={x1}
                  y="30"
                  width={Math.max(xs - x1 - (xs === x2 ? 2 : 0), 0)}
                  height="14"
                  rx="3"
                  fill={ink}
                  fillOpacity="0.28"
                />
              )}
              {x2 > xs && (
                <rect
                  data-remaining
                  x={xs}
                  y="30"
                  width={Math.max(x2 - xs - 2, 0)}
                  height="14"
                  rx="3"
                  fill={ink}
                  fillOpacity="0.9"
                />
              )}
              {label && (
                <text x={x1 + 2} y="22" fontSize="11" className="fill-text">
                  {label}
                </text>
              )}
              {10 * CHAR_10 <= dateRoom && (
                <text x={x1 + 2} y="62" fontSize="10.5" className="fill-text-dim">
                  {formatUtc8Date(p.start)}
                </text>
              )}
            </g>
          );
        })}
        <text x={x(end) - 2} y="62" textAnchor="end" fontSize="10.5" className="fill-text-bright">
          {endDate}
        </text>
        {graceEnd !== undefined && (
          <>
            <rect
              data-grace
              x={x(end)}
              y="30"
              width={x(graceEnd) - x(end)}
              height="14"
              rx="3"
              fill="none"
              stroke={ink}
              strokeOpacity="0.5"
              strokeDasharray="3 3"
            />
            <text x={x(graceEnd)} y="22" textAnchor="end" fontSize="11" className="fill-text">
              grace · {Math.round((graceEnd - end) / DAY)} days
            </text>
          </>
        )}
        {lapsed && (
          <>
            <rect
              data-lapsed
              x={x(end)}
              y="30"
              width={Math.max(x(now) - x(end), 0)}
              height="14"
              rx="3"
              fill="none"
              stroke={EXPIRED_COLOR}
              strokeOpacity="0.7"
              strokeDasharray="3 3"
            />
            {fit('lapsed · not renewed', x(now) - x(end) - 8) && (
              <text x={x(end) + 4} y="22" fontSize="11" className="fill-text">
                {fit('lapsed · not renewed', x(now) - x(end) - 8)}
              </text>
            )}
          </>
        )}
        <line
          x1={x(now)}
          x2={x(now)}
          y1="26"
          y2="50"
          className="stroke-text-bright"
          strokeWidth="1.2"
        />
        <text x={nowX} y="80" textAnchor={nowAnchor} fontSize="11" className="fill-text-bright">
          {nowText}
        </text>
      </svg>
    </section>
  );
}
