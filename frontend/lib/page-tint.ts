/**
 * The page's class hue for the ambient pools (material pass v2, move 1): the page root carries
 * `data-tint`, read by the pool variables in app/globals.css. The route is the one shared place
 * that knows every page's kind — the DAO, token, object and identity pages each draw their own
 * hero, so no shared header could name it — and the tint follows the path after the network
 * segment: `dao` pages gold, token pages azure, object / cluster / class pages violet, identity
 * pages rose, everything else jade (the default the :root variables already hold).
 */
export const PAGE_TINTS = ['jade', 'gold', 'azure', 'violet', 'rose'] as const;
export type PageTint = (typeof PAGE_TINTS)[number];

/** `path` is the route path after the network segment, e.g. `/dao`, `/tokens/0x…`, `/`. */
export function tintOfPath(path: string): PageTint {
  if (!path.startsWith('/'))
    throw new Error(`page tint: path ${JSON.stringify(path)} is not absolute`);
  const [head, next] = path.split('/').filter(Boolean);
  if (head === undefined) return 'jade';
  if (head === 'dao') return 'gold';
  const view = head === 'inventory' ? next : head;
  if (view === 'tokens') return 'azure';
  if (view === 'objects' || view === 'clusters' || view === 'classes') return 'violet';
  if (view === 'identities') return 'rose';
  return 'jade';
}
