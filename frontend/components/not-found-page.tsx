'use client';

import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import type { SignLines } from '@/components/error-sign';
import { usePathname } from '@/src/navigation';
import { HASH32 } from '@/lib/night/bits';
import { useNotFoundEvidence, type NotFoundEntity } from '@/lib/not-found-evidence';
import { useDocumentTitle } from '@/lib/use-document-title';

const NOT_FOUND_TITLE = ['Not found'] as const;

/** A route that names no page: existence is proof of work; this road never was. */
const ROUTE_READINGS: readonly SignLines[] = [
  ['all that is', 'was worked for.', 'this was not.'],
  ['nothing is lost,', 'for nothing', 'was ever here.'],
  ['emptiness', 'is capacity,', 'not loss.'],
  ['time confirms.', 'it does not', 'invent.'],
];
/** An entity known as something else: the hash is a fact, the name it was asked by an opinion. */
const NAMED_READINGS: readonly SignLines[] = [
  ['a hash', 'is a fact.', 'a name, a guess.'],
  ['what it is', 'outlasts', 'its name.'],
  ['known', 'by what it is,', 'not by its name.'],
  ['one hash', 'names one thing.', 'not this one.'],
];
/** An entity nothing answers: common knowledge is what all hold; absence here is not denial. */
const UNANSWERED_READINGS: readonly SignLines[] = [
  ['known to some,', 'perhaps.', 'to all, not yet.'],
  ['the chain keeps', 'what was proven.', 'this, unproven.'],
  ['unknown here', 'is not', 'untrue.'],
  ['silence', 'is not', 'denial.'],
];

const OUTPOINT = /^(0x[0-9a-fA-F]{64})-(0|[1-9][0-9]*)$/;

/** The hash the page is about, for the night's first grid: a 32-byte hash, or an out point's. */
export function entityHash(entity: NotFoundEntity | undefined): string | null {
  if (!entity) return null;
  if (HASH32.test(entity.identifier)) return entity.identifier.toLowerCase();
  const outpoint = OUTPOINT.exec(entity.identifier);
  return outpoint ? outpoint[1].toLowerCase() : null;
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
  /** React Router errorElement replaces AppFrame, including its footer. */
  footer?: boolean;
}

export function NotFoundPage({ entity, errMessage, footer = false }: NotFoundPageProps = {}) {
  const pathname = usePathname();
  const lookup = useNotFoundEvidence(entity);
  useDocumentTitle(NOT_FOUND_TITLE);
  const readings = entity
    ? lookup.evidence?.alternative
      ? NAMED_READINGS
      : UNANSWERED_READINGS
    : ROUTE_READINGS;
  const back = entity ? RETURNS[entity.kind] : undefined;

  return (
    <ErrorPageShell
      status="Not found · 404"
      path={pathname}
      sign={readings[0]}
      readings={readings}
      hash={entityHash(entity)}
      footer={footer}
    >
      {entity ? (
        <span className="min-w-0 break-all">
          <span className="text-text">no {entity.kind.replace('-', ' ')} at this address.</span>{' '}
          checked <span>{entity.checkedEndpoint}</span> → 404 · requested{' '}
          <span className="break-all">{entity.identifier}</span>
        </span>
      ) : (
        <span>
          <span className="text-text">no page at this path.</span> the chain was not asked · nothing
          on it is lost
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
