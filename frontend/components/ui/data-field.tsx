'use client';

import { ReactNode, useState } from 'react';
import { cn } from '@/lib/utils';

interface DataFieldProps {
  label: string;
  children: ReactNode;
  helpText?: string;
  copyValue?: string;
  className?: string;
  labelClassName?: string;
  valueClassName?: string;
  layout?: 'horizontal' | 'vertical';
}

export function DataField({
  label,
  children,
  helpText,
  copyValue,
  className,
  labelClassName,
  valueClassName,
  layout = 'horizontal',
}: DataFieldProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    if (!copyValue) return;
    await navigator.clipboard.writeText(copyValue);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  if (layout === 'vertical') {
    return (
      <div className={cn('flex flex-col gap-1', className)}>
        <div className={cn('flex items-center gap-2', labelClassName)}>
          <span className="text-text-dim font-mono text-xs uppercase tracking-wider">{label}</span>
          {helpText && (
            <span className="text-text-dim hover:text-text cursor-help" title={helpText}>
              <HelpIcon className="h-3.5 w-3.5" />
            </span>
          )}
        </div>
        <div
          className={cn(
            'text-text-bright min-w-0 break-words font-mono text-sm',
            copyValue && 'hover:text-jade cursor-pointer transition-colors',
            valueClassName
          )}
          onClick={copyValue ? handleCopy : undefined}
        >
          {copied ? <span className="text-jade">Copied!</span> : children}
        </div>
      </div>
    );
  }

  return (
    <div
      className={cn(
        'border-base-border/50 flex flex-col gap-2 border-b py-2 last:border-b-0 sm:flex-row sm:items-center sm:justify-between',
        className
      )}
    >
      <div className={cn('flex shrink-0 items-center gap-2', labelClassName)}>
        <span className="text-text-dim text-sm">{label}</span>
        {helpText && (
          <span className="text-text-dim hover:text-text cursor-help" title={helpText}>
            <HelpIcon className="h-4 w-4" />
          </span>
        )}
      </div>
      <div
        className={cn(
          'text-text-bright flex w-full min-w-0 items-center gap-2 break-words text-left font-mono text-sm sm:w-auto sm:justify-end sm:text-right',
          copyValue && 'hover:text-jade group cursor-pointer transition-colors',
          valueClassName
        )}
        onClick={copyValue ? handleCopy : undefined}
      >
        {copied ? (
          <span className="text-jade">Copied!</span>
        ) : (
          <>
            {children}
            {copyValue && (
              <CopyIcon className="group-hover:text-jade text-text-dim h-3.5 w-3.5 opacity-0 transition-opacity group-hover:opacity-100" />
            )}
          </>
        )}
      </div>
    </div>
  );
}

interface DataGridProps {
  children: ReactNode;
  columns?: 1 | 2;
  className?: string;
}

export function DataGrid({ children, columns = 2, className }: DataGridProps) {
  return (
    <div
      className={cn('grid gap-x-8', columns === 2 ? 'md:grid-cols-2' : 'grid-cols-1', className)}
    >
      {children}
    </div>
  );
}

interface DataSectionProps {
  title?: string;
  children: ReactNode;
  className?: string;
}

export function DataSection({ title, children, className }: DataSectionProps) {
  return (
    <div className={cn('', className)}>
      {title && (
        <h3 className="border-base-border text-text-dim mb-3 border-b pb-2 font-mono text-xs uppercase tracking-wider">
          {title}
        </h3>
      )}
      {children}
    </div>
  );
}

function HelpIcon({ className }: { className?: string }) {
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
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <path d="M12 17h.01" />
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
