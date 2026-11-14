'use client';

import { useState } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { shortenId, type IdKind, type ShortSize } from '@/lib/shorten';
import { cn } from '@/lib/utils';

/**
 * How an id renderer writes its value (U-09): shortened at one of the three sizes of
 * `lib/shorten.ts`, which every call site picks from where it stands (`head` in an h1 or a lede,
 * `row` in a table, a feed or a tile, `tight` in a phone row, a chart label or a narrow tile), or
 * whole with `truncate={false}`. No length can be passed: a hand length is a size of its own.
 */
export type IdShortening = { size: ShortSize; truncate?: true } | { truncate: false; size?: never };

type HexCodeProps = IdShortening & {
  value: string;
  /**
   * What the value is, for its shortened form (`lib/shorten.ts`): a hash (any hex id) by default,
   * or an address where a party is written by its address or else its lock hash.
   */
  kind?: IdKind;
  className?: string;
  copyable?: boolean;
  /**
   * The ink: the text ink by default, bright or dim; `inherit` writes no ink of its own, so the
   * value takes the ink (and the size) of the line it stands in — a name plate's id line, which
   * sets its voice once (seals by kind, C10 round 2).
   */
  variant?: 'default' | 'bright' | 'dim' | 'inherit';
};

export function HexCode({
  value,
  kind = 'hash',
  className,
  copyable = true,
  variant = 'default',
  ...shortening
}: HexCodeProps) {
  const [copied, setCopied] = useState(false);

  const displayValue =
    shortening.truncate === false ? value : shortenId(value, kind, shortening.size);
  const shortened = displayValue !== value;

  const variantClasses: Record<NonNullable<HexCodeProps['variant']>, string | null> = {
    default: 'text-text',
    bright: 'text-text-bright',
    dim: 'text-text-dim',
    inherit: null,
  };

  const handleCopy = async () => {
    if (!copyable) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  const props = {
    className: cn(
      'font-mono',
      variantClasses[variant],
      // Midnight, continued (move 1c): a hovered hash glows jade, as a hovered link does.
      copyable && 'hover-lit cursor-pointer transition-colors',
      // U-16: a full value is one token too. It sits whole on its line where the line holds it;
      // in a narrower column it is one line in its own box, as wide as the column, that scrolls
      // sideways with the page's thin scrollbar — never wrapped in the middle. `align-top` keeps
      // its text on the line's text: a scrolling box's baseline is its bottom edge.
      !shortened && 'inline-block max-w-full overflow-x-auto whitespace-nowrap align-top',
      className
    ),
    onClick: handleCopy,
    title: copyable ? 'Click to copy' : value,
    children: copied ? '✓' : displayValue,
  };
  // A shortened value is one unit that never breaks at its ellipsis (U-16).
  return shortened ? <NoBreak {...props} /> : <span data-full-value="" {...props} />;
}
