// The captured item-feed rows, as a collection feed serves them: each row gains the API's
// `collectionContext` (contracts.md · Lane R · R2, OB-C13) in exactly the documented shape.
import type {
  CollectionFeedActivity,
  CollectionFeedTransaction,
  CursorPaginatedResponse,
} from '@/lib/api';
import {
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';

const strip = <T extends { itemContext?: unknown }>({ itemContext: _, ...row }: T) => row;

export const collectionActivityFeedProducer = {
  ...itemActivityFeedProducer,
  data: itemActivityFeedProducer.data.map((row) => ({
    ...strip(row),
    collectionContext: {
      action: 'transfer',
      minted: 0,
      burned: 0,
      transferred: 1,
      updated: 0,
      items: [{ id: row.itemContext.itemId, change: 'transferred' }],
    },
  })),
} satisfies CursorPaginatedResponse<CollectionFeedActivity>;

export const collectionTransactionsProducer = {
  ...itemTransactionsProducer,
  data: itemTransactionsProducer.data.map((row) => ({
    ...strip(row),
    collectionContext: {
      action: 'mint',
      minted: 1,
      burned: 0,
      transferred: 0,
      updated: 0,
      items: [{ id: row.itemContext.itemId, change: 'minted' }],
    },
  })),
} satisfies CursorPaginatedResponse<CollectionFeedTransaction>;
