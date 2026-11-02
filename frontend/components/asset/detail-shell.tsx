import type { ReactNode } from 'react';
import { Header } from '@/components/layout/header';
import { ClassMark } from '@/components/ui/class-mark';
import { AppLink } from '@/components/ui/app-link';
import { DetailTier2 } from '@/components/ui/detail-layout';
import type { AssetClass } from '@/lib/design-tokens';
import { readErrorText } from '@/lib/read-error';
import { cn } from '@/lib/utils';

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
  /**
   * OB-14 (Object.dc.html): the subject's media, in a 300 px column beside the title block (under
   * the breadcrumb). On a one-column page it follows the title block, so the name, status and
   * summary are read first (responsive review B#23).
   */
  media?: ReactNode;
  /**
   * F2 identities (round 2, ID-C11): put the facts rail right after the header in reading order,
   * so a one-column (390 px) page shows them before the long tier-1 sections; from lg up the
   * rail still sits beside the header and tier 1.
   */
  railAfterHeader?: boolean;
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
  media,
  railAfterHeader = false,
}: AssetDetailShellProps) {
  const titleBlock = (
    <>
      <div className="flex min-w-0 flex-wrap items-center gap-3">
        <ClassMark kind={kind} size={24} />
        <h1 className="text-text-bright text-entity min-w-0 break-words font-medium">{title}</h1>
        {status}
      </div>
      {identity && <div className="text-text-dim text-meta min-w-0 break-words">{identity}</div>}
      {intro}
    </>
  );
  const headerEl = (
    <header
      className={railAfterHeader ? 'min-w-0 space-y-3 lg:col-start-1 lg:row-start-1' : 'space-y-3'}
    >
      <nav
        aria-label="Breadcrumb"
        className="text-text-dim text-small flex flex-wrap items-center gap-2"
      >
        {breadcrumb.map((b, i) => (
          <span key={`${b.href}:${b.label}`}>
            {i > 0 && <span className="text-text-ghost mr-2">/</span>}
            <AppLink href={b.href}>{b.label}</AppLink>
          </span>
        ))}
      </nav>
      {media ? (
        <div className="grid min-w-0 items-start gap-8 md:grid-cols-[300px_minmax(0,1fr)]">
          <div className="min-w-0 space-y-3 md:col-start-2 md:row-start-1">{titleBlock}</div>
          <div data-hero-media className="min-w-0 md:col-start-1 md:row-start-1">
            {media}
          </div>
        </div>
      ) : (
        titleBlock
      )}
    </header>
  );
  const asideEl = facts && (
    <aside
      aria-label="Facts"
      className={cn(
        'min-w-0 lg:sticky lg:top-24 lg:pt-8',
        railAfterHeader && 'lg:col-start-2 lg:row-span-2 lg:row-start-1'
      )}
    >
      <dl className="space-y-5">
        {facts.map((f) => (
          <div key={f.label}>
            <dt className="text-text-dim text-meta">{f.label}</dt>
            <dd className="text-text-bright mt-1 min-w-0 break-words tabular-nums">{f.value}</dd>
          </div>
        ))}
      </dl>
    </aside>
  );
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="text-body container mx-auto min-w-0 px-4 py-8 font-mono">
        <div
          data-tier="1"
          className="grid min-w-0 items-start gap-10 lg:grid-cols-[minmax(0,1fr)_260px]"
        >
          {railAfterHeader ? (
            <>
              {headerEl}
              {asideEl}
              <div data-tier1-body className="min-w-0 space-y-10 lg:col-start-1 lg:row-start-2">
                {children}
              </div>
            </>
          ) : (
            <>
              <div className="min-w-0 space-y-10">
                {headerEl}
                {children}
              </div>
              {asideEl}
            </>
          )}
        </div>
        {feed && <div className="mt-12 min-w-0">{feed}</div>}
        {tier2 && <DetailTier2>{tier2}</DetailTier2>}
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
    // F2 identities (round 2, ID-COL-1): a gateway failure is stated by its status, never the
    // proxy's internal URL, and the line breaks anywhere so no long token widens a 390 px page.
    <p
      role={error ? 'alert' : 'status'}
      className="text-text-dim text-small py-3 [overflow-wrap:anywhere]"
    >
      {error
        ? `Could not ${retained ? 'refresh' : 'load'} ${label}: ${readErrorText(error)}.${retained ? ' Showing previously loaded data.' : ''}`
        : loading
          ? `Loading ${label}…`
          : `${label} unavailable.`}
    </p>
  );
}
