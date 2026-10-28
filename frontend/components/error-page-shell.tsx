'use client';

import { useRef, type ReactNode, type RefObject } from 'react';
import { Header } from '@/components/layout/header';
import { SiteFooter } from '@/components/layout/site-footer';

interface ErrorPageShellProps {
  status: string;
  path: string;
  title: ReactNode;
  children: ReactNode;
  artwork?: (avoidRef: RefObject<HTMLElement | null>) => ReactNode;
  caption?: ReactNode;
  footer?: boolean;
  contextSafe?: boolean;
}

/** Route errors replace AppFrame, while ordinary 404 routes render inside it. */
export function ErrorPageShell({
  status,
  path,
  title,
  children,
  artwork,
  caption,
  footer = false,
  contextSafe = false,
}: ErrorPageShellProps) {
  const cardRef = useRef<HTMLElement>(null);
  return (
    <div className="bg-base-bg relative flex min-h-screen min-w-0 flex-col overflow-x-hidden">
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
      ) : (
        <Header />
      )}
      <main className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden px-4 pb-10 pt-12 sm:px-8 lg:px-20 lg:pt-20">
        {artwork && (
          <div className="pointer-events-none absolute inset-0 -z-10 opacity-70" aria-hidden="true">
            {artwork(cardRef)}
          </div>
        )}
        <div className="relative z-10 flex w-full min-w-0 flex-1 flex-col justify-between gap-16">
          <section
            ref={cardRef}
            className="bg-base-bg/85 w-full min-w-0 max-w-[47rem] py-3 backdrop-blur-[2px] sm:pr-4"
          >
            <div className="mb-5 flex min-w-0 flex-wrap items-baseline gap-x-4 gap-y-1">
              <p
                className="text-text-bright text-[18px] uppercase tracking-[0.1em] sm:text-[22px]"
                style={{ fontFamily: 'var(--font-error-display)' }}
              >
                {status}
              </p>
              <p className="text-text-dim min-w-0 break-all font-mono text-[11px]">{path}</p>
            </div>
            <h1
              className="text-text-bright max-w-full break-words uppercase leading-[0.94] tracking-[-0.045em]"
              style={{ fontFamily: 'var(--font-error-display)' }}
            >
              {title}
            </h1>
            <div className="mt-8 min-w-0">{children}</div>
          </section>
          {caption && (
            <section
              aria-label="Common knowledge cells artwork source"
              className="bg-base-bg/85 text-text-dim min-w-0 max-w-[47rem] py-3 backdrop-blur-[2px]"
            >
              <h2
                className="text-text-bright mb-2 text-[13px] uppercase tracking-[0.12em]"
                style={{ fontFamily: 'var(--font-error-display)' }}
              >
                Common knowledge cells
              </h2>
              <p className="break-words text-[12px] leading-relaxed">{caption}</p>
            </section>
          )}
        </div>
      </main>
      {footer && <SiteFooter />}
    </div>
  );
}
