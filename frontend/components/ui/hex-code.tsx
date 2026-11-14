'use client';

import { useState } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { cn } from '@/lib/utils';

interface HexCodeProps {
  value: string;
  truncate?: boolean;
  startChars?: number;
  endChars?: number;
  className?: string;
  copyable?: boolean;
  /**
   * The ink: the text ink by default, bright or dim; `inherit` writes no ink of its own, so the
   * value takes the ink (and the size) of the line it stands in — a name plate's id line, which
   * sets its voice once (seals by kind, C10 round 2).
   */
  variant?: 'default' | 'bright' | 'dim' | 'inherit';
}

export function HexCode({
  value,
  truncate = true,
  startChars = 10,
  endChars = 8,
  className,
  copyable = true,
  variant = 'default',
}: HexCodeProps) {
  const [copied, setCopied] = useState(false);

  const shortened = truncate && value.length > startChars + endChars;
  const displayValue = shortened
    ? `${value.slice(0, startChars)}...${value.slice(-endChars)}`
    : value;

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
