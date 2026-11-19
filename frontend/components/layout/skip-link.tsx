'use client';

import Link from '@/components/ui/link';

/** The frame's hold of the page (src/routes/router.tsx): the skip link's target. */
export const PAGE_CONTENT_ID = 'page-content';

/**
 * The frame's first focusable element (UI review 2026-11-14, U-20): a keyboard or a screen reader
 * goes past the header and the vitals to the page. Drawn only while it has the focus, at the top
 * left over the header, as a command line: the prompt square, then the words in lowercase. The
 * focus moves without touching the URL's fragment, which the router would read as a navigation.
 * A link of the shell's navigation (`look="nav"`, components/ui/link.tsx): its own design.
 */
export function SkipLink() {
  return (
    <Link
      look="nav"
      href={`#${PAGE_CONTENT_ID}`}
      onClick={(event) => {
        event.preventDefault();
        const target = document.getElementById(PAGE_CONTENT_ID);
        if (!target) throw new Error(`skip link: the frame holds no #${PAGE_CONTENT_ID}`);
        target.focus();
      }}
      className="border-base-border bg-base-surface text-text-bright text-small sr-only inline-flex items-center gap-2.5 border px-3 py-2 font-mono focus:not-sr-only focus:fixed focus:left-4 focus:top-3 focus:z-50"
    >
      <span aria-hidden="true" className="prompt-square" />
      skip to content
    </Link>
  );
}
