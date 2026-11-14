import { NoBreak } from '@/components/ui/no-break';
import { formatTimeAgo, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';

/**
 * A chain time as its UTC+8 date, the relative age its hover (consistency C3 and lane P: "ago"
 * drifts while the page is read, so it never stands in the text; one form on every page). The date
 * is one unit: a line never breaks at its hyphens (U-16, "since 2024-" / "08-29").
 */
export function Utc8Date({
  timestamp,
  className,
}: {
  timestamp: string | number;
  className?: string;
}) {
  return (
    <NoBreak>
      <time className={className} title={formatTimeAgo(timestamp)}>
        {formatUtc8Date(timestamp)}
      </time>
    </NoBreak>
  );
}

/**
 * A time on the site's one clock with its date, `2026-10-03 08:31:52 UTC+8` (D2), as one unit: a
 * line never breaks at the date's hyphens nor between the date, the time and the zone (PB-05).
 */
export function Utc8DateTime({
  timestamp,
  className,
  title,
  dateTime,
}: {
  timestamp: string | number | Date;
  className?: string;
  /** The hover, when the time says more there (its age, what it is the time of). */
  title?: string;
  /** The machine-readable time, when the page states it. */
  dateTime?: string;
}) {
  return (
    <NoBreak>
      <time className={className} title={title} dateTime={dateTime}>
        {formatUtc8DateTime(timestamp)}
      </time>
    </NoBreak>
  );
}

/**
 * A chain time as its age on a row of a live list (PA-22: blocks, transactions, activities, the
 * pool): `20s ago` in the one age form (`formatTimeAgo`), the full date-time on the site's clock its
 * hover. A list that is historical by nature (an address's history, a block's transactions) keeps
 * `Utc8Date`. One unit, as every time is.
 */
export function Utc8Age({ timestamp, className }: { timestamp: string; className?: string }) {
  return (
    <NoBreak>
      <time className={className} title={formatUtc8DateTime(timestamp)}>
        {formatTimeAgo(timestamp)}
      </time>
    </NoBreak>
  );
}
