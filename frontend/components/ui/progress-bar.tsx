'use client';

import { cn } from '@/lib/utils';
import { formatScaledInteger } from '@/lib/exact-integer';

function barRatio(value: number, max: number): { width: number; text: string } {
  if (!Number.isSafeInteger(value) || value < 0 || !Number.isSafeInteger(max) || max < 0) {
    throw new Error(`invalid capacity bar ratio ${value}/${max}`);
  }
  if (max === 0) return { width: 0, text: '—' };
  return {
    width: Math.min((value / max) * 100, 100),
    text: `${formatScaledInteger(BigInt(value) * BigInt(100), BigInt(max), 1)}%`,
  };
}

interface ProgressBarProps {
  value: number;
  max: number;
  showLabel?: boolean;
  labelFormat?: 'percent' | 'value' | 'both';
  size?: 'sm' | 'md' | 'lg';
  color?: 'auto' | 'green' | 'gold' | 'blue';
  className?: string;
}

export function ProgressBar({
  value,
  max,
  showLabel = true,
  labelFormat = 'both',
  size = 'md',
  color = 'auto',
  className,
}: ProgressBarProps) {
  const ratio = barRatio(value, max);
  const percent = ratio.width;

  const getColor = () => {
    if (color !== 'auto') return color;
    if (percent < 33) return 'green';
    if (percent < 66) return 'gold';
    return 'red';
  };

  const colorClass = getColor();

  const barColors = {
    green: 'bg-gradient-to-r from-jade-dim via-jade to-jade',
    gold: 'bg-gradient-to-r from-gold-dim via-gold to-gold',
    blue: 'bg-gradient-to-r from-aqua via-aqua to-aqua',
    red: 'bg-gradient-to-r from-negative-dim via-negative to-negative-bright',
  };

  const glowColors = {
    green: '',
    gold: '',
    blue: '',
    red: '',
  };

  const sizeClasses = {
    sm: 'h-1.5',
    md: 'h-2',
    lg: 'h-3',
  };

  const formatLabel = () => {
    switch (labelFormat) {
      case 'percent':
        return ratio.text;
      case 'value':
        return `${value.toLocaleString()} / ${max.toLocaleString()}`;
      case 'both':
        return `${value.toLocaleString()} / ${max.toLocaleString()} (${ratio.text})`;
    }
  };

  return (
    <div className={cn('', className)}>
      <div
        className={cn(
          'bg-base-elevated relative w-full overflow-hidden rounded-full',
          sizeClasses[size]
        )}
      >
        <div
          className={cn(
            'absolute inset-y-0 left-0 rounded-full transition-all duration-500',
            barColors[colorClass],
            percent > 0 && glowColors[colorClass]
          )}
          style={{ width: `${percent}%` }}
        />
      </div>
      {showLabel && (
        <div className="text-text-dim mt-1 font-mono text-xs tabular-nums">{formatLabel()}</div>
      )}
    </div>
  );
}

interface UsageBarProps {
  value: number;
  max: number;
  unit?: string;
  className?: string;
}

export function UsageBar({ value, max, unit = '', className }: UsageBarProps) {
  const ratio = barRatio(value, max);
  const percent = ratio.width;

  const getColorClass = () => {
    if (percent < 33) return 'bg-positive/30';
    if (percent < 66) return 'bg-warning/30';
    return 'bg-negative/30';
  };

  return (
    <span
      className={cn(
        'bg-base-elevated text-text-bright relative inline-flex overflow-hidden rounded px-2 py-1 font-mono text-sm',
        className
      )}
    >
      <span
        className={cn('absolute inset-y-0 left-0 transition-all', getColorClass())}
        style={{ width: `${percent}%` }}
      />
      <span className="relative tabular-nums">
        {value.toLocaleString()} / {max.toLocaleString()}
        {unit && ` ${unit}`} ({ratio.text})
      </span>
    </span>
  );
}
