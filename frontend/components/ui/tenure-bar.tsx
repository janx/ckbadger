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
}

const W = 900;
const PAD = 20;
const DAY = 86_400_000;

/**
 * "Held until / Expired on" (ID-C7): the paid term as registration + renewals, the elapsed part
 * dim and the remaining part bright, a dashed grace period, and a now marker.
 */
export function TenureBar({ name, periods, graceEnd, now }: TenureBarProps) {
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
  const days = Math.floor(Math.abs(end - now) / DAY);
  const endDate = formatUtc8Date(end);
  return (
    <section className="min-w-0 space-y-3">
      <h2 className="text-text-bright text-[15px] font-medium">
        {expired ? `Expired on ${endDate}` : `Held until ${endDate}`}
      </h2>
      <svg
        role="img"
        aria-label={`${name} is held from ${formatUtc8Date(first)} to ${endDate}${graceEnd ? `, then a grace period to ${formatUtc8Date(graceEnd)}` : ''}; ${expired ? `expired ${days} days ago` : `${days} days remain`}`}
        viewBox={`0 0 ${W} 92`}
        width="100%"
        className="block overflow-visible font-mono"
      >
        {periods.map((p) => {
          const x1 = x(p.start),
            x2 = x(p.end),
            split = Math.min(Math.max(now, p.start), p.end),
            xs = x(split);
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
              <text x={x1 + 2} y="22" fontSize="11" className="fill-text">
                {p.label}
              </text>
              <text x={x1 + 2} y="62" fontSize="10.5" className="fill-text-dim">
                {formatUtc8Date(p.start)}
              </text>
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
        <line
          x1={x(now)}
          x2={x(now)}
          y1="26"
          y2="50"
          className="stroke-text-bright"
          strokeWidth="1.2"
        />
        <text x={x(now)} y="80" textAnchor="middle" fontSize="11" className="fill-text-bright">
          {expired ? `expired ${days} days ago` : `now · ${days} days left`}
        </text>
      </svg>
    </section>
  );
}
