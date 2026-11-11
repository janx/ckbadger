'use client';

import Link from '@/components/ui/link';
import { openKeyboardShortcuts } from '@/lib/search-focus';

const LINK = 'text-text hover:text-jade py-1 transition-colors';

/**
 * The site footer (Main.dc.html, TX-04): Hardforks · Network · Fiber channels · Fiber dashboard ·
 * cknerv · Web5 · Keyboard shortcuts. Network and Fiber channels (review 2026-11-03, item 4) are
 * the chain's surroundings, not a section, so they live here beside Hardforks rather than in the
 * header. The release it used to name is said by the status line now, beside the session's mode
 * (components/layout/status-line.tsx, rubbing × Song print phase 2).
 */
export function SiteFooter() {
  return (
    <footer className="border-base-border bg-base-void/95 border-t">
      <div className="text-small container mx-auto flex flex-wrap items-center gap-x-6 gap-y-2 py-[18px] font-mono leading-relaxed">
        <nav aria-label="Footer" className="flex flex-wrap items-center gap-x-5 gap-y-1">
          <Link href="/hardforks" className={LINK}>
            Hardforks
          </Link>
          <Link href="/network" className={LINK}>
            Network
          </Link>
          <Link href="/fiber/channels" className={LINK}>
            Fiber channels
          </Link>
          <a
            href="https://dashboard.fiber.channel/"
            target="_blank"
            rel="noreferrer"
            className={LINK}
          >
            Fiber dashboard
          </a>
          <a href="https://cknerv.web5.info" target="_blank" rel="noreferrer" className={LINK}>
            cknerv
          </a>
          <a href="https://web5.info" target="_blank" rel="noreferrer" className={LINK}>
            Web5
          </a>
          <button type="button" onClick={openKeyboardShortcuts} className={LINK}>
            Keyboard shortcuts
          </button>
        </nav>
      </div>
    </footer>
  );
}
