'use client';

import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import { usePathname } from '@/src/navigation';
import { useNotFoundEvidence, type NotFoundEntity } from '@/lib/not-found-evidence';
import { useDocumentTitle } from '@/lib/use-document-title';

const NOT_FOUND_TITLE = ['Not found'] as const;

/**
 * The 404's verse (rubbing × Song print, the voice): three comment lines, the last the only place
 * the meme is quoted. A route that names no page asked nothing of the chain.
 */
export const ROUTE_VERSE = [
  'no page at this path.',
  'the chain was not asked; nothing on it is lost.',
  "honey badger don't care.",
] as const;

/**
 * An entity the page names but the local index does not hold: the index was asked (the facts say
 * which endpoint answered 404), so the route's second line would not be true here.
 */
export function entityVerse(entity: NotFoundEntity): readonly string[] {
  return [
    `no ${entity.kind.replace('-', ' ')} at this address.`,
    'the local index was asked; it holds none.',
    ROUTE_VERSE[2],
  ];
}

const RETURNS: Partial<Record<NotFoundEntity['kind'], { href: string; label: string }>> = {
  transaction: { href: '/transactions', label: 'transactions' },
  block: { href: '/blocks', label: 'blocks' },
  uncle: { href: '/blocks', label: 'blocks' },
  address: { href: '/activities', label: 'activities' },
  token: { href: '/inventory/tokens', label: 'tokens' },
  object: { href: '/inventory/objects', label: 'objects' },
  collection: { href: '/inventory/objects', label: 'objects' },
  identity: { href: '/inventory/identities', label: 'identities' },
  'fiber-channel': { href: '/fiber/channels', label: 'Fiber channels' },
  script: { href: '/scripts', label: 'scripts' },
  'script-family': { href: '/scripts', label: 'scripts' },
  fork: { href: '/forks', label: 'forks' },
};

interface NotFoundPageProps {
  entity?: NotFoundEntity;
  errMessage?: string;
  /** Drawn in place of AppFrame (the frame route's error boundary): its own header and footer. */
  standalone?: boolean;
}

export function NotFoundPage({ entity, errMessage, standalone = false }: NotFoundPageProps = {}) {
  const pathname = usePathname();
  const lookup = useNotFoundEvidence(entity);
  useDocumentTitle(NOT_FOUND_TITLE);
  const back = entity ? RETURNS[entity.kind] : undefined;

  return (
    <ErrorPageShell
      status="Not found · 404"
      path={pathname}
      verse={entity ? entityVerse(entity) : ROUTE_VERSE}
      standalone={standalone}
    >
      {entity && (
        <span className="min-w-0 break-all">
          checked <span>{entity.checkedEndpoint}</span> → 404 · requested{' '}
          <span className="break-all">{entity.identifier}</span>
        </span>
      )}
      {lookup.status === 'loading' && (
        <span role="status">checking the local /search index for an exact alternative…</span>
      )}
      {lookup.status === 'checked' && lookup.evidence && (
        <span className="min-w-0">
          <span className="break-all">{lookup.evidence.finding}</span>
          {lookup.evidence.alternative && (
            <>
              {' '}
              <Link
                href={lookup.evidence.alternative.href}
                className="text-jade focus-visible:outline-jade whitespace-nowrap underline-offset-4 hover:underline focus-visible:outline"
              >
                → open {lookup.evidence.alternative.label}
              </Link>
            </>
          )}
        </span>
      )}
      {lookup.status === 'error' && lookup.error && (
        <span role="status" className="min-w-0 break-all">
          the separate /search check could not complete: {lookup.error}
        </span>
      )}
      {errMessage && <span className="min-w-0 break-all">{errMessage}</span>}
      <nav aria-label="Find another route" className="flex flex-wrap gap-x-4 gap-y-1">
        <Link
          href="/"
          className="text-jade focus-visible:outline-jade whitespace-nowrap underline-offset-4 hover:underline focus-visible:outline"
        >
          ← back to the explorer
        </Link>
        {back ? (
          <Link
            href={back.href}
            className="text-text hover:text-jade focus-visible:outline-jade whitespace-nowrap focus-visible:outline"
          >
            ← {back.label}
          </Link>
        ) : (
          <>
            <Link
              href="/blocks"
              className="text-text hover:text-jade focus-visible:outline-jade whitespace-nowrap focus-visible:outline"
            >
              browse blocks
            </Link>
            <Link
              href="/inventory/tokens"
              className="text-text hover:text-jade focus-visible:outline-jade whitespace-nowrap focus-visible:outline"
            >
              inventory
            </Link>
          </>
        )}
      </nav>
    </ErrorPageShell>
  );
}
