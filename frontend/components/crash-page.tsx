'use client';

import { useContext } from 'react';
import { UNSAFE_LocationContext } from 'react-router-dom';
import Link from '@/components/ui/link';
import { CopyButton } from '@/components/ui/copy-button';
import { ErrorPageShell } from '@/components/error-page-shell';
import { usePathname } from '@/src/navigation';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { isKnownNetwork } from '@/lib/active-network';
import { useDocumentTitle } from '@/lib/use-document-title';

const ERROR_TITLE = ['Error'] as const;

/**
 * A page that fell, in the voice: the world keeps only what succeeded; a failed transition leaves
 * no state. Its own words, not the 404's (the meme is the 404's alone).
 */
const CRASH_VERSE = ['the chain keeps what succeeded.', 'this did not.'] as const;

interface CrashPageProps {
  message: string;
  /** The outer React boundary can render without Router or QueryClient context. */
  contextSafe?: boolean;
  /** Drawn in place of AppFrame (the frame route's error boundary): its own header and footer. */
  standalone?: boolean;
}

/** A failed read/render is not evidence that a chain entity does not exist. */
export function CrashPage({ message, contextSafe = false, standalone = false }: CrashPageProps) {
  const pathname = usePathname();
  const locationContext = useContext(UNSAFE_LocationContext);
  const location = locationContext?.location;
  const fullRoute = location
    ? `${location.pathname}${location.search}${location.hash}`
    : typeof window === 'undefined'
      ? pathname
      : `${window.location.pathname}${window.location.search}${window.location.hash}`;
  const networkSegment = fullRoute.replace(/^\/+/, '').split('/')[0];
  const network = isKnownNetwork(networkSegment) ? networkSegment : 'not resolved';
  const build = resolveBuildVersion();
  useDocumentTitle(ERROR_TITLE);
  const recovery = pathname.startsWith('/fiber/')
    ? { href: '/fiber/channels', label: 'Fiber channels' }
    : pathname.startsWith('/tx/')
      ? { href: '/transactions', label: 'transactions' }
      : pathname.startsWith('/blocks/')
        ? { href: '/blocks', label: 'blocks' }
        : { href: '/', label: 'back to the explorer' };

  // What a bug report needs, exact: the error, the page, the network and the build, and that this
  // view read no chain tip.
  const report = [
    `Error: ${message}`,
    `Page: ${fullRoute}`,
    `Network: ${network}`,
    `Build: CKBadger ${build}`,
    'Chain tip: not checked in this error view',
  ].join('\n');

  return (
    <ErrorPageShell
      status="Error · not a 404"
      path={fullRoute}
      verse={CRASH_VERSE}
      // the outer React boundary is above the router, so never inside the frame
      standalone={standalone || contextSafe}
      contextSafe={contextSafe}
    >
      <span
        role="alert"
        className="text-negative min-w-0 whitespace-pre-wrap [overflow-wrap:anywhere]"
      >
        ✗ {message}
      </span>
      {/* words wrap between words; only the route, one long token, may break anywhere. The
          facts end with the one copy of the controls board, which copies them for a bug report
          and answers in words beside it; an action with a purpose, not an id's copy, so its words
          stand after its icon (lane c2-finish, item 4). */}
      <span className="min-w-0 [overflow-wrap:anywhere]">
        page <span className="break-all">{fullRoute}</span> · network {network} · CKBadger {build} ·{' '}
        <span>the chain was not checked in this view — this says nothing about what it holds</span>{' '}
        <CopyButton
          value={report}
          label="copy details for a bug report"
          words
          className="align-middle"
        />
      </span>
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        <button
          type="button"
          className="text-jade focus-visible:outline-jade underline-offset-4 hover:underline focus-visible:outline"
          onClick={() => window.location.reload()}
        >
          <span className="whitespace-nowrap">↻ </span>try again
        </button>
        <Link
          href={recovery.href}
          className="text-text hover:text-jade focus-visible:outline-jade focus-visible:outline"
        >
          <span className="whitespace-nowrap">← </span>
          {recovery.label}
        </Link>
      </div>
    </ErrorPageShell>
  );
}
