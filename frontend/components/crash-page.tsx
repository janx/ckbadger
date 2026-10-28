'use client';

import { useContext, useState } from 'react';
import { UNSAFE_LocationContext } from 'react-router-dom';
import Link from '@/components/ui/link';
import { ErrorPageShell } from '@/components/error-page-shell';
import { ErrorVessels, VesselsCaption } from '@/components/error-vessels';
import { usePathname } from '@/src/navigation';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { isKnownNetwork } from '@/lib/active-network';

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
  const recovery = pathname.startsWith('/fiber/')
    ? { href: '/fiber/channels', label: 'Back to Fiber channels' }
    : pathname.startsWith('/tx/')
      ? { href: '/transactions', label: 'Back to transactions' }
      : pathname.startsWith('/blocks/')
        ? { href: '/blocks', label: 'Back to blocks' }
        : { href: '/', label: 'Back to the explorer' };

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
      title={
        <>
          <span className="block text-[clamp(2.4rem,7vw,5.75rem)]">The page fell.</span>{' '}
          <span className="block text-[clamp(1.9rem,6.4vw,3.75rem)]">The cause is unclear.</span>
        </>
      }
      footer
      contextSafe={contextSafe}
      artwork={contextSafe ? undefined : (avoidRef) => <ErrorVessels avoidRef={avoidRef} />}
      caption={contextSafe ? undefined : <VesselsCaption />}
    >
      <p
        className="text-text-bright max-w-xl text-[clamp(1.25rem,3vw,1.7rem)] leading-snug"
        style={{ fontFamily: 'var(--font-error-display)' }}
      >
        We could not complete this page.
      </p>
      <p className="text-text-dim mt-3 max-w-xl text-[13px] leading-relaxed">
        This error does not establish whether the requested entity or chain data is available.
      </p>
      <dl className="border-base-border-subtle mt-8 min-w-0 border-t font-mono text-[12px]">
        <div className="border-base-border-subtle grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[7rem_minmax(0,1fr)]">
          <dt className="text-text-dim">error</dt>
          <dd role="alert" className="text-negative min-w-0 whitespace-pre-wrap break-all">
            {message}
          </dd>
        </div>
        <div className="border-base-border-subtle grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[7rem_minmax(0,1fr)]">
          <dt className="text-text-dim">page</dt>
          <dd className="text-text min-w-0 break-all">{fullRoute}</dd>
        </div>
        <div className="border-base-border-subtle grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[7rem_minmax(0,1fr)]">
          <dt className="text-text-dim">network</dt>
          <dd className="text-text min-w-0 break-all">{network}</dd>
        </div>
        <div className="border-base-border-subtle grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[7rem_minmax(0,1fr)]">
          <dt className="text-text-dim">build</dt>
          <dd className="text-text min-w-0 break-all">CKBadger {build}</dd>
        </div>
        <div className="border-base-border-subtle grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] gap-3 border-b py-3 sm:grid-cols-[7rem_minmax(0,1fr)]">
          <dt className="text-text-dim">chain tip</dt>
          <dd className="text-text min-w-0 break-words">Not checked in this error view</dd>
        </div>
      </dl>
      <div className="mt-8 flex flex-wrap gap-3 font-mono text-[12px]">
        <button
          type="button"
          className="border-jade text-jade hover:bg-jade/10 focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
          onClick={copyDetails}
        >
          Copy details for a bug report
        </button>
        <button
          type="button"
          className="border-base-border text-text hover:text-jade focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
          onClick={() => window.location.reload()}
        >
          Try again →
        </button>
        <Link
          href={recovery.href}
          className="border-base-border text-text hover:text-jade focus-visible:outline-jade rounded border px-4 py-2 focus-visible:outline"
        >
          {recovery.label} →
        </Link>
      </div>
      {copyState !== 'idle' && (
        <p role="status" className="text-text-dim mt-3 text-[12px]">
          {copyState === 'copied' ? 'Error details copied.' : 'Could not copy details.'}
        </p>
      )}
    </ErrorPageShell>
  );
}
