import { cn } from '@/lib/utils';

/**
 * The one wording of the touch hint (plan R6; `docs/RESPONSIVE_LAYOUT.md`, Touch): anything a hover
 * reveals is also reachable by a tap, so a hint never asks for a hover alone. Every hint in the
 * app is built here, so they all read the same (`__tests__/lib/tap-or-hover-wording.test.ts`).
 */
const WORDS = 'tap or hover';

export interface TapOrHoverOptions {
  /** Start a sentence: "Tap or hover a day." */
  capital?: boolean;
}

/**
 * The hint as text — "tap or hover a day", or "Tap or hover a day." with `capital` — for a string
 * (a `title`, an idle line passed as a prop). In JSX text render `TapOrHoverHint`.
 */
export function tapOrHover(what?: string, { capital = false }: TapOrHoverOptions = {}): string {
  const words = capital ? `${WORDS[0].toUpperCase()}${WORDS.slice(1)}` : WORDS;
  if (what === undefined) return words;
  if (what.trim() === '') throw new Error('tap-or-hover hint: an empty target');
  return `${words} ${what}`;
}

export interface TapOrHoverHintProps extends TapOrHoverOptions {
  /** What the reader taps or hovers, e.g. "a day" or "for its name"; omitted, the bare words. */
  what?: string;
  className?: string;
}

/** The touch hint as an inline element: "tap or hover a day". */
export function TapOrHoverHint({ what, capital, className }: TapOrHoverHintProps) {
  return (
    <span data-hint="tap-or-hover" className={cn(className)}>
      {tapOrHover(what, { capital })}
    </span>
  );
}
