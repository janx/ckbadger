'use client';

import { CSSProperties, ReactNode } from 'react';
import { cn } from '@/lib/utils';

type TrendDirection = 'up' | 'down' | 'neutral';
type AccentColor = 'jade' | 'aqua' | 'gold' | 'rouge' | 'lavender' | 'default';
type GlowTier = 'neon' | 'soft' | 'none';

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
  color?: AccentColor;
  glowTier?: GlowTier;
  className?: string;
  labelClassName?: string;
  subtext?: ReactNode;
}

const sizeClasses = {
  sm: { label: 'text-xs', value: 'text-lg', trend: 'text-xs', gap: 'gap-1' },
  md: { label: 'text-sm', value: 'text-2xl', trend: 'text-sm', gap: 'gap-2' },
  lg: { label: 'text-base', value: 'text-3xl', trend: 'text-base', gap: 'gap-3' },
};

const colorClasses: Record<AccentColor, string> = {
  jade: 'text-jade',
  aqua: 'text-aqua',
  gold: 'text-gold',
  rouge: 'text-rouge',
  lavender: 'text-lavender',
  default: 'text-text-bright',
};

const glowColors: Record<
  Exclude<AccentColor, 'default'>,
  { color: string; mid: string; far: string }
> = {
  jade: { color: '#2edba3', mid: 'rgba(46, 219, 163, 0.25)', far: 'rgba(46, 219, 163, 0.15)' },
  aqua: { color: '#68ccf0', mid: 'rgba(104, 204, 240, 0.25)', far: 'rgba(104, 204, 240, 0.15)' },
  gold: { color: '#f2c55c', mid: 'rgba(242, 197, 92, 0.25)', far: 'rgba(242, 197, 92, 0.15)' },
  rouge: { color: '#e8555a', mid: 'rgba(232, 85, 90, 0.25)', far: 'rgba(232, 85, 90, 0.15)' },
  lavender: {
    color: '#b8a9e8',
    mid: 'rgba(184, 169, 232, 0.25)',
    far: 'rgba(184, 169, 232, 0.15)',
  },
};

function getGlowStyle(color: AccentColor, glowTier: GlowTier): CSSProperties | undefined {
  if (glowTier === 'none' || color === 'default') return undefined;
  const glow = glowColors[color];
  if (glowTier === 'neon') {
    return {
      '--glow-color': glow.color,
      '--glow-color-mid': glow.mid,
      '--glow-color-far': glow.far,
    } as CSSProperties;
  }
  // soft
  return {
    '--glow-color-mid': glow.mid,
    '--glow-color-far': glow.far,
  } as CSSProperties;
}

function getGlowClass(color: AccentColor, glowTier: GlowTier): string | undefined {
  if (glowTier === 'none' || color === 'default') return undefined;
  return glowTier === 'neon' ? 'glow-neon' : 'glow-soft';
}

export function StatBlock({
  label,
  value,
  decimals = 0,
  prefix,
  suffix,
  trend,
  size = 'md',
  color = 'jade',
  glowTier = 'none',
  className,
  labelClassName,
  subtext,
}: StatBlockProps) {
  const config = sizeClasses[size];

  const trendColors: Record<TrendDirection, string> = {
    up: 'text-jade',
    down: 'text-rouge',
    neutral: 'text-text-dim',
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

  const glowStyle = getGlowStyle(color, glowTier);
  const glowClass = getGlowClass(color, glowTier);

  return (
    <div className={cn('flex min-w-0 flex-col', config.gap, className)}>
      <div
        className={cn(
          'text-text-dim font-mono uppercase tracking-wider',
          config.label,
          labelClassName
        )}
      >
        {label}
      </div>

      <div className="flex min-w-0 items-baseline gap-2">
        <span
          className={cn(
            'truncate font-mono font-bold tabular-nums transition-all',
            config.value,
            colorClasses[color],
            glowClass
          )}
          style={glowStyle}
          title={`${prefix ?? ''}${formatValue(value)}${suffix ?? ''}`}
        >
          {prefix}
          {formatValue(value)}
          {suffix}
        </span>

        {trend && (
          <span className={cn('flex items-center gap-1', trendColors[trend.direction])}>
            <span>{trendIcons[trend.direction]}</span>
            <span className={cn('font-mono', config.trend)}>{trend.value}</span>
            {trend.label && <span className="text-text-dim ml-1">{trend.label}</span>}
          </span>
        )}
      </div>

      {subtext && <div className="text-text-dim font-mono text-sm">{subtext}</div>}
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
    2: 'grid-cols-1 min-[480px]:grid-cols-2',
    3: 'grid-cols-1 min-[480px]:grid-cols-2 md:grid-cols-3',
    4: 'grid-cols-2 md:grid-cols-4',
  };

  return <div className={cn('grid gap-4', columnClasses[columns], className)}>{children}</div>;
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
          'via-base-border w-px self-stretch bg-gradient-to-b from-transparent to-transparent',
          className
        )}
      />
    );
  }

  return (
    <div
      className={cn(
        'via-base-border h-px w-full bg-gradient-to-r from-transparent to-transparent',
        className
      )}
    />
  );
}

type MiniStatColor = 'jade' | 'aqua' | 'gold' | 'rouge' | 'default' | 'dim';

interface MiniStatProps {
  label: string;
  value: string | number;
  color?: MiniStatColor;
  className?: string;
}

export function MiniStat({ label, value, color = 'dim', className }: MiniStatProps) {
  const miniColorClasses: Record<MiniStatColor, string> = {
    jade: 'text-jade',
    aqua: 'text-aqua',
    gold: 'text-gold',
    rouge: 'text-rouge',
    default: 'text-text-bright',
    dim: 'text-text',
  };

  return (
    <div className={cn('flex items-center justify-between gap-4', className)}>
      <span className="text-text-dim font-mono text-xs uppercase tracking-wide">{label}</span>
      <span className={cn('font-mono text-sm tabular-nums', miniColorClasses[color])}>{value}</span>
    </div>
  );
}
