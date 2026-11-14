'use client';

import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface BadgeProps {
  children: ReactNode;
  variant?: 'green' | 'gold' | 'blue' | 'purple' | 'red' | 'gray' | 'neutral';
  className?: string;
}

export function Badge({ children, variant = 'gray', className }: BadgeProps) {
  const variantClasses = {
    green: 'bg-positive/10 text-positive border-positive/20',
    gold: 'bg-warning/10 text-warning border-warning/20',
    blue: 'bg-info/10 text-info border-info/20',
    purple: 'bg-info/10 text-info-bright border-info/20',
    red: 'bg-negative/10 text-negative border-negative/20',
    gray: 'bg-base-elevated text-text-dim border-base-border',
    neutral: 'bg-base-elevated/70 text-text border-base-border',
  };

  return (
    <span
      className={cn(
        'text-small inline-flex items-center rounded border px-2 py-0.5 font-mono',
        variantClasses[variant],
        className
      )}
    >
      {children}
    </span>
  );
}
