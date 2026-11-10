'use client';

// The not-found and error pages (rubbing × Song print, phase 4): a session that asked for a page.
//
//   ■ NOT FOUND · 404  /the/path
//   # no page at this path.
//     the chain was not asked; nothing on it is lost.
//     honey badger don't care.
//   the facts, then the ways on
//
// The title is the verse, in the voice (components/ui/comment, `.comment-lg`). Behind it the
// newest block's hash is pressed on the page as its talisman — the hash seal of design 6, the heavy
// brush with no stone — the full height of the first screen, centred and faint: the chain went on
// while this page was not found. CSS only; no canvas, no WebGL, nothing moves but the seal, which
// is the next block's when the tip moves.

import { lazy, Suspense, useState, type ReactNode } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { clsx } from 'clsx';
import { Header } from '@/components/layout/header';
import { PromptEnd } from '@/components/layout/prompt-end';
import { SiteFooter } from '@/components/layout/site-footer';
import { Wordmark } from '@/components/layout/wordmark';
import { BlockLink } from '@/components/ui/block-link';
import { Comment } from '@/components/ui/comment';
import { useNetworkStats } from '@/hooks/useNetworkStats';
import { api } from '@/lib/api';
import { color } from '@/lib/palette';
import { truncateHash } from '@/lib/utils';

interface ErrorPageShellProps {
  /** The eyebrow: what happened (`Not found · 404`, `Error · not a 404`). */
  status: string;
  path: string;
  /** The page's title, in the voice: a sentence to a line. */
  verse: readonly string[];
  /** The facts and the ways on, under the verse. */
  children: ReactNode;
  /**
   * Drawn in place of AppFrame — the frame route's error boundary caught a failure of the frame
   * itself: the shell draws its own header, page end and footer and fills the screen. Otherwise
   * (the not-found route, a page's not-found, a page's route error) it renders inside the frame,
   * under the frame's header (visual finish V6), and fills what the header leaves.
   */
  standalone?: boolean;
  /**
   * The outer boundary renders without Router or QueryClient: a plain header in place of the site
   * header, and no seal — with no query client there is no tip to read, and no hash is made up.
   * Never inside the frame: a whole screen.
   */
  contextSafe?: boolean;
}

const EYEBROW_EGG = 'You are (not) here';

/** In the frame by default; `standalone` / `contextSafe` when drawn in place of it. */
export function ErrorPageShell({
  status,
  path,
  verse,
  children,
  standalone = false,
  contextSafe = false,
}: ErrorPageShellProps) {
  const wholeScreen = standalone || contextSafe;
  const [eyebrowEgg, setEyebrowEgg] = useState(false);
  const eggOn = () => setEyebrowEgg(true);
  const eggOff = () => setEyebrowEgg(false);
  return (
    <div
      className={clsx(
        'bg-base-bg relative flex min-w-0 flex-col overflow-x-hidden',
        wholeScreen ? 'min-h-screen' : 'flex-1'
      )}
    >
      {contextSafe ? (
        <header className="border-base-border-subtle bg-base-bg/95 relative z-20 border-b px-4 py-3 sm:px-8 lg:px-20">
          <Wordmark plain />
        </header>
      ) : standalone ? (
        <Header />
      ) : null}
      {/* `isolate`: the seal (z −1) is pressed on the page's ground, under every line of text */}
      <main className="relative isolate flex min-h-[34rem] min-w-0 flex-1 flex-col overflow-hidden">
        {/* not positioned, so the seal's box is main's: the first screen, whatever the column holds */}
        <div className="flex min-w-0 flex-col px-4 pb-10 pt-12 sm:px-8 lg:px-24 lg:pt-28">
          <p className="m-0 flex min-w-0 flex-wrap items-baseline gap-x-3.5 gap-y-1">
            <span aria-hidden="true" className="section-mark self-center" />
            <span
              className="eyebrow text-text-dim"
              data-eyebrow
              onPointerEnter={eggOn}
              onPointerLeave={eggOff}
              onPointerDown={eggOn}
              onPointerUp={eggOff}
            >
              {eyebrowEgg ? EYEBROW_EGG : status}
            </span>
            <span className="text-text-dim text-meta min-w-0 break-all font-mono">{path}</span>
          </p>
          <Comment as="h1" size="lg" lines={verse} className="mt-7 max-w-[40rem]" />
          <div className="error-facts text-text-dim text-meta mt-10 flex min-w-0 max-w-[48rem] flex-col gap-0.5 font-mono leading-[1.8]">
            {children}
          </div>
          {!contextSafe && <TipSeal />}
        </div>
      </main>
      {wholeScreen && <PromptEnd />}
      {standalone && <SiteFooter />}
    </div>
  );
}

/**
 * The talisman generator arrives with the first 404 or error page, not with every first load: the
 * shell is in the entry chunk (the error boundaries are), and the seal is the one thing on it a
 * reader can wait a moment for — the verse, the facts and the caption are there at once.
 */
const HashSeal = lazy(() =>
  import('@/components/ui/hash-seal').then((m) => ({ default: m.HashSeal }))
);

/** The seal's longer side as drawn; the CSS sets it to the first screen's height (`.error-seal`). */
const SEAL_PX = 720;
/** Pressed a little askew, as a hand presses a seal (the not-found board's −3°). */
const SEAL_TILT = -3;

/**
 * The newest block's seal, pressed behind the verse, and a line naming it. The tip is the vitals'
 * (the status line's `tip #n`); the hash is that block's own, read from `/blocks/{n}`. While
 * neither has arrived, or when either fails, there is no seal: the verse stands alone. The seal
 * stays pressed until the next tip's block is read.
 */
function TipSeal() {
  const { data: stats } = useNetworkStats();
  const tip = stats?.latestBlock;
  const { data: block } = useQuery({
    queryKey: ['block', tip],
    queryFn: () => {
      if (tip === undefined) throw new Error('tip seal: the vitals carry no tip yet');
      return api.getBlock(tip);
    },
    enabled: tip !== undefined,
    placeholderData: keepPreviousData,
  });
  if (!block) return null;
  return (
    <>
      <div className="error-seal" aria-hidden="true" data-testid="error-seal">
        <Suspense fallback={null}>
          <HashSeal
            hash={block.hash}
            variant="brush"
            size={SEAL_PX}
            tilt={SEAL_TILT}
            ink={color('mark')}
          />
        </Suspense>
      </div>
      <p data-seal-caption="" className="text-text-ghost text-meta m-0 mt-8 min-w-0">
        behind: the seal of the newest block, <BlockLink number={block.number} /> ·{' '}
        <span className="break-all">{truncateHash(block.hash)}</span>
      </p>
    </>
  );
}
