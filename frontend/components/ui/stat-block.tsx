'use client';

import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

type TrendDirection = 'up' | 'down' | 'neutral';

interface StatBlockProps {
  label: string;
  value: number | string;
  decimals?: number;
  prefix?: string;
  suffix?: string;
  trend?: {
    direction: TrendDirection;
    value: string;
    label?: string;
  };
  size?: 'sm' | 'md' | 'lg';
  color?: 'green' | 'amber' | 'white';
  className?: string;
  labelClassName?: string;
  subtext?: ReactNode;
}

const sizeClasses = {
  sm: { label: 'text-xs', value: 'text-lg', trend: 'text-xs', gap: 'gap-1' },
  md: { label: 'text-sm', value: 'text-2xl', trend: 'text-sm', gap: 'gap-2' },
  lg: { label: 'text-base', value: 'text-3xl', trend: 'text-base', gap: 'gap-3' },
};

const colorClasses = {
  green: 'text-terminal-green',
  amber: 'text-amber',
  white: 'text-white',
};

export function StatBlock({
  label,
  value,
  decimals = 0,
  prefix,
  suffix,
  trend,
  size = 'md',
  color = 'green',
  className,
  labelClassName,
  subtext,
}: StatBlockProps) {
  const config = sizeClasses[size];

  const trendColors: Record<TrendDirection, string> = {
    up: 'text-terminal-green',
    down: 'text-red-400',
    neutral: 'text-slate-500',
  };

  const trendIcons: Record<TrendDirection, string> = {
    up: '↑',
    down: '↓',
    neutral: '→',
  };

  const formatValue = (val: number | string): string => {
    if (typeof val === 'string') return val;
    const fixed = val.toFixed(decimals);
    const [intPart, decPart] = fixed.split('.');
    const formatted = intPart.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
    return decPart ? `${formatted}.${decPart}` : formatted;
  };

  return (
    <div className={cn('flex flex-col', config.gap, className)}>
      <div
        className={cn(
          'font-mono uppercase tracking-wider text-slate-500',
          config.label,
          labelClassName
        )}
      >
        {label}
      </div>

      <div className="flex items-baseline gap-2">
        <span
          className={cn(
            'font-mono font-bold tabular-nums transition-all',
            config.value,
            colorClasses[color]
          )}
        >
          {prefix}
          {formatValue(value)}
          {suffix}
        </span>

        {trend && (
          <span className={cn('flex items-center gap-1', trendColors[trend.direction])}>
            <span>{trendIcons[trend.direction]}</span>
            <span className={cn('font-mono', config.trend)}>{trend.value}</span>
            {trend.label && <span className="ml-1 text-slate-600">{trend.label}</span>}
          </span>
        )}
      </div>

      {subtext && <div className="font-mono text-sm text-slate-600">{subtext}</div>}
    </div>
  );
}

interface StatGridProps {
  children: ReactNode;
  columns?: 2 | 3 | 4;
  className?: string;
}

export function StatGrid({ children, columns = 3, className }: StatGridProps) {
  const columnClasses = {
    2: 'grid-cols-2',
    3: 'grid-cols-2 md:grid-cols-3',
    4: 'grid-cols-2 md:grid-cols-4',
  };

  return <div className={cn('grid gap-6', columnClasses[columns], className)}>{children}</div>;
}

interface StatDividerProps {
  orientation?: 'horizontal' | 'vertical';
  className?: string;
}

export function StatDivider({ orientation = 'horizontal', className }: StatDividerProps) {
  if (orientation === 'vertical') {
    return (
      <div
        className={cn(
          'w-px self-stretch bg-gradient-to-b from-transparent via-slate-700 to-transparent',
          className
        )}
      />
    );
  }

  return (
    <div
      className={cn(
        'h-px w-full bg-gradient-to-r from-transparent via-slate-700 to-transparent',
        className
      )}
    />
  );
}

interface MiniStatProps {
  label: string;
  value: string | number;
  color?: 'green' | 'amber' | 'white' | 'dim';
  className?: string;
}

export function MiniStat({ label, value, color = 'dim', className }: MiniStatProps) {
  const miniColorClasses = {
    green: 'text-terminal-green',
    amber: 'text-amber',
    white: 'text-white',
    dim: 'text-slate-400',
  };

  return (
    <div className={cn('flex items-center justify-between gap-4', className)}>
      <span className="font-mono text-xs uppercase tracking-wide text-slate-600">{label}</span>
      <span className={cn('font-mono text-sm tabular-nums', miniColorClasses[color])}>{value}</span>
    </div>
  );
}
