'use client';

import { useState } from 'react';
import { LockKind, LockWordsView, lockWordsText, rowLock } from '@/components/ui/lock-kind';
import { useQuery } from '@tanstack/react-query';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { ENTITY_PAGE_SIZE, rankedPageAt } from '@/lib/pagination';
import { AssetCount, wordHeads } from '@/components/asset/inventory-table';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { DetailSection } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Pager } from '@/components/ui/pager';
import { FoldBack } from '@/components/transaction/tx-structure';
import { listTotal } from '@/components/asset/token-holders';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import {
  AlignedBar,
  BAR_FIGURE_GAP,
  COUNT_TRACK_PX,
  ScaleHeader,
} from '@/components/ui/aligned-bar';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { PhoneRow } from '@/components/ui/phone-row';
import { countNoun } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { WIDEST_HELD_ITEMS, shortChars } from '@/lib/list-bounds';
import { PartReadBoundary } from '@/components/read-failure';

/** A run of characters at the cells' 13 px (readability F3-e: every column budgets what it holds). */
const cell = (chars: number) => textPx(chars, fontPx('small'));

export interface CollectionHoldersProps {
  load: (params: {
    limit?: number;
    cursor?: string;
  }) => Promise<CursorPaginatedResponse<CollectionHolder>>;
  queryKey: string;
  distribution?: readonly { range: string; holders: number; items: number }[];
  /** F2 objects (OB-C7), PB-12: the counts are bars in the collection's class colour. */
  assetClass: BarClass;
}

/** The asset classes a row bar can be drawn in (`AlignedBar` tones). */
type BarClass = Extract<AssetClass, 'tokens' | 'objects' | 'identities' | 'dao'>;

/**
 * Line 1 of a collection's holder on a phone (the phone-rows board's head()): a rank of two digits in
 * its 22 px box, the holder's address at the tight size and the key's 14 px (92.4), the 12 px between
 * the parts and the most objects one holder holds of one collection, "999,999 " at 14 px (67.2) and
 * "items" at 13 px (39; WIDEST_HELD_ITEMS): 232.6 of a 360 px phone's 328 px line.
 */
const COLLECTION_HOLDER_PHONE_HEAD_PX = phoneHead(
  'collection holders',
  22 +
    textPx(shortChars(SHORT_FORMS.tight.address), fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(`${WIDEST_HELD_ITEMS} `.length, fontPx('body')) +
    textPx('items'.length, fontPx('small'))
);

/**
 * The count bars' track; the exact count prints beside it (table-rows L2-4). Readability F3-e:
 * the track holds its head's ticks ("0" … "750M", the tick form) whole, under the 48 px a bar
 * keeps at least, so 48; the count is a holder's items or a range's holders, under 100,000:
 * "99,999" (6 characters at 13 px, 46.8; it was 44, five).
 */
const TRACK = COUNT_TRACK_PX;
const WIDEST_COUNT = '99,999';
const COUNT_WIDTH = columnPx(cell(WIDEST_COUNT.length));

/**
 * F2 objects (OB-C7) on the row rules: a count as an 8 px bar in the class colour on the column's
 * round scale, its exact number beside it.
 */
function countColumn<T>(
  key: string,
  label: string,
  rows: readonly T[],
  value: (row: T) => number,
  assetClass: BarClass,
  /** The table's last column keeps no gap after it. */
  last = false
): ListColumn<T> {
  const scale = resourceScale(rows.map(value));
  return {
    key,
    label,
    // the track, the bar's 10 px gap, the count and the gap, none after the last column
    minWidth: columnPx(TRACK + BAR_FIGURE_GAP + COUNT_WIDTH + (last ? 0 : CELL_GAP_PX)),
    header: <ScaleHeader label={label} scale={scale} trackWidth={TRACK} />,
    render: (row) => (
      <AlignedBar
        label={label}
        scale={scale}
        trackWidth={TRACK}
        minFill={2}
        segments={[{ key, label, value: value(row), tone: assetClass }]}
        display={
          <span>
            <AssetCount value={value(row)} />
          </span>
        }
        displayWidth={COUNT_WIDTH}
      />
    ),
  };
}

function count(value: number, label: string): bigint {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`Invalid collection ${label}: ${value}`);
  return BigInt(value);
}

function holderKey(holder: CollectionHolder): string {
  const key = holder.lockScriptHash ?? holder.ownerHashPrefix ?? holder.address;
  if (!key) throw new Error('Collection holder has no lock or named owner prefix');
  return key;
}

export function CollectionHolders(props: CollectionHoldersProps) {
  // A different collection starts a new cursor scope and Top 10 view.
  return <ScopedCollectionHolders key={props.queryKey} {...props} />;
}

function ScopedCollectionHolders({
  load,
  queryKey,
  distribution,
  assetClass,
}: CollectionHoldersProps) {
  const [all, setAll] = useState(false);
  // "All holders" pages by the list size (R2: twenty below md); the top ten stay ten.
  const allPageSize = usePageSize(ENTITY_PAGE_SIZE);
  const limit = all ? allPageSize : 10;
  // a change of page size (at md, or the top ten opened to all) starts the walk again (lane
  // c2-finish, 1d)
  const pagination = useCursorPagination(limit);
  const showAll = (open: boolean) => {
    setAll(open);
    pagination.reset();
  };
  const holders = useQuery({
    queryKey: ['collection-holders', queryKey, all, pagination.cursor, limit],
    queryFn: pagination.ask((cursor) => load({ limit, cursor })),
  });
  // The distribution's totals, read while its column is drawn: a distribution that breaks its
  // invariants fails its own column in place (D7), the holders' table and the heading standing.
  const totalsOf = () => {
    let holdersTotal = BigInt(0);
    let itemsTotal = BigInt(0);
    const ranges = new Set<string>();
    for (const bucket of distribution ?? []) {
      if (!bucket.range || ranges.has(bucket.range))
        throw new Error('Collection holder distribution has an empty or repeated range');
      ranges.add(bucket.range);
      const n = count(bucket.holders, 'bucket holders');
      const items = count(bucket.items, 'bucket items');
      if (items < n || (n === BigInt(0) && items !== BigInt(0)))
        throw new Error(`Collection holder distribution has inconsistent counts: ${bucket.range}`);
      holdersTotal += n;
      itemsTotal += items;
    }
    return { holders: holdersTotal, items: itemsTotal };
  };
  // the holders' table: where it is drawn, by its view and page (its part starts again on another)
  const holdersWhere = `collection ${queryKey}, ${all ? `all holders, page ${pagination.page}` : 'top 10'}`;
  const rank = (holder: CollectionHolder) =>
    (pagination.page - 1) * limit + holders.data!.data.indexOf(holder) + 1;
  // the holder at the form's size (U-09): row in the table and in a phone row from sm, tight in
  // a phone row below it (R9)
  const phoneSize = useTightBelowSm();
  const party = (holder: CollectionHolder, size: ShortSize) => (
    <DotCellParty
      party={{
        hashPrefix: holder.ownerHashPrefix ?? null,
        lockHash: holder.lockScriptHash,
        address: holder.address,
      }}
      size={size}
      // Table-rows L2-4: the holder is the row's bright key, on one 18 px line.
      className="[&>span]:text-small [&_a]:text-text-bright [&>span]:leading-row [&_a]:leading-row max-w-full flex-nowrap whitespace-nowrap [&>span]:py-0"
    />
  );
  const holderTitle = (holder: CollectionHolder) =>
    [
      `#${rank(holder)}`,
      holder.address ??
        holder.lockScriptHash ??
        `${holder.ownerHashPrefix ?? holderKey(holder)} (unresolved)`,
      // what its Lock cell says, from the one decision (B4)
      lockWordsText(rowLock(holder, 'column')!, 'row'),
      `${holder.itemCount.toLocaleString('en-US')} ${countNoun(holder.itemCount, 'item', 'items')}`,
    ].join(' · ');
  const listed = holders.data?.data ?? [];
  // "All holders" pages through the list; the top ten are one page
  const holdersAt = all && holders.data ? rankedPageAt('holders', holders.data, pagination) : null;
  // the holders' columns, built while their table is drawn (inside its part)
  const columnsOf = (): ListColumn<CollectionHolder>[] => [
    {
      key: 'rank',
      label: '#',
      // A rank of five digits (a collection's holders under 100,000), "99999" (39 at 13 px), and
      // the gap; it was 32, two digits.
      minWidth: columnPx(cell('99999'.length) + CELL_GAP_PX),
      className: 'whitespace-nowrap',
      render: (holder) => <span className="text-text-bright">{rank(holder)}</span>,
    },
    {
      key: 'owner',
      label: 'Holder',
      // The widest holder: an owner known by its hash prefix alone, the prefix at the row size (17
      // characters at 13 px, 132.6), its 8 px gap and the "unresolved" badge (78, 8 px sides and
      // a 1 px border), and the gap: 252.6. It was 196, which held an address.
      minWidth: columnPx(
        cell(shortChars(SHORT_FORMS.row.hash)) +
          8 +
          cell('unresolved'.length) +
          2 * 8 +
          2 * 1 +
          CELL_GAP_PX
      ),
      render: (holder) => party(holder, 'row'),
    },
    {
      key: 'lock',
      label: 'Lock',
      // The flexible column (readability F3-e): 12 characters of the lock's name at the cells'
      // 13 px (93.6) and the gap at its minimum, and what the container gives beyond the sum; a
      // longer name ends in the one ellipsis. It was 115, a tag of 12 characters at 12 px with its
      // 5 px sides and 1 px border (98.4).
      minWidth: columnPx(cell(12) + CELL_GAP_PX),
      width: 'auto',
      render: (holder) => (
        <span className="flex min-w-0">
          {/* a column headed "Lock" is never empty: the default lock by its name (B4); a lock is
              plain words, no chip (lane p-assets; D5 as the readability plan changed it) */}
          <LockKind
            lockKind={holder.lockKind}
            lockCodeHash={holder.lockCodeHash}
            address={holder.address}
            size="row"
            place="column"
            className="text-text text-small min-w-0 truncate"
          />
        </span>
      ),
    },
    countColumn('items', 'Items', listed, (holder) => holder.itemCount, assetClass, true),
  ];
  // the distribution's columns, built while its column is drawn (inside its part)
  const bucketColumnsOf = (): ListColumn<
    NonNullable<CollectionHoldersProps['distribution']>[number]
  >[] => [
    {
      key: 'range',
      label: 'Holds',
      // The head, "Holds" (36 at 12 px), wider than the widest range the API writes, "3–5" (25.2
      // at 14 px), and the gap. It was 65.
      minWidth: columnPx(
        Math.max(textPx('Holds'.length, fontPx('meta')), textPx('3–5'.length, fontPx('body'))) +
          CELL_GAP_PX
      ),
      render: (bucket) => (
        <span className="text-body text-text-bright leading-row">{bucket.range}</span>
      ),
    },
    countColumn('holders', 'Holders', distribution ?? [], (bucket) => bucket.holders, assetClass),
    {
      key: 'items',
      label: 'Items',
      // A range's items under a million, "999,999" (54.6 at 13 px); the last column, no gap after
      // it. It was 100.
      minWidth: columnPx(cell('999,999'.length)),
      numeric: true,
      render: (bucket) => (
        <span>
          <AssetCount value={bucket.items} />
        </span>
      ),
    },
  ];
  return (
    <DetailSection title="Who holds it">
      <div
        className={
          distribution ? 'grid min-w-0 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]' : 'min-w-0'
        }
      >
        <div className="min-w-0">
          {holders.data ? (
            <>
              <AssetReadState error={holders.error} label="collection holders" retained />
              {/* a refusal while the holders are drawn fails their table in its place (D7) */}
              <PartReadBoundary
                what="the collection holders"
                where={holdersWhere}
                render={() => {
                  for (const holder of holders.data!.data) {
                    holderKey(holder);
                    count(holder.itemCount, 'holder item count');
                  }
                  return (
                    <ListTable
                      label={all ? 'All collection holders' : 'Top 10 collection holders'}
                      columns={wordHeads(columnsOf())}
                      rows={holders.data.data}
                      rowKey={holderKey}
                      empty="No current holders."
                      density="row"
                      rowTitle={holderTitle}
                      mobileRow={(holder, wide) => {
                        // R7: the table's scale for its Items bars — the page's rounded maximum.
                        const scale = resourceScale(listed.map((h) => h.itemCount));
                        return (
                          <HolderPhoneRow
                            rank={rank(holder)}
                            party={party(holder, phoneSize)}
                            lock={(() => {
                              // no part where the row names no lock (B4)
                              const words = rowLock(holder, 'inline');
                              return words && <LockWordsView words={words} size={phoneSize} />;
                            })()}
                            amount={
                              <span className="whitespace-nowrap">
                                <AssetCount value={holder.itemCount} />{' '}
                                <span className="text-text-dim text-small">
                                  {countNoun(holder.itemCount, 'item', 'items')}
                                </span>
                              </span>
                            }
                            bar={{
                              value: holder.itemCount,
                              scale,
                              color: CLASS_COLORS[assetClass],
                            }}
                            axis={listed[0] === holder ? formatTick(scale, scale) : undefined}
                            head={COLLECTION_HOLDER_PHONE_HEAD_PX}
                            wide={wide}
                          />
                        );
                      }}
                    />
                  );
                }}
              />
              {all ? (
                <div className="mt-3 space-y-1">
                  {holdersAt && (
                    // the controls board's pager: a ranked list in pages, every figure the
                    // response's
                    <Pager mode="pages" label="holders" {...holdersAt} />
                  )}
                  <FoldBack onFold={() => showAll(false)} />
                </div>
              ) : (
                holders.data.hasMore && (
                  // the short list's form, opened in place (the placement lane's toggles, the
                  // lead's ruling): "all 522 holders ↓", the count its response's own
                  <div className="mt-3">
                    <Pager
                      mode="more"
                      all={{
                        count: listTotal(holders.data, `the holders of collection ${queryKey}`),
                        singular: 'holder',
                        onShow: () => showAll(true),
                      }}
                    />
                  </div>
                )
              )}
            </>
          ) : (
            <AssetReadState error={holders.error} read={holders} label="collection holders" />
          )}
        </div>
        {distribution && (
          <div className="min-w-0">
            {/* a distribution that breaks its invariants fails this column in place (D7) */}
            <PartReadBoundary
              what="the holder distribution"
              where={`collection ${queryKey}`}
              render={() => {
                const totals = totalsOf();
                const bucketColumns = bucketColumnsOf();
                // The list (server-cached up to 30 s) and the distribution are separate reads:
                // when a page lists more holders than the distribution counts, say so and show
                // both (#6).
                const readsDiffer =
                  !!holders.data && BigInt(holders.data.data.length) > totals.holders;
                return (
                  <>
                    <p className="text-text-dim text-meta max-w-measure mb-2 break-words">
                      Full collection: <AssetCount value={totals.holders.toString()} /> holders ·{' '}
                      <AssetCount value={totals.items.toString()} /> items
                    </p>
                    {readsDiffer && (
                      <p role="note" className="text-text-dim text-meta max-w-measure mb-2">
                        The holder list ({holders.data!.data.length.toLocaleString('en-US')} shown)
                        and the distribution ({totals.holders.toLocaleString('en-US')} holder
                        {totals.holders === BigInt(1) ? '' : 's'}) were read at different moments.
                      </p>
                    )}
                    <ListTable
                      label="Holders by how many items they hold"
                      columns={wordHeads(bucketColumns)}
                      rows={distribution}
                      rowKey={(bucket) => bucket.range}
                      empty="No current holders."
                      density="row"
                      mobileRow={(bucket) => (
                        <PhoneRow
                          testId="holder-bucket-row-mobile"
                          lines={[
                            {
                              key: 'range',
                              tone: 'body',
                              left: (
                                <>
                                  holds {bucket.range}
                                  <span className="text-text-dim">
                                    {' · '}
                                    <AssetCount value={bucket.items} />{' '}
                                    {countNoun(bucket.items, 'item', 'items')}
                                  </span>
                                </>
                              ),
                              right: (
                                <span className="inline-flex items-center gap-1.5">
                                  {bucketColumns[1].render(bucket)}
                                  <span className="text-text-dim">
                                    {countNoun(bucket.holders, 'holder', 'holders')}
                                  </span>
                                </span>
                              ),
                            },
                          ]}
                        />
                      )}
                    />
                  </>
                );
              }}
            />
          </div>
        )}
      </div>
    </DetailSection>
  );
}
