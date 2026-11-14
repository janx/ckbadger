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

/**
 * The page's one clock (the review's item H): every age on the page and the status line's clock
 * read it, so an age that is shown keeps telling the truth between data updates. One timer for the
 * page however many readers: it starts with the first reader, ticks on each whole second and stops
 * with the last. A reader takes it through `useSyncExternalStore` with a snapshot of its own text,
 * so it renders again only when that text changes (an age under a minute each second, under an
 * hour each minute, and so on: the age form's own unit decides).
 */
const readers = new Set<() => void>();
let clockMs = Date.now();
let timer: ReturnType<typeof setTimeout> | null = null;

function tick() {
  clockMs = Date.now();
  timer = setTimeout(tick, 1000 - (Date.now() % 1000));
  for (const read of readers) read();
}

/** Subscribe to the clock's ticks; the returned function unsubscribes. */
export function subscribeClock(onTick: () => void): () => void {
  if (readers.size === 0) {
    clockMs = Date.now();
    timer = setTimeout(tick, 1000 - (Date.now() % 1000));
  }
  readers.add(onTick);
  return () => {
    readers.delete(onTick);
    if (readers.size === 0 && timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };
}

/**
 * The clock's time, ms since the epoch, as of its last tick; with no reader yet (the first render,
 * before it subscribes), the time now, so an age never starts stale.
 */
export function clockNow(): number {
  if (readers.size === 0) clockMs = Date.now();
  return clockMs;
}
