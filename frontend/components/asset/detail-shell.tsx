import { useId, type ReactNode } from 'react';
import { Header } from '@/components/layout/header';
import { STICKY_RAIL } from '@/components/layout/sticky-header';
import { ClassMark } from '@/components/ui/class-mark';
import { AppLink } from '@/components/ui/app-link';
import { DetailTier2 } from '@/components/ui/detail-layout';
import type { AssetClass } from '@/lib/design-tokens';
import { readErrorText } from '@/lib/read-error';
import { cn } from '@/lib/utils';
import {
  documentTitleParts,
  useDocumentTitle,
  type DocumentTitleProps,
} from '@/lib/use-document-title';

interface AssetDetailShellBaseProps {
  kind: AssetClass;
  breadcrumb: readonly { label: string; href: string }[];
  status?: ReactNode;
  identity?: ReactNode;
  /**
   * Hierarchy v3 (R13): the page's one-sentence lede. Given, the title row, the identity and the
   * lede form the page's hero `<section>` (named by the title) and `intro` follows it; omitted, the
   * title block is laid out as before.
   */
  lede?: ReactNode;
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
  /**
   * Consistency round C7 (Main.dc.html): the page's figure and picture — the display number, a
   * composition block, the history chart — inside the hero section after the lede (R13). Needs
   * `lede`.
   */
  hero?: ReactNode;
  /**
   * Consistency round C7: the metrics row (`AssetMetrics`) under the hero, in place of the facts
   * rail. A page that passes `metrics` and no `facts` is one column: no 260 px rail track.
   */
  metrics?: ReactNode;
  /**
   * Lane U (U5): the page's actions (an external "Omiga ↗" button-link), at the right end of the
   * h1 row, where the boards put page actions (R13: h1 and range buttons share one row). Omitted,
   * the row is as before.
   */
  actions?: ReactNode;
}
/**
 * `title` is the h1; `documentTitle` (C1) names the browser tab — asset pages give
 * `[displayName, kind]` (`['RGB++', 'token']`, `['.cell deeds', 'cluster']`).
 */
export type AssetDetailShellProps = AssetDetailShellBaseProps & DocumentTitleProps;
/** The domain and its capacity history are tier one; raw evidence follows a 72px boundary. */
export function AssetDetailShell({
  title,
  documentTitle,
  kind,
  breadcrumb,
  status,
  identity,
  lede,
  intro,
  facts,
  children,
  feed,
  tier2,
  media,
  railAfterHeader = false,
  hero,
  metrics,
  actions,
}: AssetDetailShellProps) {
  if (hero && !lede) throw new Error('AssetDetailShell: a hero figure needs the lede it follows');
  useDocumentTitle(documentTitleParts(title, documentTitle));
  const titleId = useId();
  const titleRow = (
    <div className="flex min-w-0 flex-wrap items-center gap-3">
      <ClassMark kind={kind} size={24} />
      <h1
        id={lede ? titleId : undefined}
        className="text-text-bright text-entity min-w-0 break-words font-medium"
      >
        {title}
      </h1>
      {status}
    </div>
  );
  const named = (
    <>
      {actions ? (
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-x-4 gap-y-2">
          {titleRow}
          <div data-role="page-actions" className="flex shrink-0 items-center gap-2">
            {actions}
          </div>
        </div>
      ) : (
        titleRow
      )}
      {identity && <div className="text-text-dim text-meta min-w-0 break-words">{identity}</div>}
    </>
  );
  const titleBlock = lede ? (
    <>
      <section aria-labelledby={titleId} className="min-w-0 space-y-3">
        {named}
        <div data-role="lede">{lede}</div>
        {hero && (
          <div data-role="hero" className="min-w-0 pt-6">
            {hero}
          </div>
        )}
      </section>
      {intro && <div data-role="intro">{intro}</div>}
    </>
  ) : (
    <>
      {named}
      {intro && <div data-role="intro">{intro}</div>}
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
        'min-w-0 lg:pt-8',
        STICKY_RAIL,
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
          className={cn(
            'grid min-w-0 items-start gap-10',
            facts && 'lg:grid-cols-[minmax(0,1fr)_260px]'
          )}
        >
          {railAfterHeader ? (
            <>
              {headerEl}
              {asideEl}
              <div data-tier1-body className="min-w-0 space-y-10 lg:col-start-1 lg:row-start-2">
                {metrics}
                {children}
              </div>
            </>
          ) : (
            <>
              <div className="min-w-0 space-y-10">
                {headerEl}
                {metrics}
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
/** One metric of the asset metrics row (C7, R13). */
export interface AssetMetric {
  label: string;
  value: ReactNode;
  /** The inline dim note after the value: "· 748 hold one". */
  note?: ReactNode;
  /** `body` for a value that is a name or an address (the board's 13 px); default `section`. */
  size?: 'section' | 'body';
  /** What the row leaves out of sight (a full date, an exact amount), on hover. */
  title?: string;
}
/**
 * Consistency round C7: the facts rail as one row under the hero (R13: a meta label over a
 * section-size value and its inline dim note) — one column on a phone, two from sm, one grid row
 * from lg.
 */
export function AssetMetrics({ items }: { items: readonly AssetMetric[] }) {
  return (
    <section aria-label="Metrics" data-role="metrics" className="min-w-0">
      <dl className="grid min-w-0 grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2 lg:auto-cols-[minmax(0,1fr)] lg:grid-flow-col lg:grid-cols-none">
        {items.map((m) => (
          <div key={m.label} className="min-w-0" title={m.title}>
            <dt className="text-text-dim text-meta">{m.label}</dt>
            <dd
              className={cn(
                'text-text-bright mt-0.5 min-w-0 break-words tabular-nums',
                m.size === 'body' ? 'text-body' : 'text-section'
              )}
            >
              {m.value}
              {m.note && (
                <span data-role="metric-note" className="text-text-dim text-small">
                  {' · '}
                  <span>{m.note}</span>
                </span>
              )}
            </dd>
          </div>
        ))}
      </dl>
    </section>
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
