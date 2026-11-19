import type { ReactNode } from 'react';
import { linkLookClass, type LinkLook } from '@/components/ui/link';

/**
 * A link that leaves the site (UI review 2026-11-14, U-20): its words, then ` ↗` held to the last
 * word, opening in a new tab without a referrer. The one form for an `<a>` to another site; a link
 * within the site is `Link` (components/ui/link) and carries no mark. Only an absolute http(s)
 * address leaves the site: anything else is refused, not drawn as external. It takes a look as
 * `Link` does (components/ui/link.tsx, the owner of the looks): its ` ↗` is the way onward's arrow
 * to another site, so it writes no ` →`; a block is never one (it says where it goes).
 */
export function ExternalLink({
  href,
  look,
  className,
  children,
}: {
  href: string;
  /** Optional only while the link-look gate's TOLERATED list holds call sites that state none. */
  look?: Exclude<LinkLook, 'block'>;
  className?: string;
  children: ReactNode;
}) {
  if (!/^https?:\/\//.test(href))
    throw new Error(`ExternalLink: ${JSON.stringify(href)} is not an http(s) address`);
  return (
    <a href={href} target="_blank" rel="noreferrer" className={linkLookClass(look, className)}>
      {children}
      <span className="whitespace-nowrap"> ↗</span>
    </a>
  );
}
