'use client';

import Link from '@/components/ui/link';

/**
 * The wordmark's look: the square, a 10 px gap, the name in the moon ink at 600. The link is at
 * least 32 px high (U-20; its 15 px line was the whole hit area), the mark centred in it, so the
 * mark stays where the row centres it.
 */
const WORDMARK_CLASS = 'flex min-h-8 shrink-0 items-center gap-2.5 font-semibold text-text-bright';

/**
 * The square and the name; the link around them is the caller's. Where the wordmark heads the
 * home page the name is the page's h1 (lane D2, U-11: the outline opens on it), drawn as the span
 * it is everywhere else: a flex item of the link, `.wordmark`'s size and weight, no new words.
 */
function Mark({ heading = false }: { heading?: boolean }) {
  const Name = heading ? 'h1' : 'span';
  return (
    <>
      <span aria-hidden="true" className="prompt-square" />
      <Name className="wordmark">ckbadger</Name>
    </>
  );
}

/**
 * The logo, for now (rubbing × Song print, the wordmark "2"): the cinnabar prompt square that
 * begins every command line on the site, then the name in lowercase — the site is a session and
 * its name is the first command. When an animal mark exists it replaces the square without moving
 * anything. `plain` draws a bare `<a href="/">` for the error page that renders without the router.
 * `heading`: the wordmark heads the home page, so its name is that page's h1.
 */
export function Wordmark({
  plain = false,
  heading = false,
}: {
  plain?: boolean;
  heading?: boolean;
}) {
  if (plain)
    return (
      <a href="/" aria-label="CKBadger home" className={WORDMARK_CLASS}>
        <Mark />
      </a>
    );
  return (
    <Link look="nav" href="/" aria-label="CKBadger home" className={WORDMARK_CLASS}>
      <Mark heading={heading} />
    </Link>
  );
}
