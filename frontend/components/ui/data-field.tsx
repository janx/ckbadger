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
          <span className="font-mono text-xs uppercase tracking-wider text-slate-500">{label}</span>
          {helpText && (
            <span className="cursor-help text-slate-500 hover:text-slate-400" title={helpText}>
              <HelpIcon className="h-3.5 w-3.5" />
            </span>
          )}
        </div>
        <div
          className={cn(
            'font-mono text-sm text-white',
            copyValue && 'hover:text-terminal-green cursor-pointer transition-colors',
            valueClassName
          )}
          onClick={copyValue ? handleCopy : undefined}
        >
          {copied ? <span className="text-terminal-green">Copied!</span> : children}
        </div>
      </div>
    );
  }

  return (
    <div
      className={cn(
        'flex items-center justify-between border-b border-slate-800/50 py-3 last:border-b-0',
        className
      )}
    >
      <div className={cn('flex items-center gap-2', labelClassName)}>
        <span className="text-sm text-slate-500">{label}</span>
        {helpText && (
          <span className="cursor-help text-slate-500 hover:text-slate-400" title={helpText}>
            <HelpIcon className="h-4 w-4" />
          </span>
        )}
      </div>
      <div
        className={cn(
          'flex items-center gap-2 text-right font-mono text-sm text-white',
          copyValue && 'hover:text-terminal-green group cursor-pointer transition-colors',
          valueClassName
        )}
        onClick={copyValue ? handleCopy : undefined}
      >
        {copied ? (
          <span className="text-terminal-green">Copied!</span>
        ) : (
          <>
            {children}
            {copyValue && (
              <CopyIcon className="group-hover:text-terminal-green h-3.5 w-3.5 text-slate-500 opacity-0 transition-opacity group-hover:opacity-100" />
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
        <h3 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
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
