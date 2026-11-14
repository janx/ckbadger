import type { ReactNode } from 'react';

/**
 * A link that leaves the site (UI review 2026-11-14, U-20): its words, then ` ↗` held to the last
 * word, opening in a new tab without a referrer. The one form for an `<a>` to another site; a link
 * within the site is `Link` (components/ui/link) and carries no mark. Only an absolute http(s)
 * address leaves the site: anything else is refused, not drawn as external.
 */
export function ExternalLink({
  href,
  className,
  children,
}: {
  href: string;
  className?: string;
  children: ReactNode;
}) {
  if (!/^https?:\/\//.test(href))
    throw new Error(`ExternalLink: ${JSON.stringify(href)} is not an http(s) address`);
  return (
    <a href={href} target="_blank" rel="noreferrer" className={className}>
      {children}
      <span className="whitespace-nowrap"> ↗</span>
    </a>
  );
}
