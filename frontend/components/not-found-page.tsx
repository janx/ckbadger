'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import { usePathname } from '@/src/navigation';
import {
  useNotFoundEvidence,
  type EntityKind,
  type NotFoundEntity,
} from '@/lib/not-found-evidence';
import { useDocumentTitle } from '@/lib/use-document-title';
import { truncateHash } from '@/lib/utils';

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

/** What a page's id must be, by kind: the first line of the verse when it is not one (R11). */
const NOT_AN_ID: Record<EntityKind, string> = {
  block: 'that is not a block number or hash.',
  uncle: 'that is not a block and uncle index.',
  transaction: 'that is not a transaction hash.',
  cell: 'that is not a cell out point.',
  address: 'that is not a ckb address.',
  script: 'that is not a script code hash.',
  'script-family': 'that is not a script name.',
  token: 'that is not a token type hash.',
  object: 'that is not an object id.',
  collection: 'that is not a collection id.',
  identity: 'that is not an identity id.',
  class: 'that is not a class id.',
  cluster: 'that is not a cluster id.',
  fork: 'that is not a fork height and orphan.',
  'fiber-channel': 'that is not a channel id.',
};

/**
 * An id that names nothing that could exist (the API's 400, or the page's own parse): nothing
 * was looked up, so the second line says the index was not asked.
 */
export function notAnIdVerse(kind: EntityKind): readonly string[] {
  return [NOT_AN_ID[kind], 'the index was not asked; nothing on it is lost.', ROUTE_VERSE[2]];
}

/** A page's id that is not one: its kind, and why (the API's message or the page's rule). */
export interface NotAnId {
  kind: EntityKind;
  reason: string;
}

const RETURNS: Partial<Record<EntityKind, { href: string; label: string }>> = {
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

/**
 * The endpoint a 404 came from, as one fact line shows it: every long segment (a hash, an
 * address) shortened, and only those may break anywhere (U-18). The rest wraps between words.
 */
function CheckedEndpoint({ path }: { path: string }) {
  return (
    <span data-endpoint="">
      {path.split(/([/?&=])/).map((part, i) => {
        const short = truncateHash(part);
        return short === part ? (
          part
        ) : (
          <span key={i} className="break-all">
            {short}
          </span>
        );
      })}
    </span>
  );
}

type NotFoundPageProps = {
  /** Drawn in place of AppFrame (the frame route's error boundary): its own header and footer. */
  standalone?: boolean;
} & (
  | { /** The page's entity lookup answered 404. */ entity?: NotFoundEntity; notAnId?: undefined }
  | {
      /** The page's id is not one (a 400, or the page's parse). */ notAnId: NotAnId;
      entity?: undefined;
    }
);

export function NotFoundPage({ entity, notAnId, standalone = false }: NotFoundPageProps = {}) {
  const pathname = usePathname();
  const lookup = useNotFoundEvidence(entity);
  useDocumentTitle(NOT_FOUND_TITLE);
  const kind = entity?.kind ?? notAnId?.kind;
  const back = kind ? RETURNS[kind] : undefined;

  // One line of facts under the verse: for a 404, what was asked and what /search said; for an
  // id that is not one, why.
  let facts: ReactNode = null;
  if (notAnId) {
    facts = <span className="min-w-0 [overflow-wrap:anywhere]">{notAnId.reason}</span>;
  } else if (entity) {
    facts = (
      // words wrap between words; only the shortened id breaks anywhere
      <span className="min-w-0 [overflow-wrap:anywhere]">
        checked <CheckedEndpoint path={entity.checkedEndpoint} /> → 404
        {lookup.status === 'loading' && (
          <>
            {' · '}
            <span role="status">checking /search for an exact match…</span>
          </>
        )}
        {lookup.status === 'checked' && lookup.evidence.finding !== null && (
          <>
            {' · '}
            {lookup.evidence.finding}
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
          </>
        )}
        {lookup.status === 'error' && (
          <>
            {' · '}
            <span role="status">the /search check could not complete: {lookup.error}</span>
          </>
        )}
      </span>
    );
  }

  return (
    <ErrorPageShell
      status={notAnId ? 'Not an id · 400' : 'Not found · 404'}
      path={pathname}
      verse={notAnId ? notAnIdVerse(notAnId.kind) : entity ? entityVerse(entity) : ROUTE_VERSE}
      standalone={standalone}
    >
      {facts}
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
