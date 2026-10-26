import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

export interface DetailFact {
  label: string;
  value: ReactNode;
}
export interface DetailLayoutProps {
  title: ReactNode;
  kind?: 'entity' | 'topic' | 'index';
  identity?: ReactNode;
  status?: ReactNode;
  actions?: ReactNode;
  facts?: readonly DetailFact[];
  children: ReactNode;
  /** Developer evidence. Omit on pages that are developer pages throughout. */
  tier2?: ReactNode;
  rawData?: ReactNode;
  rawOpen?: boolean;
  className?: string;
}

/** Domain first, with one enforced 72 px boundary before developer evidence. */
export function DetailLayout({
  title,
  kind = 'entity',
  identity,
  status,
  actions,
  facts,
  children,
  tier2,
  rawData,
  rawOpen,
  className,
}: DetailLayoutProps) {
  return (
    <div className={cn('min-w-0 font-mono text-[13px]', className)}>
      <header className="mb-8">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-baseline gap-3">
            <h1
              className={cn(
                'text-text-bright break-words font-medium',
                kind === 'index' ? 'text-[22px]' : 'text-[26px]'
              )}
            >
              {title}
            </h1>
            {status}
          </div>
          {actions}
        </div>
        {identity && <div className="text-text-dim mt-2 break-all text-xs">{identity}</div>}
        {facts && (
          <dl className="mt-6 grid grid-cols-1 gap-x-12 gap-y-3 sm:grid-cols-2">
            {facts.map(({ label, value }) => (
              <div
                key={label}
                className="grid min-w-0 grid-cols-[minmax(80px,1fr)_minmax(0,2fr)] items-baseline gap-3"
              >
                <dt className="text-text-dim text-xs">{label}</dt>
                <dd className="text-text-bright min-w-0 break-words tabular-nums">{value}</dd>
              </div>
            ))}
          </dl>
        )}
      </header>
      <div data-tier="1" className="min-w-0 space-y-8">
        {children}
      </div>
      {(tier2 || rawData) && (
        <div data-tier="2" className="mt-[72px] min-w-0 space-y-8">
          {tier2}
          {rawData && (
            <details open={rawOpen} className="border-base-border-subtle border-t pt-4">
              <summary className="text-text-dim hover:text-text cursor-pointer text-xs">
                Raw data
              </summary>
              <div className="mt-4 min-w-0 overflow-x-auto text-xs">{rawData}</div>
            </details>
          )}
        </div>
      )}
    </div>
  );
}

export function DetailSection({
  title,
  actions,
  children,
  className,
}: {
  title: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn('min-w-0', className)}>
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <h2 className="text-text-bright text-[15px] font-medium">{title}</h2>
        {actions}
      </div>
      {children}
    </section>
  );
}
