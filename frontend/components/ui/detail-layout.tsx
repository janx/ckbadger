import { createContext, useContext, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

/** Which information tier a section sits in; tier-2 headings are quieter (FINAL_REVIEW, TX-16). */
const DetailTierContext = createContext<1 | 2>(1);

/** The developer-evidence tier: one 72 px boundary, and every section heading inside is quiet. */
export function DetailTier2({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <DetailTierContext.Provider value={2}>
      <div data-tier="2" className={cn('mt-[72px] min-w-0 space-y-8', className)}>
        {children}
      </div>
    </DetailTierContext.Provider>
  );
}

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
        {identity && (
          <div className="text-text-dim mt-2 min-w-0 text-xs [overflow-wrap:anywhere]">
            {identity}
          </div>
        )}
        {facts && (
          <dl className="mt-6 grid grid-cols-1 gap-x-12 gap-y-3 sm:grid-cols-2">
            {facts.map(({ label, value }) => (
              <div
                key={label}
                className="grid min-w-0 grid-cols-1 items-baseline gap-x-3 gap-y-1 sm:grid-cols-[minmax(80px,1fr)_minmax(0,2fr)]"
              >
                <dt className="text-text-dim min-w-0 break-words text-xs">{label}</dt>
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
        <DetailTier2>
          {tier2}
          {rawData && (
            <details open={rawOpen} className="border-base-border-subtle border-t pt-4">
              <summary className="text-text-dim hover:text-text cursor-pointer text-xs">
                Raw data
              </summary>
              <div className="mt-4 min-w-0 overflow-x-auto text-xs">{rawData}</div>
            </details>
          )}
        </DetailTier2>
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
  const tier = useContext(DetailTierContext);
  return (
    <section className={cn('min-w-0', className)}>
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <h2
          className={cn(
            'font-medium',
            tier === 2 ? 'text-[13px] text-[#a0a8be]' : 'text-text-bright text-[15px]'
          )}
        >
          {title}
        </h2>
        {actions}
      </div>
      {children}
    </section>
  );
}
