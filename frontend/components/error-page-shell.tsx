'use client';

// The not-found and error pages (rubbing × Song print, phase 4): a session that asked for a page.
//
//   ■ NOT FOUND · 404  /the/path
//   # no page at this path.
//     the chain was not asked; nothing on it is lost.
//     honey badger don't care.
//   the facts, then the ways on
//
// The title is the verse, in the voice (components/ui/comment, `.comment-lg`). Beside it (below
// it on a narrower screen) the newest block's seal is pressed on the page — the block's square
// stone (the seals by kind), its transactions counted in rods — faint, clear of every line of text
// (readability R6): the chain went on while this page was not found. CSS only; no canvas, no
// WebGL, nothing moves but the seal, which is the next block's when the tip moves.

import { lazy, Suspense, useState, type ReactNode } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { clsx } from 'clsx';
import { Header } from '@/components/layout/header';
import { PromptEnd } from '@/components/layout/prompt-end';
import { SiteFooter } from '@/components/layout/site-footer';
import { Wordmark } from '@/components/layout/wordmark';
import { Dotted } from '@/components/ui/phone-row';
import { ShortId } from '@/components/ui/short-id';
import { BlockLink } from '@/components/ui/block-link';
import { Comment } from '@/components/ui/comment';
import { useNetworkStats } from '@/hooks/useNetworkStats';
import { api } from '@/lib/api';

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
   * itself: the shell draws its own header, footer and page end and fills the screen. Otherwise
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

/**
 * Where the newest block's seal stands, px (readability ruling R6: the pressed seal moves clear of
 * the text column). The side gutters are the column's padding (`px-4 sm:px-8 lg:px-24`): 16, from
 * 640 px 32, from 1024 96. From 1280 (Tailwind's `xl`) the text column and the stone stand side by
 * side: the column at most 768 px (48rem, the widest measure in it, the facts'), a 64 px gap, the
 * 256 px stone: 768 + 64 + 256 = 1,088, all that 1280 leaves between its gutters (with less, a
 * window's scrollbar, the column gives way, never the stone). Below 1280 the stone stands under
 * the column, 64 px below it. Pressed −3°, it draws 256 × (cos 3° + sin 3°) = 269.05 px across,
 * 6.52 past its box on every side, inside the 64 px either way.
 */
export const ERROR_PAGE_LAYOUT = {
  gutters: [
    { from: 0, px: 16 },
    { from: 640, px: 32 },
    { from: 1024, px: 96 },
  ],
  besideFrom: 1280,
  textColumnPx: 768,
  sealGapPx: 64,
  sealPx: 256,
  sealTilt: -3,
} as const;

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
      <main className="flex min-h-[34rem] min-w-0 flex-1 flex-col overflow-hidden">
        {/* the text column, then the seal's place (ERROR_PAGE_LAYOUT): beside it from xl, a
            768 px column, 64 px, the 256 px stone; under it, 64 px down, below xl */}
        <div
          data-error-layout
          className="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-y-16 px-4 pb-10 pt-12 sm:px-8 lg:px-24 lg:pt-28 xl:grid-cols-[minmax(0,768px)_256px] xl:gap-x-16"
        >
          <div data-error-text className="flex min-w-0 flex-col">
            <p className="m-0 flex min-w-0 flex-wrap items-baseline gap-x-3.5 gap-y-1">
              <span aria-hidden="true" className="section-mark self-center" />
              <span
                className="eyebrow text-text-dim text-meta"
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
          </div>
          {!contextSafe && <TipSeal />}
        </div>
      </main>
      {/* U-20: the footer is the last quiet row, then the empty prompt ends the page, as the
          frame's pages end */}
      {standalone && <SiteFooter />}
      {wholeScreen && <PromptEnd />}
    </div>
  );
}

/**
 * The seal library arrives with the first 404 or error page, not with every first load: the shell
 * is in the entry chunk (the error boundaries are), and the seal is the one thing on it a reader
 * can wait a moment for — the verse, the facts and the caption are there at once.
 */
const Seal = lazy(() => import('@/components/ui/seal').then((m) => ({ default: m.Seal })));

/**
 * The stone's side as drawn and shown, px (ERROR_PAGE_LAYOUT: the room beside the text column at
 * 1280; it was 469 px, the area of the old tall brush seal, pressed behind the verse). Its box is
 * that square before the drawing arrives, so nothing moves when it does. Pressed a little askew,
 * as a hand presses a seal (the not-found board's −3°).
 */
const { sealPx: SEAL_PX, sealTilt: SEAL_TILT } = ERROR_PAGE_LAYOUT;

/**
 * The newest block's seal, pressed in its own place beside or below the text column, and a line
 * under it naming it. The tip is the vitals' (the status line's `tip #n`); the block's hash and
 * its transaction count are that block's own, read from `/blocks/{n}`. While neither has arrived,
 * or when either fails, there is no seal: the verse stands alone. The seal stays pressed until
 * the next tip's block is read.
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
    <figure data-seal-figure="" className="m-0 min-w-0">
      <div
        className="error-seal"
        aria-hidden="true"
        data-testid="error-seal"
        style={{ width: SEAL_PX, height: SEAL_PX }}
      >
        <Suspense fallback={null}>
          {/* in the cinnabar mark (Seal's ink), pressed: the block is on the chain */}
          <Seal
            id={{ kind: 'block', hash: block.hash, txs: block.transactionsCount }}
            form="page"
            height={SEAL_PX}
            tilt={SEAL_TILT}
          />
        </Suspense>
      </div>
      {/* a note under the seal, quiet; the block's link and its hash in it read (R3). The two
          figures are one separated list (Dotted, the one owner of a separator between parts) set
          inline after the words: where it does not fit the rest of the line it moves to the next
          line whole, so no line opens with its " · " (at 320, 360 and 390 px and in the stone's
          256 px column: "the seal of the newest block," / "#… · 0x…") */}
      <figcaption data-seal-caption="" className="text-text-dim text-meta mt-8 min-w-0">
        the seal of the newest block,{' '}
        <Dotted
          inline
          parts={[
            // a link inside the caption's line, in the reading ink the caption gives its values
            <span key="block" className="text-text">
              <BlockLink look="inline" number={block.number} />
            </span>,
            <ShortId key="hash" value={block.hash} kind="hash" size="row" className="text-text" />,
          ]}
        />
      </figcaption>
    </figure>
  );
}
