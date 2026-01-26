'use client';

import { ReactNode, useState } from 'react';
import Link from 'next/link';
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
    <div className={cn('mb-8', className)}>
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          {navigation?.prev && (
            <Link
              href={navigation.prev.href}
              className="hover:text-terminal-green hover:border-terminal-dark rounded border border-slate-800 p-1.5 text-slate-400 transition-colors"
              title={navigation.prev.label || 'Previous'}
            >
              <ChevronLeftIcon className="h-4 w-4" />
            </Link>
          )}

          <div>
            <div className="flex items-center gap-3">
              <h1 className="font-mono text-2xl font-bold text-white">{title}</h1>
              {badge}
            </div>
            {subtitle && <div className="mt-1 text-sm text-slate-500">{subtitle}</div>}
          </div>

          {navigation?.next && (
            <Link
              href={navigation.next.href}
              className="hover:text-terminal-green hover:border-terminal-dark rounded border border-slate-800 p-1.5 text-slate-400 transition-colors"
              title={navigation.next.label || 'Next'}
            >
              <ChevronRightIcon className="h-4 w-4" />
            </Link>
          )}
        </div>

        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </div>

      {hash && (
        <div
          className="group mt-3 inline-flex cursor-pointer items-center gap-2 rounded border border-slate-800 bg-slate-900/50 px-3 py-1.5 transition-colors hover:border-slate-700"
          onClick={handleCopy}
          title="Click to copy"
        >
          <span className="break-all font-mono text-sm text-slate-400 group-hover:text-slate-300">
            {hash}
          </span>
          {copied ? (
            <CheckIcon className="text-terminal-green h-4 w-4 shrink-0" />
          ) : (
            <CopyIcon className="h-4 w-4 shrink-0 text-slate-600 group-hover:text-slate-400" />
          )}
        </div>
      )}
    </div>
  );
}

interface BadgeProps {
  children: ReactNode;
  variant?: 'green' | 'amber' | 'blue' | 'purple' | 'red' | 'gray';
  className?: string;
}

export function Badge({ children, variant = 'gray', className }: BadgeProps) {
  const variantClasses = {
    green: 'bg-green-900/50 text-green-400 border-green-900/50',
    amber: 'bg-amber-900/50 text-amber border-amber-900/50',
    blue: 'bg-blue-900/50 text-blue-400 border-blue-900/50',
    purple: 'bg-purple-900/50 text-purple-400 border-purple-900/50',
    red: 'bg-red-900/50 text-red-400 border-red-900/50',
    gray: 'bg-slate-800 text-slate-400 border-slate-700',
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
