/** The status line's clock: local time as HH:MM:SS, 24-hour. */
export function clockText(at: Date): string {
  return [at.getHours(), at.getMinutes(), at.getSeconds()]
    .map((n) => String(n).padStart(2, '0'))
    .join(':');
}

/**
 * The local time zone's name (the clock's title, the `?` help and the footer say which time it
 * is), or null where Intl gives none (an engine may leave `timeZone` out): each place that writes
 * the zone then leaves it out, never writing "undefined" or a bare separator (A4 review 5).
 */
export function localZone(): string | null {
  const zone: string | undefined = Intl.DateTimeFormat().resolvedOptions().timeZone;
  return zone ? zone : null;
}

/** "Local time, Asia/Shanghai": the words, then the local zone where there is one. */
export function zoned(words: string): string {
  const zone = localZone();
  return zone === null ? words : `${words}, ${zone}`;
}
