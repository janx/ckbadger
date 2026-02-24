'use client';

import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

type IndicatorStatus = 'active' | 'warning' | 'inactive' | 'none';

interface TerminalPanelProps {
  children: ReactNode;
  className?: string;
  variant?: 'default' | 'elevated' | 'inset';
  glow?: boolean;
}

export function TerminalPanel({
  children,
  className,
  variant = 'default',
  glow = false,
}: TerminalPanelProps) {
  const variantClasses = {
    default: 'bg-slate-900 border-slate-800',
    elevated: 'bg-slate-850 border-slate-700',
    inset: 'bg-slate-950 border-slate-800 shadow-terminal-inset',
  };

  return (
    <div
      className={cn(
        'relative overflow-hidden rounded-lg border transition-shadow duration-300',
        variantClasses[variant],
        glow && 'hover:shadow-terminal-glow',
        className
      )}
    >
      <div className="pointer-events-none absolute inset-0">
        <div
          className="absolute inset-0"
          style={{
            background:
              'repeating-linear-gradient(0deg, rgba(0,0,0,0.03) 0px, rgba(0,0,0,0.03) 1px, transparent 1px, transparent 2px)',
          }}
        />
      </div>

      <div className="relative z-10">{children}</div>
    </div>
  );
}

interface TerminalPanelHeaderProps {
  children: ReactNode;
  className?: string;
  indicator?: IndicatorStatus;
  actions?: ReactNode;
}

export function TerminalPanelHeader({
  children,
  className,
  indicator = 'none',
  actions,
}: TerminalPanelHeaderProps) {
  const indicatorClasses: Record<IndicatorStatus, string> = {
    active: 'indicator-light',
    warning: 'indicator-light indicator-light-amber',
    inactive: 'indicator-light indicator-light-static opacity-30',
    none: 'hidden',
  };

  return (
    <div
      className={cn(
        'flex flex-wrap items-start justify-between gap-3 px-4 py-3 sm:items-center',
        'border-b border-slate-800',
        'from-slate-850/50 bg-gradient-to-r to-transparent',
        className
      )}
    >
      <div className="flex min-w-0 items-center gap-3">
        <div className={indicatorClasses[indicator]} />
        <div className="min-w-0 break-words font-mono text-sm uppercase tracking-wider text-slate-400">
          {children}
        </div>
      </div>

      {actions && (
        <div className="flex max-w-full flex-wrap items-center justify-end gap-2">{actions}</div>
      )}
    </div>
  );
}

interface TerminalPanelContentProps {
  children: ReactNode;
  className?: string;
  padding?: 'none' | 'sm' | 'md' | 'lg';
}

export function TerminalPanelContent({
  children,
  className,
  padding = 'md',
}: TerminalPanelContentProps) {
  const paddingClasses = {
    none: '',
    sm: 'p-2',
    md: 'p-4',
    lg: 'p-6',
  };

  return <div className={cn(paddingClasses[padding], className)}>{children}</div>;
}

interface TerminalPanelFooterProps {
  children: ReactNode;
  className?: string;
}

export function TerminalPanelFooter({ children, className }: TerminalPanelFooterProps) {
  return (
    <div
      className={cn(
        'border-t border-slate-800 px-4 py-3',
        'to-slate-850/30 bg-gradient-to-r from-transparent',
        className
      )}
    >
      {children}
    </div>
  );
}

interface TerminalDividerProps {
  className?: string;
  label?: string;
}

export function TerminalDivider({ className, label }: TerminalDividerProps) {
  if (label) {
    return (
      <div className={cn('flex items-center gap-3 py-2', className)}>
        <div className="h-px flex-1 bg-gradient-to-r from-transparent via-slate-700 to-transparent" />
        <span className="font-mono text-xs uppercase tracking-widest text-slate-500">{label}</span>
        <div className="h-px flex-1 bg-gradient-to-r from-transparent via-slate-700 to-transparent" />
      </div>
    );
  }

  return (
    <div
      className={cn(
        'h-px bg-gradient-to-r from-transparent via-slate-700 to-transparent',
        className
      )}
    />
  );
}

interface TerminalRowProps {
  children: ReactNode;
  className?: string;
  hoverable?: boolean;
}

export function TerminalRow({ children, className, hoverable = true }: TerminalRowProps) {
  return (
    <div
      className={cn(
        'border-b border-slate-800/50 px-4 py-3 last:border-b-0',
        hoverable && 'row-scan hover:bg-slate-850/50 transition-colors',
        className
      )}
    >
      {children}
    </div>
  );
}
