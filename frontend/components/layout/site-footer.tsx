'use client';

import Link from '@/components/ui/link';
import { resolveBuildVersion } from '@/lib/runtime-config';

export function SiteFooter() {
  const buildVersion = resolveBuildVersion();

  return (
    <footer className="border-base-border bg-base-void/95 border-t">
      <div className="container mx-auto grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-6 gap-y-3 px-4 py-3 font-mono text-[11px] leading-relaxed lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
        <a
          href="https://github.com/janx/ckbadger"
          target="_blank"
          rel="noreferrer"
          title={`CKBadger ${buildVersion}`}
          className="text-text-dim hover:text-jade group flex min-w-0 max-w-full items-center gap-2 justify-self-start py-1 transition-colors"
        >
          <span className="live-dot shrink-0" aria-hidden="true" />
          <span className="text-text group-hover:text-jade shrink-0 transition-colors">
            CKBadger
          </span>
          <span className="truncate">{buildVersion}</span>
        </a>

        <nav
          aria-label="Footer"
          className="col-span-2 row-start-2 flex flex-wrap items-center justify-center gap-x-5 gap-y-2 lg:col-span-1 lg:col-start-2 lg:row-start-1"
        >
          <Link href="/hardforks" className="text-text hover:text-jade py-1 transition-colors">
            Hardforks
          </Link>
          <a
            href="https://dashboard.fiber.channel/"
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-jade py-1 transition-colors"
          >
            Fiber Dashboard
          </a>
          <a
            href="https://web5.info"
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-jade py-1 transition-colors"
          >
            Web5
          </a>
        </nav>

        <span className="text-text-dim col-start-2 row-start-1 flex items-center gap-2 justify-self-end lg:col-start-3">
          <kbd className="border-base-border text-text rounded border px-1.5 py-0.5 font-mono text-[10px]">
            ?
          </kbd>
          <span>keys</span>
        </span>
      </div>
    </footer>
  );
}
