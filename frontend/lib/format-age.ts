/**
 * A fact rail's relative age in its two largest units (Transaction board: "2d 20h ago"); feed
 * rows keep `formatTimeAgo`'s one unit. Truncates, never rounds up.
 */
export function formatAgeTwoUnits(timestamp: string | number | Date, now = Date.now()): string {
  const then =
    timestamp instanceof Date
      ? timestamp.getTime()
      : typeof timestamp === 'number'
        ? timestamp
        : /^\d+$/.test(timestamp)
          ? Number(timestamp)
          : Date.parse(timestamp);
  if (!Number.isFinite(then)) throw new Error(`Invalid timestamp ${JSON.stringify(timestamp)}`);
  if (then > now) throw new Error(`Timestamp ${JSON.stringify(timestamp)} is after now`);
  const seconds = Math.floor((now - then) / 1000);
  const units: [number, string][] = [
    [86400, 'd'],
    [3600, 'h'],
    [60, 'm'],
    [1, 's'],
  ];
  const first = units.findIndex(([size]) => seconds >= size);
  if (first === -1) return '0s ago';
  const [size, unit] = units[first];
  const lead = `${Math.floor(seconds / size)}${unit}`;
  const next = units[first + 1];
  if (!next) return `${lead} ago`;
  const rest = Math.floor((seconds % size) / next[0]);
  return rest === 0 ? `${lead} ago` : `${lead} ${rest}${next[1]} ago`;
}
