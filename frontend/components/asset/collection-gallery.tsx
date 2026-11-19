'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type CollectionItem, type ItemStatusFilter } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { Pager } from '@/components/ui/pager';
import { rankedPageAt } from '@/lib/pagination';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { TextField } from '@/components/ui/text-field';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { AppLink } from '@/components/ui/app-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { AssetReadState } from '@/components/asset/detail-shell';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { formatStandard } from '@/components/activity-event-row';
import { formatBlockNumber } from '@/lib/format-block';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { useFromSm } from '@/hooks/useFromMd';
import { PHONE_LINE_PX, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import type { CompositionTier } from '@/lib/asset-utils';
// F2 objects (round 2, OB-C3): the 24-card gallery was replaced by the Collection board's
// Newest list (`collection-objects.tsx`).
export interface CollectionItemsProps {
  id: string;
  kind: 'object' | 'identity';
  /**
   * The composition every live object of the collection shares, its rows' chip (the owner's rule:
   * on every row of an object list); null where they do not share one, so no row can say its own.
   */
  tier?: CompositionTier | null;
}
// ── F2 identities (round 2): R3 owners on item rows (ID-COL-2 / ID-COL-3) ──
/**
 * An item row's owner as the API names it: its address; a lock hash whose script is not
 * indexed, as a hash; a `.cell` owner prefix no indexed lock has, as that prefix. A lock hash is
 * never passed off as an address, and a named owner is never reported as "No live owner".
 */
function ItemOwner({ item }: { item: CollectionItem }) {
  if (item.ownerAddress)
    return <Address address={item.ownerAddress} size="row" className="text-meta" />;
  if (item.ownerLockHash)
    return (
      <span className="text-meta">
        <Hash hash={item.ownerLockHash} size="row" className="text-meta" />
        <span className="text-text-dim block">address not indexed</span>
      </span>
    );
  if (item.ownerHashPrefix)
    return (
      <span className="text-meta [overflow-wrap:anywhere]">
        <Hash hash={item.ownerHashPrefix} truncate={false} className="text-meta" />
        <span className="text-text-dim block">named owner prefix · no indexed lock</span>
      </span>
    );
  if (item.isLive)
    return (
      <span role="alert" className="text-text-dim text-meta">
        {item.nftId}: a live item with no owner in the API response
      </span>
    );
  return <span className="text-text-dim text-meta">No live owner</span>;
}

/**
 * Round 3 (contracts R-A · R-1, OB-K3): an m-NFT token has no name of its own; its row reads
 * "<class name> #<token index>" (only "#<index>" for a class without an on-chain name).
 */
function mnftRowTitle(item: CollectionItem): ReactNode {
  if (item.tokenIndex === undefined)
    return (
      <span role="alert" className="text-rouge text-small font-mono">
        m-NFT item {item.nftId} has no tokenIndex
      </span>
    );
  return item.name === null ? `#${item.tokenIndex}` : `${item.name} #${item.tokenIndex}`;
}

/**
 * Who holds an item, on its phone row: "held by" and the owner's address at the row size (as the
 * board writes it, ckb1… and its last 8; with the chip, 295.8 of 328 px), or what the API says in
 * its place (a lock hash with no address, a `.cell` owner prefix), as the desktop row's ItemOwner
 * does; an item with no live owner says so.
 */
function itemHolder(item: CollectionItem): ReactNode {
  if (item.ownerAddress)
    return (
      <Labelled label="held by">
        <Address address={item.ownerAddress} size="row" look="inline" />
      </Labelled>
    );
  if (item.ownerLockHash)
    return (
      <Dotted
        parts={[
          <Labelled key="lock" label="held by">
            <Hash hash={item.ownerLockHash} size="row" />
          </Labelled>,
          <span key="note" className="text-text-dim">
            address not indexed
          </span>,
        ]}
      />
    );
  if (item.ownerHashPrefix)
    return (
      <Dotted
        parts={[
          <Labelled key="prefix" label="held by">
            <Hash hash={item.ownerHashPrefix} truncate={false} />
          </Labelled>,
          <span key="note" className="text-text-dim">
            named owner prefix · no indexed lock
          </span>,
        ]}
      />
    );
  if (item.isLive)
    return (
      <span role="alert" className="text-text-dim">
        {item.nftId}: a live item with no owner in the API response
      </span>
    );
  return <span className="text-text-dim">No live owner</span>;
}

/**
 * An item on a phone (the phone-rows board, its collection items pane; PB-03): the name has line 1
 * to itself, wrapping between its words and never cut (a 27-character name at 14 px, 226.8 px, and
 * the composition chip with its 12 px, about 120, are 357 px, past a 360 px phone's 328 px line:
 * so the chip stands on the holder's line); who holds it, and on an object collection's row the
 * chip held at that line's right edge (the owner's rule); where it was minted, named, and its state
 * where it is not live, quiet.
 */
function CollectionItemPhoneRow({
  item,
  href,
  kind,
  tier,
}: {
  item: CollectionItem;
  href: string;
  kind: 'object' | 'identity';
  tier: CompositionTier | null;
}) {
  return (
    <PhoneRow
      testId="collection-item-row-mobile"
      head={ITEM_NAME_LINE_PX}
      lines={[
        {
          key: 'name',
          tone: 'head',
          left: (
            <AppLink href={href} look="key">
              {itemName(item)}
            </AppLink>
          ),
        },
        {
          key: 'holder',
          tone: 'body',
          hold: true,
          left: itemHolder(item),
          right:
            kind === 'object' ? (
              tier ? (
                <CompositionBadge tier={tier} size="sm" />
              ) : (
                <span className="text-text-dim">composition not recorded</span>
              )
            ) : undefined,
        },
        {
          key: 'minted',
          tone: 'quiet',
          left: (
            <Dotted
              parts={[
                <Labelled key="minted" label="minted at">
                  <AppLink href={`/blocks/${item.createdAtBlock}`} look="inline">
                    {formatBlockNumber(item.createdAtBlock)}
                  </AppLink>
                </Labelled>,
                !item.isLive && <span key="state">inactive</span>,
              ]}
            />
          ),
        },
      ]}
    />
  );
}

/**
 * Line 1 of an item's phone row: its name alone at the key's 14 px, 39 characters on the 328 px line
 * of a 360 px phone (327.6; 42 at 390); a longer name wraps between its words, never cut.
 */
const ITEM_NAME_LINE_PX = phoneHead(
  'collection items',
  textPx(Math.floor(PHONE_LINE_PX[360] / textPx(1, fontPx('body'))), fontPx('body'))
);

/** An item's name as its row says it: an m-NFT's class name and index, else its name or DID. */
function itemName(item: CollectionItem): ReactNode {
  return item.standard === 'm-nft'
    ? mnftRowTitle(item)
    : (item.name ?? item.did ?? <Hash hash={item.nftId} size="row" className="text-meta" />);
}

/** The status filter's values, as the heading's args say them (the chips' own words). */
const STATUS_WORDS: Record<ItemStatusFilter, string> = {
  all: 'all',
  live: 'live',
  recycled: 'inactive',
};
/** The status filter's chips: all, live, inactive. */
const STATUS_VALUES: readonly FilterValue<ItemStatusFilter>[] = (
  ['all', 'live', 'recycled'] as const
).map((key) => ({ key, label: STATUS_WORDS[key] }));

export function CollectionItems({ id, kind, tier = null }: CollectionItemsProps) {
  const fromSm = useFromSm();
  const [search, setSearch] = useState(''),
    // Live first: live cells are the state (DATA_DESIGN); a .bit collection is 97 % recycled names,
    // so 'all' opened on a screen of names with no live owner (review 2026-11-03, item 3).
    [status, setStatus] = useState<ItemStatusFilter>('live');
  const pageSize = usePageSize(24);
  // a change of page size at md starts the walk again (lane c2-finish, 1d)
  const pages = useCursorPagination(pageSize);
  const query = useQuery({
    queryKey: ['collection-items', kind, id, search, status, pages.cursor, pageSize],
    queryFn: pages.ask((cursor) =>
      kind === 'object'
        ? api.getObjectCollectionItems(id, {
            limit: pageSize,
            cursor,
            search: search || undefined,
            status,
          })
        : api.getIdentityCollectionItems(id, {
            limit: pageSize,
            cursor,
            search: search || undefined,
            status,
          })
    ),
  });
  const itemsAt = query.data
    ? rankedPageAt(kind === 'identity' ? 'identities' : 'objects', query.data, pages)
    : null;
  const href = (item: CollectionItem) =>
    kind === 'identity'
      ? getIdentityItemDetailHref(item.standard, item.nftId)
      : item.standard === 'm-nft'
        ? `/objects/mnft/${item.nftId}`
        : `/objects/${item.nftId}`;
  return (
    <section aria-label="Collection items" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        {/* Rubbing × Song print, phase 5 (A12): the items, run on how many the chosen status
            holds and that status. Before the page loads, and on a search (the API gives a search
            page no total), it names the status alone. The controls stay beside. */}
        <PromptHeading
          command={kind === 'identity' ? 'Identities' : 'Objects'}
          args={
            query.data?.total === undefined
              ? STATUS_WORDS[status]
              : `${query.data.total.toLocaleString('en-US')} ${STATUS_WORDS[status]}`
          }
        />
        {/* the controls board's field and filter chips: a search and one status of three (a
            filter of three values is chips at every width: 195 px in a 328 px line) */}
        <div className="flex max-w-full flex-wrap items-center gap-3">
          <TextField
            label="Search collection"
            search
            value={search}
            onChange={(value) => {
              pages.reset();
              setSearch(value);
            }}
            placeholder="search names or ids"
            className="w-[200px] max-w-full"
          />
          <FilterChips
            label="status"
            values={STATUS_VALUES}
            value={status}
            onChange={(value) => {
              pages.reset();
              setStatus(value);
            }}
          />
        </div>
      </div>
      {query.data ? (
        <>
          <AssetReadState label="collection items" error={query.error} retained />
          {/* Below sm, the phone-rows board's row (PB-03); from sm the three-column grid. */}
          {fromSm ? (
            <div className="divide-base-border divide-y">
              {query.data.data.map((item) => (
                <article
                  key={item.nftId}
                  className="grid min-w-0 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
                >
                  <div className="min-w-0">
                    <AppLink href={href(item)} className="text-text-bright break-words">
                      {/* F2 identities (round 2): a did:ckb row has no name but its DID (R2b-b-22). */}
                      {item.standard === 'm-nft'
                        ? mnftRowTitle(item)
                        : (item.name ??
                          item.did ?? <Hash hash={item.nftId} size="row" className="text-meta" />)}
                    </AppLink>
                    <span className="text-text-dim text-meta mt-1 block">
                      {formatStandard(item.standard)} · {item.isLive ? 'Live' : 'Inactive'}
                    </span>
                  </div>
                  <div className="min-w-0">
                    <ItemOwner item={item} />
                  </div>
                  <AppLink href={`/blocks/${item.createdAtBlock}`} className="text-meta">
                    {formatBlockNumber(item.createdAtBlock)}
                  </AppLink>
                </article>
              ))}
            </div>
          ) : (
            <div className="divide-base-border divide-y">
              {query.data.data.map((item) => (
                <div key={item.nftId} className="py-2.5">
                  <CollectionItemPhoneRow item={item} href={href(item)} kind={kind} tier={tier} />
                </div>
              ))}
            </div>
          )}
          {query.data.data.length === 0 && <p className="text-text-dim">No items.</p>}
          {/* The library's pager (lane c2-finish, 1a): counted of the chosen status's total
              where this page's response states it, an ordered list with no count on a search
              page (the API gives a search no total), nothing for one page. */}
          {itemsAt && (
            <Pager
              mode="pages"
              label={kind === 'identity' ? 'identities' : 'objects'}
              {...itemsAt}
            />
          )}
        </>
      ) : (
        <AssetReadState label="collection items" error={query.error} read={query} />
      )}
    </section>
  );
}
