'use client';

import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { localZone } from '@/lib/clock';
import { releaseOf, resolveBuildVersion } from '@/lib/runtime-config';
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
 * screen reader whose browse mode takes the `?` key. Below md, where nothing opens the help, the
 * footer ends on the release and the clock's zone (ReleaseAndZone).
 */
export function SiteFooter() {
  return (
    <footer className="border-base-border bg-base-void/95 border-t">
      <div className="text-small container mx-auto flex flex-wrap items-center gap-x-6 gap-y-2 py-[18px] font-mono leading-relaxed">
        <nav aria-label="Footer" className="flex flex-wrap items-center gap-x-5 gap-y-1">
          <Link look="nav" href="/hardforks" className={LINK}>
            Hardforks
          </Link>
          <Link look="nav" href="/network" className={LINK}>
            Network
          </Link>
          <Link look="nav" href="/fiber/channels" className={LINK}>
            Fiber channels
          </Link>
          <ExternalLink look="nav" href="https://dashboard.fiber.channel/" className={LINK}>
            Fiber dashboard
          </ExternalLink>
          <ExternalLink look="nav" href="https://cknerv.web5.info" className={LINK}>
            cknerv
          </ExternalLink>
          <ExternalLink look="nav" href="https://web5.info" className={LINK}>
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
        <ReleaseAndZone />
      </div>
    </footer>
  );
}

/** A token of the row: its "·", then its words, never broken inside. */
function Token({ name, title, children }: { name: string; title?: string; children: string }) {
  return (
    <span data-token={name} className="whitespace-nowrap">
      <span aria-hidden="true">{'· '}</span>
      <span data-text="" title={title}>
        {children}
      </span>
    </span>
  );
}

/**
 * Below md, the footer's last row (ruling on B4, 2026-11-14): the release and the clock's zone, what
 * the status line's wide form and the `?` help say from md, so a phone does not lose them. The
 * release is the status line's (resolveBuildVersion, releaseOf; the whole build string in its
 * title), the zone the clock's (localZone). Quiet text in the footer's lowercase, on a line of its
 * own: at 360 the links take two lines (274.0 and 281.8 of 328 px at 7.8 px a character) and this
 * one, "ckbadger 0.8.2 · asia/shanghai", 234.0. It breaks only between its two tokens (a zone of
 * more than 25 characters at 360): the row is drawn 2ch to the left in a box that clips its left
 * edge only, 0.75ch out as the separated list's (clip-path, never overflow-hidden, which would also
 * cut a token on the right), so the "·" leading the first token of each line is never seen and the
 * others read "a · b".
 */
function ReleaseAndZone() {
  const build = resolveBuildVersion();
  // no zone where Intl gives none: the release stands alone
  const zone = localZone();
  return (
    <p
      data-testid="footer-about"
      className="text-text-dim m-0 min-w-0 basis-full lowercase [clip-path:inset(-1em_-100vw_-1em_-0.75ch)] md:hidden"
    >
      <span className="-ml-[2ch] flex flex-wrap gap-x-[1ch]">
        <Token name="release" title={`ckbadger ${build}`}>{`ckbadger ${releaseOf(build)}`}</Token>
        {zone !== null && <Token name="zone">{zone}</Token>}
      </span>
    </p>
  );
}
