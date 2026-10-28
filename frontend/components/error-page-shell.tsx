'use client';

import type { ReactNode } from 'react';
import { Header } from '@/components/layout/header';
import { SiteFooter } from '@/components/layout/site-footer';

interface ErrorPageShellProps {
  status: string;
  path: string;
  title: ReactNode;
  children: ReactNode;
  artwork?: ReactNode;
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
  return (
    <div className="bg-base-bg relative flex min-h-screen min-w-0 flex-col overflow-x-hidden">
      {!contextSafe && <Header />}
      <main className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden px-4 pb-10 pt-12 sm:px-8 lg:px-20 lg:pt-20">
        {artwork && (
          <div className="pointer-events-none absolute inset-0 -z-10 opacity-70" aria-hidden="true">
            {artwork}
          </div>
        )}
        <div className="relative z-10 flex w-full min-w-0 flex-1 flex-col justify-between gap-16">
          <section className="bg-base-bg/85 w-full min-w-0 max-w-[47rem] py-3 backdrop-blur-[2px] sm:pr-4">
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
            <div className="bg-base-bg/85 text-text-dim min-w-0 max-w-[47rem] py-3 backdrop-blur-[2px]">
              {caption}
            </div>
          )}
        </div>
      </main>
      {footer && <SiteFooter />}
    </div>
  );
}
