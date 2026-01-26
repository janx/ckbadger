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
  variant?: 'default' | 'bright' | 'dim';
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

  const variantClasses = {
    default: 'text-terminal-dim',
    bright: 'text-terminal-green',
    dim: 'text-terminal-dark',
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
        copyable && 'hover:text-terminal-green cursor-pointer',
        className
      )}
      onClick={handleCopy}
      title={copyable ? 'Click to copy' : value}
    >
      {copied ? '✓' : displayValue}
    </span>
  );
}
