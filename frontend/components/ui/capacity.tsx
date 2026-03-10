'use client';

import { cn, formatCkbAmount } from '@/lib/utils';
import { TerminalNumber } from '@/components/ui/terminal-number';

interface CapacityProps {
  value: string | bigint;
  className?: string;
  showUnit?: boolean;
  showSign?: boolean;
  animate?: boolean;
  glowIntensity?: 'none' | 'subtle' | 'strong';
}

export function Capacity({
  value,
  className,
  showUnit = true,
  showSign = false,
  animate = true,
  glowIntensity = 'subtle',
}: CapacityProps) {
  const { integer, decimal, isNegative } = formatCkbAmount(value);
  const signPrefix = showSign ? (isNegative ? '-' : '+') : isNegative ? '-' : '';

  return (
    <span className={cn('font-mono tabular-nums', className)}>
      {signPrefix && (
        <span className={cn(isNegative ? 'text-negative' : 'text-mint')}>{signPrefix}</span>
      )}
      <TerminalNumber value={integer} animate={animate} glowIntensity={glowIntensity} />
      <span className="text-amber-dim text-[0.85em]">.{decimal}</span>
      {showUnit && <span className="text-amber-dim ml-1 text-[0.85em]">CKB</span>}
    </span>
  );
}
