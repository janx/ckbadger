'use client';

import { useSyncExternalStore } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { clockNow, subscribeClock } from '@/lib/clock';
import { formatTimeAgo, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';

/**
 * A chain time as its UTC+8 date, the relative age its hover (consistency C3 and lane P): the form
 * of a list that is historical by nature and of a time in a sentence. A row of a live list says its
 * age instead (PA-22, `Utc8Age`), read from the page's one clock so it never drifts (the review's
 * item H). The date is one unit: a line never breaks at its hyphens (U-16, "since 2024-" / "08-29").
 */
export function Utc8Date({
  timestamp,
  className,
}: {
  timestamp: string | number;
  className?: string;
}) {
  // the hover's age reads the page's one clock, as every age does (the review's item H)
  const age = useAge(timestamp);
  return (
    <NoBreak>
      <time className={className} title={age}>
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
  const age = useAge(timestamp);
  return (
    <NoBreak>
      <time className={className} title={formatUtc8DateTime(timestamp)}>
        {age}
      </time>
    </NoBreak>
  );
}

/**
 * Any text a time on the page's one clock decides (lib/clock.ts, the review's item H): `text`
 * writes it from the clock's time, and that text is the snapshot, so the reader renders again only
 * when what it writes changes — an age under a minute each second, under an hour each minute, a
 * count of days once a day. The one path from the clock to the page: every age on screen reads it
 * (`useAge`, `ClockText`, `useClockTime`), however it is worded (lane E4).
 */
export function useClockText(text: (now: number) => string): string {
  const read = () => text(clockNow());
  return useSyncExternalStore(subscribeClock, read, read);
}

/**
 * A time's age in the one form (`formatTimeAgo`), read from the page's one clock: for an age in an
 * attribute (a hover) or a component's own line.
 */
export function useAge(timestamp: string | number): string {
  return useClockText((now) => formatTimeAgo(timestamp, now));
}

/**
 * A clock-written text in a sentence, a fact or a cell (`updated 5m ago`, `in pool for 5m`), as
 * bare text: no element of its own, so the line it stands in is laid out as before; only it
 * renders again as the clock moves, never the page around it (lane E4).
 */
export function ClockText({ text }: { text: (now: number) => string }) {
  return <>{useClockText(text)}</>;
}

/**
 * The clock's time, for a reader that writes several things from one time (an identity's state,
 * its days to or since its expiry and its term bar's now mark; a feed's day dividers): `text`
 * writes from a time everything the reader shows of it, and the reader renders again only when
 * that changes. Every value it writes from the returned time must be in `text`.
 */
export function useClockTime(text: (now: number) => string): number {
  useClockText(text);
  return clockNow();
}
