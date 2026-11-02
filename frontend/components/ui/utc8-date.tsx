import { formatTimeAgo, formatUtc8Date } from '@/lib/utils';

/**
 * A chain time as its UTC+8 date, the relative age its hover (consistency C3 and lane P: "ago"
 * drifts while the page is read, so it never stands in the text; one form on every page).
 */
export function Utc8Date({
  timestamp,
  className,
}: {
  timestamp: string | number;
  className?: string;
}) {
  return (
    <time className={className} title={formatTimeAgo(timestamp)}>
      {formatUtc8Date(timestamp)}
    </time>
  );
}
