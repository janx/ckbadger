'use client';

import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { openKeyboardShortcuts } from '@/lib/search-focus';
import { cn } from '@/lib/utils';

/** A footer entry: the nav's lowercase (U-01), by CSS, the label as written. */
const LINK = 'text-text hover:text-jade py-1 lowercase transition-colors';

/**
 * The site footer (Main.dc.html, TX-04): Hardforks · Network · Fiber channels · Fiber dashboard ↗ ·
 * cknerv ↗ · Web5 ↗ · Keyboard shortcuts. Network and Fiber channels (review 2026-11-03, item 4) are
 * the chain's surroundings, not a section, so they live here beside Hardforks rather than in the
 * header. The release it used to name is said by the status line now, beside the session's mode
 * (components/layout/status-line.tsx, rubbing × Song print phase 2).
 *
 * The page's last quiet row (UI review 2026-11-14, U-20): the frame draws it after the page and
 * before the empty prompt, which ends the page. Its entries are lowercase as the nav's; the three
 * that leave the site carry ` ↗` (ExternalLink). "Keyboard shortcuts" is drawn from md only, as the
 * status line's key hints are (PA-29: a touch screen has no keys to list). From md it stays: the
 * status line's `? help` names the key but is no control (a hint in a status region, neither
 * focusable nor clickable), and this button is the one way into the help for a pointer, or for a
 * screen reader whose browse mode takes the `?` key.
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
          <ExternalLink href="https://dashboard.fiber.channel/" className={LINK}>
            Fiber dashboard
          </ExternalLink>
          <ExternalLink href="https://cknerv.web5.info" className={LINK}>
            cknerv
          </ExternalLink>
          <ExternalLink href="https://web5.info" className={LINK}>
            Web5
          </ExternalLink>
          <button
            type="button"
            onClick={openKeyboardShortcuts}
            className={cn(LINK, 'hidden md:inline')}
          >
            Keyboard shortcuts
          </button>
        </nav>
      </div>
    </footer>
  );
}
