/** The status line's clock: local time as HH:MM:SS, 24-hour. */
export function clockText(at: Date): string {
  return [at.getHours(), at.getMinutes(), at.getSeconds()]
    .map((n) => String(n).padStart(2, '0'))
    .join(':');
}

/** The local time zone's name (the clock's title and the `?` help say which time it is). */
export function localZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}
