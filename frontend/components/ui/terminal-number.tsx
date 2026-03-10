'use client';

import { cn } from '@/lib/utils';

interface TerminalNumberProps {
  value: string | number;
  animate?: boolean;
  glowIntensity?: 'none' | 'subtle' | 'strong';
  className?: string;
}

export function TerminalNumber({
  value,
  animate = true,
  glowIntensity = 'subtle',
  className,
}: TerminalNumberProps) {
  const displayValue = String(value);

  const glowClasses = {
    none: 'text-amber-dim',
    subtle: 'text-amber',
    strong: 'text-amber',
  };

  return (
    <span
      className={cn(
        'font-mono tabular-nums',
        glowClasses[glowIntensity],
        animate && 'transition-all duration-200',
        className
      )}
    >
      {displayValue}
    </span>
  );
}
