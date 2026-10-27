import type { ReactNode } from 'react';
import { Header } from '@/components/layout/header';
import { ClassMark } from '@/components/ui/class-mark';
import { AppLink } from '@/components/ui/app-link';
import type { AssetClass } from '@/lib/design-tokens';

export interface AssetDetailShellProps {
  title: ReactNode;
  kind: AssetClass;
  breadcrumb: readonly { label: string; href: string }[];
  status?: ReactNode;
  identity?: ReactNode;
  intro?: ReactNode;
  facts?: readonly { label: string; value: ReactNode }[];
  children: ReactNode;
  feed?: ReactNode;
  tier2?: ReactNode;
}
/** The domain and its capacity history are tier one; raw evidence follows a 72px boundary. */
export function AssetDetailShell({
  title,
  kind,
  breadcrumb,
  status,
  identity,
  intro,
  facts,
  children,
  feed,
  tier2,
}: AssetDetailShellProps) {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8 font-mono text-[13px]">
        <div
          data-tier="1"
          className="grid min-w-0 items-start gap-10 lg:grid-cols-[minmax(0,1fr)_260px]"
        >
          <div className="min-w-0 space-y-10">
            <header className="space-y-3">
              <nav
                aria-label="Breadcrumb"
                className="text-text-dim flex flex-wrap items-center gap-2 text-xs"
              >
                {breadcrumb.map((b, i) => (
                  <span key={`${b.href}:${b.label}`}>
                    {i > 0 && <span className="text-text-muted mr-2">/</span>}
                    <AppLink href={b.href}>{b.label}</AppLink>
                  </span>
                ))}
              </nav>
              <div className="flex min-w-0 flex-wrap items-center gap-3">
                <ClassMark kind={kind} size={24} />
                <h1 className="text-text-bright min-w-0 break-words text-[26px] font-medium">
                  {title}
                </h1>
                {status}
              </div>
              {identity && (
                <div className="text-text-dim min-w-0 break-words text-[11px]">{identity}</div>
              )}
              {intro}
            </header>
            {children}
          </div>
          {facts && (
            <aside aria-label="Facts" className="min-w-0 lg:sticky lg:top-24 lg:pt-8">
              <dl className="space-y-5">
                {facts.map((f) => (
                  <div key={f.label}>
                    <dt className="text-text-dim text-[11px]">{f.label}</dt>
                    <dd className="text-text-bright mt-1 min-w-0 break-words tabular-nums">
                      {f.value}
                    </dd>
                  </div>
                ))}
              </dl>
            </aside>
          )}
        </div>
        {feed && <div className="mt-12 min-w-0">{feed}</div>}
        {tier2 && (
          <div data-tier="2" className="mt-[72px] min-w-0 space-y-8">
            {tier2}
          </div>
        )}
      </main>
    </div>
  );
}
export function AssetReadState({
  error,
  loading,
  label,
  retained = false,
}: {
  error: Error | null;
  loading?: boolean;
  label: string;
  retained?: boolean;
}) {
  if (retained && !error) return null;
  return (
    <p role={error ? 'alert' : 'status'} className="text-text-dim py-3 text-xs">
      {error
        ? `Could not ${retained ? 'refresh' : 'load'} ${label}: ${error.message}.${retained ? ' Showing previously loaded data.' : ''}`
        : loading
          ? `Loading ${label}…`
          : `${label} unavailable.`}
    </p>
  );
}
