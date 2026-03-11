'use client';

import { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';

interface ChartCardProps {
  title: string;
  href?: string;
  children: ReactNode;
  isLoading?: boolean;
  error?: boolean;
  height?: number;
  className?: string;
}

export function ChartCard({
  title,
  href,
  children,
  isLoading,
  error,
  height = 180,
  className,
}: ChartCardProps) {
  const content = (
    <div
      className={cn(
        'border-base-border bg-base-surface overflow-hidden rounded-lg border',
        'transition-all duration-200',
        href && 'hover:bg-base-elevated hover:border-base-border cursor-pointer',
        className
      )}
    >
      <div className="from-base-elevated/50 border-base-border flex items-center justify-between border-b bg-gradient-to-r to-transparent px-3 py-2">
        <h3 className="text-text font-mono text-sm uppercase tracking-wide">{title}</h3>
        {href && (
          <span className="text-jade font-mono text-xs opacity-0 transition-opacity group-hover:opacity-100">
            VIEW →
          </span>
        )}
      </div>

      <div className="p-3" style={{ minHeight: height }}>
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <div className="w-full space-y-3">
              <div className="bg-base-elevated h-3 w-3/4 animate-pulse rounded" />
              <div className="bg-base-elevated h-3 w-1/2 animate-pulse rounded" />
              <div className="bg-base-elevated h-3 w-2/3 animate-pulse rounded" />
              <div className="bg-base-elevated mt-4 h-20 animate-pulse rounded" />
            </div>
          </div>
        ) : error ? (
          <div className="text-text-dim flex h-full items-center justify-center text-sm">
            Failed to load data
          </div>
        ) : (
          children
        )}
      </div>
    </div>
  );

  if (href) {
    return (
      <Link href={href} className="group block">
        {content}
      </Link>
    );
  }

  return content;
}

interface ChartSectionProps {
  title: string;
  children: ReactNode;
  className?: string;
}

export function ChartSection({ title, children, className }: ChartSectionProps) {
  return (
    <section className={cn('mb-6', className)}>
      <h2 className="text-text-bright mb-4 flex items-center gap-3 font-mono text-lg uppercase tracking-wider">
        <span className="bg-emphasis h-2 w-2 rounded-full" />
        {title}
      </h2>
      <div className="grid gap-4 lg:grid-cols-2 xl:grid-cols-3">{children}</div>
    </section>
  );
}

interface StatCardProps {
  label: string;
  value: ReactNode;
  subValue?: ReactNode;
  trend?: {
    direction: 'up' | 'down' | 'neutral';
    value: string;
  };
  className?: string;
  valueClassName?: string;
}

export function StatCard({
  label,
  value,
  subValue,
  trend,
  className,
  valueClassName,
}: StatCardProps) {
  const trendColors = {
    up: 'text-positive',
    down: 'text-negative',
    neutral: 'text-text-dim',
  };

  const trendIcons = {
    up: '↑',
    down: '↓',
    neutral: '→',
  };

  return (
    <div className={cn('text-center', className)}>
      <div className="text-text-dim font-mono text-xs uppercase tracking-wider">{label}</div>
      <div
        className={cn(
          'text-text-bright mt-2 font-mono text-2xl font-bold tabular-nums',
          valueClassName
        )}
      >
        {value}
      </div>
      {subValue && <div className="text-text mt-1 text-sm">{subValue}</div>}
      {trend && (
        <div className={cn('mt-1 font-mono text-sm', trendColors[trend.direction])}>
          {trendIcons[trend.direction]} {trend.value}
        </div>
      )}
    </div>
  );
}

interface FilterButtonGroupProps {
  options: { label: string; value: string | number | undefined }[];
  selected: string | number | undefined;
  onChange: (value: string | number | undefined) => void;
  className?: string;
}

export function FilterButtonGroup({
  options,
  selected,
  onChange,
  className,
}: FilterButtonGroupProps) {
  return (
    <div className={cn('flex gap-1', className)}>
      {options.map((option) => (
        <button
          key={option.label}
          onClick={() => onChange(option.value)}
          className={cn(
            'rounded px-3 py-1 font-mono text-xs transition-colors',
            selected === option.value
              ? 'bg-emphasis text-base-bg'
              : 'bg-base-elevated text-text-dim hover:bg-base-border hover:text-text'
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
