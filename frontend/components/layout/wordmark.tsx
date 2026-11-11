'use client';

import Link from '@/components/ui/link';

/** The wordmark's look: the square, a 10 px gap, the name in the moon ink at 600. */
const WORDMARK_CLASS = 'flex shrink-0 items-center gap-2.5 font-semibold text-text-bright';

/** The square and the name; the link around them is the caller's. */
function Mark() {
  return (
    <>
      <span aria-hidden="true" className="prompt-square" />
      <span className="wordmark">ckbadger</span>
    </>
  );
}

/**
 * The logo, for now (rubbing × Song print, the wordmark "2"): the cinnabar prompt square that
 * begins every command line on the site, then the name in lowercase — the site is a session and
 * its name is the first command. When an animal mark exists it replaces the square without moving
 * anything. `plain` draws a bare `<a href="/">` for the error page that renders without the router.
 */
export function Wordmark({ plain = false }: { plain?: boolean }) {
  if (plain)
    return (
      <a href="/" aria-label="CKBadger home" className={WORDMARK_CLASS}>
        <Mark />
      </a>
    );
  return (
    <Link href="/" aria-label="CKBadger home" className={WORDMARK_CLASS}>
      <Mark />
    </Link>
  );
}
