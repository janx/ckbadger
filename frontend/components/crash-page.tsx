'use client';

import { useContext, useState } from 'react';
import { UNSAFE_LocationContext } from 'react-router-dom';
import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import type { SignLines } from '@/components/error-sign';
import { usePathname } from '@/src/navigation';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { isKnownNetwork } from '@/lib/active-network';
import { useDocumentTitle } from '@/lib/use-document-title';

const ERROR_TITLE = ['Error'] as const;

/** A page that fell: the world keeps only what succeeded; a failed transition leaves no state. */
const ERROR_READINGS: readonly SignLines[] = [
  ['the chain keeps', 'what succeeded.', 'this did not.'],
  ['a transition', 'that failed', 'leaves no state.'],
  ['work was spent,', 'and nothing', 'came of it.'],
  ['our fault.', 'the chain', 'was not asked.'],
];

interface CrashPageProps {
  message: string;
  /** The outer React boundary can render without Router or QueryClient context. */
  contextSafe?: boolean;
}

/** A failed read/render is not evidence that a chain entity does not exist. */
export function CrashPage({ message, contextSafe = false }: CrashPageProps) {
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
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle');
  useDocumentTitle(ERROR_TITLE);
  const recovery = pathname.startsWith('/fiber/')
    ? { href: '/fiber/channels', label: 'Fiber channels' }
    : pathname.startsWith('/tx/')
      ? { href: '/transactions', label: 'transactions' }
      : pathname.startsWith('/blocks/')
        ? { href: '/blocks', label: 'blocks' }
        : { href: '/', label: 'back to the explorer' };

  async function copyDetails() {
    const report = [
      `Error: ${message}`,
      `Page: ${fullRoute}`,
      `Network: ${network}`,
      `Build: CKBadger ${build}`,
      'Chain tip: not checked in this error view',
    ].join('\n');
    try {
      await navigator.clipboard.writeText(report);
      setCopyState('copied');
    } catch {
      setCopyState('failed');
    }
  }

  return (
    <ErrorPageShell
      status="Error · not a 404"
      path={fullRoute}
      sign={ERROR_READINGS[0]}
      readings={ERROR_READINGS}
      brownout
      footer
      contextSafe={contextSafe}
    >
      <span role="alert" className="text-negative min-w-0 whitespace-pre-wrap break-all">
        ✗ {message}
      </span>
      <span className="min-w-0 break-all">
        page <span>{fullRoute}</span> · network {network} · CKBadger {build} ·{' '}
        <span>the chain was not checked in this view — this says nothing about what it holds</span>
      </span>
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        <button
          type="button"
          className="text-jade focus-visible:outline-jade whitespace-nowrap underline-offset-4 hover:underline focus-visible:outline"
          onClick={() => window.location.reload()}
        >
          ↻ try again
        </button>
        <button
          type="button"
          className="text-text hover:text-jade focus-visible:outline-jade whitespace-nowrap focus-visible:outline"
          onClick={copyDetails}
        >
          → copy details for a bug report
        </button>
        <Link
          href={recovery.href}
          className="text-text hover:text-jade focus-visible:outline-jade whitespace-nowrap focus-visible:outline"
        >
          ← {recovery.label}
        </Link>
      </div>
      {copyState !== 'idle' && (
        <span role="status">
          {copyState === 'copied' ? 'error details copied.' : 'could not copy details.'}
        </span>
      )}
    </ErrorPageShell>
  );
}
