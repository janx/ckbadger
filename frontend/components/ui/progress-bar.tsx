'use client';

import { cn } from '@/lib/utils';

interface ProgressBarProps {
  value: number;
  max: number;
  showLabel?: boolean;
  labelFormat?: 'percent' | 'value' | 'both';
  size?: 'sm' | 'md' | 'lg';
  color?: 'auto' | 'green' | 'amber' | 'blue';
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
  const percent = max > 0 ? Math.min((value / max) * 100, 100) : 0;

  const getColor = () => {
    if (color !== 'auto') return color;
    if (percent < 33) return 'green';
    if (percent < 66) return 'amber';
    return 'red';
  };

  const colorClass = getColor();

  const barColors = {
    green: 'bg-gradient-to-r from-emphasis-dim via-emphasis-dim to-emphasis',
    amber: 'bg-gradient-to-r from-warning-dim via-warning-dim to-warning',
    blue: 'bg-gradient-to-r from-blue-900 via-blue-600 to-blue-400',
    red: 'bg-gradient-to-r from-red-900 via-red-600 to-red-400',
  };

  const glowColors = {
    green: '',
    amber: '',
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
        return `${percent.toFixed(1)}%`;
      case 'value':
        return `${value.toLocaleString()} / ${max.toLocaleString()}`;
      case 'both':
        return `${value.toLocaleString()} / ${max.toLocaleString()} (${percent.toFixed(1)}%)`;
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
        <div className="text-text-muted mt-1 font-mono text-xs tabular-nums">{formatLabel()}</div>
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
  const percent = max > 0 ? Math.min((value / max) * 100, 100) : 0;

  const getColorClass = () => {
    if (percent < 33) return 'bg-green-500/30';
    if (percent < 66) return 'bg-yellow-500/30';
    return 'bg-red-500/30';
  };

  return (
    <span
      className={cn(
        'bg-base-elevated relative inline-flex overflow-hidden rounded px-2 py-1 font-mono text-sm text-white',
        className
      )}
    >
      <span
        className={cn('absolute inset-y-0 left-0 transition-all', getColorClass())}
        style={{ width: `${percent}%` }}
      />
      <span className="relative tabular-nums">
        {value.toLocaleString()} / {max.toLocaleString()}
        {unit && ` ${unit}`} ({percent.toFixed(1)}%)
      </span>
    </span>
  );
}
