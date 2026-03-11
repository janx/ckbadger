'use client';

import { ReactNode, useState } from 'react';
import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';

interface PageHeaderProps {
  title: string;
  subtitle?: ReactNode;
  hash?: string;
  badge?: ReactNode;
  navigation?: {
    prev?: { href: string; label?: string };
    next?: { href: string; label?: string };
  };
  actions?: ReactNode;
  className?: string;
}

export function PageHeader({
  title,
  subtitle,
  hash,
  badge,
  navigation,
  actions,
  className,
}: PageHeaderProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    if (!hash) return;
    await navigator.clipboard.writeText(hash);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className={cn('mb-4', className)}>
      <div className="flex flex-wrap items-start justify-between gap-4 sm:items-center">
        <div className="flex min-w-0 items-center gap-3">
          {navigation?.prev && (
            <Link
              href={navigation.prev.href}
              className="hover:text-jade hover:border-jade-dim border-base-border text-text-dim rounded border p-1.5 transition-colors"
              title={navigation.prev.label || 'Previous'}
            >
              <ChevronLeftIcon className="h-4 w-4" />
            </Link>
          )}

          <div>
            <div className="flex min-w-0 flex-wrap items-center gap-3">
              <h1 className="text-text-bright break-words font-mono text-2xl font-bold">{title}</h1>
              {badge}
            </div>
            {subtitle && <div className="text-text-dim mt-1 break-words text-sm">{subtitle}</div>}
          </div>

          {navigation?.next && (
            <Link
              href={navigation.next.href}
              className="hover:text-jade hover:border-jade-dim border-base-border text-text-dim rounded border p-1.5 transition-colors"
              title={navigation.next.label || 'Next'}
            >
              <ChevronRightIcon className="h-4 w-4" />
            </Link>
          )}
        </div>

        {actions && (
          <div className="flex max-w-full flex-wrap items-center justify-end gap-2">{actions}</div>
        )}
      </div>

      {hash && (
        <div
          className="border-base-border bg-base-surface/50 hover:border-base-border group mt-3 inline-flex cursor-pointer items-center gap-2 rounded border px-3 py-1.5 transition-colors"
          onClick={handleCopy}
          title="Click to copy"
        >
          <span className="text-text group-hover:text-text break-all font-mono text-sm">
            {hash}
          </span>
          {copied ? (
            <CheckIcon className="text-emphasis h-4 w-4 shrink-0" />
          ) : (
            <CopyIcon className="text-text-dim group-hover:text-text h-4 w-4 shrink-0" />
          )}
        </div>
      )}
    </div>
  );
}

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
        'inline-flex items-center rounded border px-2 py-0.5 font-mono text-xs',
        variantClasses[variant],
        className
      )}
    >
      {children}
    </span>
  );
}

function ChevronLeftIcon({ className }: { className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

function ChevronRightIcon({ className }: { className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="m9 18 6-6-6-6" />
    </svg>
  );
}

function CopyIcon({ className }: { className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </svg>
  );
}

function CheckIcon({ className }: { className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}
