'use client';

import { ReactNode } from 'react';
import Link from 'next/link';
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
        'overflow-hidden rounded-lg border border-slate-800 bg-slate-900',
        'transition-all duration-200',
        href && 'hover:bg-slate-850 cursor-pointer hover:border-slate-700',
        className
      )}
    >
      <div className="from-slate-850/50 flex items-center justify-between border-b border-slate-800 bg-gradient-to-r to-transparent px-4 py-3">
        <h3 className="font-mono text-sm uppercase tracking-wide text-slate-300">{title}</h3>
        {href && (
          <span className="text-terminal-green font-mono text-xs opacity-0 transition-opacity group-hover:opacity-100">
            VIEW →
          </span>
        )}
      </div>

      <div className="p-4" style={{ minHeight: height }}>
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <div className="w-full space-y-3">
              <div className="h-3 w-3/4 animate-pulse rounded bg-slate-800" />
              <div className="h-3 w-1/2 animate-pulse rounded bg-slate-800" />
              <div className="h-3 w-2/3 animate-pulse rounded bg-slate-800" />
              <div className="mt-4 h-20 animate-pulse rounded bg-slate-800" />
            </div>
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center text-sm text-slate-500">
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
    <section className={cn('mb-10', className)}>
      <h2 className="mb-4 flex items-center gap-3 font-mono text-lg uppercase tracking-wider text-white">
        <span className="bg-terminal-green h-2 w-2 rounded-full" />
        {title}
      </h2>
      <div className="grid gap-6 lg:grid-cols-2 xl:grid-cols-3">{children}</div>
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
}

export function StatCard({ label, value, subValue, trend, className }: StatCardProps) {
  const trendColors = {
    up: 'text-terminal-green',
    down: 'text-red-400',
    neutral: 'text-slate-500',
  };

  const trendIcons = {
    up: '↑',
    down: '↓',
    neutral: '→',
  };

  return (
    <div className={cn('text-center', className)}>
      <div className="font-mono text-xs uppercase tracking-wider text-slate-500">{label}</div>
      <div className="mt-2 font-mono text-2xl font-bold tabular-nums text-white">{value}</div>
      {subValue && <div className="mt-1 text-sm text-slate-400">{subValue}</div>}
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
              ? 'bg-terminal-green text-slate-950'
              : 'bg-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-300'
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
