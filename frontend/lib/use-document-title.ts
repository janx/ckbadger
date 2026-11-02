import { useEffect, type ReactNode } from 'react';

/** The site's name, last in every page's `<title>` (consistency plan C1). */
export const SITE_TITLE = 'CKBadger';

/**
 * `<name> · <kind> · CKBadger` (C1): the parts in order, empty ones left out, the site name last.
 * Entity pages give `[name, kind]` (`['#20,894,843', 'block']`), topic and list pages `[title]`.
 */
export function formatDocumentTitle(parts: readonly string[]): string {
  return [...parts.filter((part) => part !== ''), SITE_TITLE].join(' · ');
}

/**
 * Names the page in the browser tab while it is mounted and puts the previous title back on
 * unmount. The four shared headers call it (`DetailLayout`, `AssetDetailShell`, `TopicHero`,
 * `PageHeader`); a page without one of them calls it directly.
 */
export function useDocumentTitle(parts: readonly string[]): void {
  const title = formatDocumentTitle(parts);
  useEffect(() => {
    const previous = document.title;
    document.title = title;
    return () => {
      document.title = previous;
    };
  }, [title]);
}

/**
 * `useDocumentTitle` as an element, for a page without one of the four shared headers: it names
 * the tab only from the branch that renders the page's main view, so a `NotFoundPage` or
 * `CrashPage` that the page renders instead keeps its own title.
 */
export function DocumentTitle({ parts }: { parts: readonly string[] }): null {
  useDocumentTitle(parts);
  return null;
}

/** A hash as a tab names it (C1): `0xdcd1…334d`, the first 4 and last 4 hex digits. */
export function titleHash(hash: string): string {
  if (!/^0x[0-9a-f]{9,}$/i.test(hash)) throw new Error(`titleHash: not a hash: ${hash}`);
  return `${hash.slice(0, 6)}…${hash.slice(-4)}`;
}

/** An address as a tab names it (C1): `ckb1qzda…hprdnn`, the first 8 and last 6 characters. */
export function titleAddress(address: string): string {
  if (!/^ck[bt]1[0-9a-z]{14,}$/.test(address))
    throw new Error(`titleAddress: not a CKB address: ${address}`);
  return `${address.slice(0, 8)}…${address.slice(-6)}`;
}

/**
 * A shared header's title props (C1): a plain-string heading names the tab by itself; a heading
 * built from markup must say the page's name in `documentTitle`, so no page is left unnamed.
 */
export type DocumentTitleProps =
  | { title: string; documentTitle?: readonly string[] }
  | { title: ReactNode; documentTitle: readonly string[] };

/** The parts a shared header passes to `useDocumentTitle`: `documentTitle`, else the string heading. */
export function documentTitleParts(
  title: ReactNode,
  documentTitle: readonly string[] | undefined,
  kind?: string
): readonly string[] {
  if (documentTitle !== undefined) return documentTitle;
  if (typeof title === 'string') return kind === undefined ? [title] : [title, kind];
  throw new Error(
    'A page heading built from markup needs documentTitle: the browser tab cannot be named from it'
  );
}
