'use client';

import Link from '@/components/ui/link';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { openKeyboardShortcuts } from '@/lib/search-focus';

const LINK = 'text-text hover:text-jade py-1 transition-colors';

/**
 * The release a build string names: its part before any semver build metadata ("+…") or commit
 * ("@…") — "0.8.2" for "0.8.2@f8947b8d3915". The whole build string stays in the title.
 */
function releaseOf(buildVersion: string): string {
  return buildVersion.split(/[+@]/)[0];
}

/**
 * The site footer (Main.dc.html, TX-04): "CKBadger <release>" as plain text, then Hardforks ·
 * Fiber dashboard · cknerv · Web5 · Keyboard shortcuts. No always-on pulse: whether updates are
 * live is said only by the vitals strip, on a page that holds the realtime socket (TX-01).
 */
export function SiteFooter() {
  const buildVersion = resolveBuildVersion();

  return (
    <footer className="border-base-border bg-base-void/95 border-t">
      <div className="text-small container mx-auto flex flex-wrap items-center justify-between gap-x-6 gap-y-2 px-4 py-[18px] font-mono leading-relaxed">
        <span className="text-text-dim" title={`CKBadger ${buildVersion}`}>
          CKBadger {releaseOf(buildVersion)}
        </span>
        <nav aria-label="Footer" className="flex flex-wrap items-center gap-x-5 gap-y-1">
          <Link href="/hardforks" className={LINK}>
            Hardforks
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
