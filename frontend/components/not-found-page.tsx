'use client';

import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import { ErrorVessels, VesselsCaption } from '@/components/error-vessels';
import { usePathname } from '@/src/navigation';
import { useNotFoundEvidence, type NotFoundEntity } from '@/lib/not-found-evidence';

interface NotFoundPageProps {
  entity?: NotFoundEntity;
  errMessage?: string;
  /** React Router errorElement replaces AppFrame, including its footer. */
  footer?: boolean;
}

export function NotFoundPage({ entity, errMessage, footer = false }: NotFoundPageProps = {}) {
  const pathname = usePathname();
  const lookup = useNotFoundEvidence(entity);
  const title = entity ? (
    lookup.evidence?.alternative ? (
      <>
        <span className="block text-[clamp(3rem,8vw,7rem)]">A name,</span>{' '}
        <span className="block text-[clamp(2rem,7vw,4.25rem)]">
          not a {entity.kind.replace('-', ' ')}.
        </span>
      </>
    ) : (
      <>
        <span className="block text-[clamp(2.5rem,7vw,5rem)]">An unanswered</span>{' '}
        <span className="block text-[clamp(3rem,8vw,7rem)]">name.</span>
      </>
    )
  ) : (
    <>
      <span className="block text-[clamp(2.5rem,7vw,5.5rem)]">The road ends</span>{' '}
      <span className="block text-[clamp(3rem,8vw,7rem)]">here.</span>
    </>
  );

  return (
    <ErrorPageShell
      status="Not found · 404"
      path={pathname}
      title={title}
      footer={footer}
      artwork={(avoidRef) => <ErrorVessels avoidRef={avoidRef} />}
      caption={<VesselsCaption />}
    >
      <p
        className="text-text-bright max-w-xl text-[clamp(1.25rem,3vw,1.7rem)] leading-snug"
        style={{ fontFamily: 'var(--font-error-display)' }}
      >
        {entity
          ? `This explorer could not open the ${entity.kind} at this address.`
          : 'This route does not lead to a page.'}
      </p>
      {entity && (
        <dl className="border-base-border-subtle mt-8 min-w-0 border-t font-mono text-[12px]">
          <div className="border-base-border-subtle grid min-w-0 grid-cols-[5rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[8rem_minmax(0,1fr)]">
            <dt className="text-text-dim">requested</dt>
            <dd className="text-text min-w-0 break-all">{entity.identifier}</dd>
          </div>
          <div className="border-base-border-subtle grid min-w-0 grid-cols-[5rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[8rem_minmax(0,1fr)]">
            <dt className="text-text-dim">checked</dt>
            <dd className="text-text min-w-0 break-all">{entity.checkedEndpoint}</dd>
          </div>
        </dl>
      )}
      {lookup.status === 'loading' && (
        <p role="status" className="text-text-dim mt-4 max-w-xl text-[12px]">
          Checking the local /search index for an exact alternative…
        </p>
      )}
      {lookup.status === 'checked' && lookup.evidence && (
        <div className="text-text-dim mt-4 min-w-0 max-w-xl text-[13px] leading-relaxed">
          <p className="break-all">{lookup.evidence.finding}</p>
          {lookup.evidence.alternative && (
            <Link
              href={lookup.evidence.alternative.href}
              className="text-jade focus-visible:outline-jade mt-3 inline-block break-all underline-offset-4 hover:underline focus-visible:outline"
            >
              Open {lookup.evidence.alternative.label} →
            </Link>
          )}
        </div>
      )}
      {lookup.status === 'error' && lookup.error && (
        <p role="status" className="text-text-dim mt-4 min-w-0 max-w-xl break-all text-[12px]">
          The separate /search check could not complete: {lookup.error}
        </p>
      )}
      {errMessage && (
        <p className="text-text-dim mt-4 max-w-xl break-all font-mono text-[12px]">{errMessage}</p>
      )}
      <nav
        aria-label="Find another route"
        className="mt-8 flex flex-wrap gap-3 font-mono text-[12px]"
      >
        <Link
          href="/"
          className="border-jade text-jade hover:bg-jade/10 focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
        >
          Back to the explorer →
        </Link>
        <Link
          href="/blocks"
          className="border-base-border text-text hover:text-jade focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
        >
          Browse blocks
        </Link>
        <Link
          href="/inventory/tokens"
          className="border-base-border text-text hover:text-jade focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
        >
          Inventory
        </Link>
      </nav>
    </ErrorPageShell>
  );
}
