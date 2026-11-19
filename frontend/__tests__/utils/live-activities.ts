import live from '@/__tests__/fixtures/activities-live-mainnet.json';
import type { GlobalActivity } from '@/lib/api';

/**
 * The live capture's feeds (`fixtures/activities-live-mainnet.json`, said in it: 2026-11-19, before
 * the indexer recorded a row's recognised type codes, C7) as the page reads a row now. The capture
 * states `itemTypes`, the codes of the items and the Nervos DAO steps its rows moved, as the API of
 * then resolved them; the API now states `recognizedTypes`, every recognised code that ran, read
 * from the row: the same codes, and besides them a recognised code that ran and moved nothing (a
 * wallet merging its own token cells), which the capture cannot say. So each row's `itemTypes` is
 * read as its `recognizedTypes`, exact on every row whose recognised codes each moved an item or a
 * DAO step; a row whose recognised script moved nothing reads as it did then, until the capture is
 * re-taken over a re-synced store. A row whose item types the API of then could not state is
 * refused: it has no list to read.
 */
export function liveFeeds(): Record<string, GlobalActivity[]> {
  type Captured = Omit<GlobalActivity, 'recognizedTypes'> & {
    itemTypes: GlobalActivity['recognizedTypes'] | null;
    itemTypesUnread: string | null;
  };
  const feeds = (live as unknown as { feeds: Record<string, Captured[]> }).feeds;
  return Object.fromEntries(
    Object.entries(feeds).map(([feed, rows]) => [
      feed,
      rows.map(({ itemTypes, itemTypesUnread, ...row }) => {
        if (itemTypes === null)
          throw new Error(`${row.txHash}: the capture states no item types (${itemTypesUnread})`);
        return { ...row, recognizedTypes: itemTypes };
      }),
    ])
  );
}
