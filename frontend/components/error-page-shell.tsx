'use client';

// The not-found and error pages, direction Z: the sign over the night of ink.
//
//   all that is / was worked for. / this was not.        — a route that names no page
//   a hash / is a fact. / a name, a guess.                — an entity known as something else
//   known to some, / perhaps. / to all, not yet.          — an entity nothing answers
//   the chain keeps / what succeeded. / this did not.     — a page that fell
//
// Two things on the page and nothing else. The sign is the title: one thought from the worldview
// cut into three tubes, two lit and the third never lit — the dark line is where this page falls
// outside the law the lit lines state. The night is WebGL (components/error-night.tsx): every
// block the new hash is poured in as what it is, 256 bits in a grid of ink that holds for a
// second and then dissolves in a current the hash itself seeded; the 1-bits leave as motes. The
// honest tier is two or three small lines bottom-left. No card, no serif, nothing depicted.

import { useCallback, useState, type ReactNode } from 'react';
import { clsx } from 'clsx';
import { Header } from '@/components/layout/header';
import { SiteFooter } from '@/components/layout/site-footer';
import { CssNight, ErrorNight } from '@/components/error-night';
import { ErrorSign, type SignLines } from '@/components/error-sign';
import type { Block } from '@/hooks/useRealtimeStore';

interface ErrorPageShellProps {
  status: string;
  path: string;
  sign: SignLines;
  readings?: readonly SignLines[];
  brownout?: boolean;
  /** The hash this page is about: the night's first grid. Null: the first block's. */
  hash?: string | null;
  /** The facts: two or three lines bottom-left, the actions among them. */
  children: ReactNode;
  /**
   * Drawn in place of AppFrame — the frame route's error boundary caught a failure of the frame
   * itself: the shell draws its own header and footer and fills the screen. Otherwise (the
   * not-found route, a page's not-found, a page's route error) it renders inside the frame, under
   * the frame's header (visual finish V6), and fills what the header leaves.
   */
  standalone?: boolean;
  /**
   * The outer boundary renders without Router or QueryClient: no realtime night, no ticks, a
   * plain header in place of the site header. Never inside the frame: a whole screen.
   */
  contextSafe?: boolean;
}

const EYEBROW_EGG = 'You are (not) here';

/** In the frame by default; `standalone` / `contextSafe` when drawn in place of it. */
export function ErrorPageShell({
  status,
  path,
  sign,
  readings,
  brownout = false,
  hash = null,
  children,
  standalone = false,
  contextSafe = false,
}: ErrorPageShellProps) {
  const wholeScreen = standalone || contextSafe;
  const [block, setBlock] = useState<Block | null>(null);
  const [inverted, setInverted] = useState(false);
  const [eyebrowEgg, setEyebrowEgg] = useState(false);
  const onBlock = useCallback((next: Block) => setBlock(next), []);
  const eggOn = () => setEyebrowEgg(true);
  const eggOff = () => setEyebrowEgg(false);
  return (
    <div
      className={clsx(
        'bg-base-bg relative flex min-w-0 flex-col overflow-x-hidden',
        wholeScreen ? 'min-h-screen' : 'flex-1',
        inverted && 'sign-inverted'
      )}
    >
      {contextSafe ? (
        <header className="border-base-border-subtle bg-base-bg/95 relative z-20 border-b px-4 py-3 sm:px-8 lg:px-20">
          <a href="/" aria-label="CKBadger Home" className="inline-flex items-center">
            <img
              src="/ckbadger-logo.webp"
              alt="CKBadger"
              width="143"
              height="97"
              className="h-auto w-28 rotate-[8deg]"
            />
          </a>
        </header>
      ) : standalone ? (
        <Header />
      ) : null}
      <main className="relative isolate flex min-h-[34rem] min-w-0 flex-1 flex-col overflow-hidden">
        {contextSafe ? (
          <div className="error-night" aria-hidden="true" data-night="static">
            <CssNight hash={null} />
          </div>
        ) : (
          <ErrorNight hash={hash} onBlock={onBlock} />
        )}
        <p className="absolute left-4 top-[52px] z-[2] m-0 flex min-w-0 max-w-[calc(100%-2rem)] flex-wrap items-baseline gap-x-3.5 gap-y-1 lg:left-24 lg:top-32 lg:max-w-[calc(100%-12rem)]">
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
        <div className="absolute left-4 top-24 z-[2] lg:left-24 lg:top-44">
          <ErrorSign
            lines={sign}
            readings={readings}
            brownout={brownout}
            blockKey={block?.hash ?? null}
            epochTurn={block?.epochIndex === 0}
            onInverted={setInverted}
          />
        </div>
        <div className="error-facts text-text-dim text-meta absolute bottom-6 left-4 right-4 z-[2] flex min-w-0 flex-col gap-0.5 font-mono leading-[1.8] lg:bottom-11 lg:left-24 lg:right-24">
          {children}
        </div>
      </main>
      {standalone && <SiteFooter />}
    </div>
  );
}
