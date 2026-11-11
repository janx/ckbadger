'use client';

import { useState } from 'react';
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

  const displayValue =
    truncate && value.length > startChars + endChars
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

  return (
    <span
      className={cn(
        'font-mono',
        variantClasses[variant],
        // Midnight, continued (move 1c): a hovered hash glows jade, as a hovered link does.
        copyable && 'hover-lit cursor-pointer transition-colors',
        className
      )}
      onClick={handleCopy}
      title={copyable ? 'Click to copy' : value}
    >
      {copied ? '✓' : displayValue}
    </span>
  );
}
