'use client';

import { useState } from 'react';
import { LockKind } from '@/components/ui/lock-kind';
import { useQuery } from '@tanstack/react-query';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { ENTITY_PAGE_SIZE } from '@/lib/pagination';
import { AssetCount, wordHeads } from '@/components/asset/inventory-table';
import { AssetReadState } from '@/components/asset/detail-shell';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { DetailSection } from '@/components/ui/detail-layout';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import {
  AlignedBar,
  BAR_FIGURE_GAP,
  COUNT_TRACK_PX,
  ScaleHeader,
} from '@/components/ui/aligned-bar';
import { rowTagClass } from '@/components/ui/row-tag';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatTick } from '@/lib/chart-ticks';
import { HolderPhoneRow } from '@/components/asset/holder-phone-row';
import { PhoneRow } from '@/components/ui/phone-row';
import { countNoun } from '@/lib/format-count';
import { SHORT_FORMS, type ShortSize } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { shortChars } from '@/lib/list-bounds';

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
  const pagination = useCursorPagination();
  // "All holders" pages by the list size (R2: twenty below md); the top ten stay ten.
  const allPageSize = usePageSize(ENTITY_PAGE_SIZE);
  const limit = all ? allPageSize : 10;
  const holders = useQuery({
    queryKey: ['collection-holders', queryKey, all, pagination.cursor, limit],
    queryFn: () => load({ limit, cursor: pagination.cursor }),
  });
  let totalHolders = BigInt(0);
  let totalItems = BigInt(0);
  const ranges = new Set<string>();
  for (const bucket of distribution ?? []) {
    if (!bucket.range || ranges.has(bucket.range))
      throw new Error('Collection holder distribution has an empty or repeated range');
    ranges.add(bucket.range);
    const n = count(bucket.holders, 'bucket holders');
    const items = count(bucket.items, 'bucket items');
    if (items < n || (n === BigInt(0) && items !== BigInt(0)))
      throw new Error(`Collection holder distribution has inconsistent counts: ${bucket.range}`);
    totalHolders += n;
    totalItems += items;
  }
  for (const holder of holders.data?.data ?? []) {
    holderKey(holder);
    count(holder.itemCount, 'holder item count');
  }
  // The list (server-cached up to 30 s) and the distribution are separate reads: when a page
  // lists more holders than the distribution counts, say so and show both (#6).
  const readsDiffer =
    !!distribution && !!holders.data && BigInt(holders.data.data.length) > totalHolders;
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
      holder.lockCodeHash === null ? 'lock not indexed' : (holder.lockKind ?? holder.lockCodeHash),
      `${holder.itemCount.toLocaleString('en-US')} ${countNoun(holder.itemCount, 'item', 'items')}`,
    ].join(' · ');
  const listed = holders.data?.data ?? [];
  const columns: ListColumn<CollectionHolder>[] = [
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
      // The flexible column (readability F3-e): its tag holds 12 characters of the kind at its
      // minimum (its words at 12 px, 5 px sides, a 1 px border) and the gap, and takes what the
      // container gives beyond the sum; a longer kind ends in the one ellipsis. It was 116.
      minWidth: columnPx(textPx(12, fontPx('meta')) + 2 * 5 + 2 * 1 + CELL_GAP_PX),
      width: 'auto',
      render: (holder) => (
        <span className="flex min-w-0">
          {/* a column headed "Lock" is never empty: the default lock by its name (B4) */}
          <LockKind
            lockKind={holder.lockKind}
            lockCodeHash={holder.lockCodeHash}
            size="row"
            place="column"
            className={rowTagClass('default', 'min-w-0 truncate')}
          />
        </span>
      ),
    },
    countColumn('items', 'Items', listed, (holder) => holder.itemCount, assetClass, true),
  ];
  const bucketColumns: ListColumn<NonNullable<CollectionHoldersProps['distribution']>[number]>[] = [
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
    <DetailSection
      title="Who holds it"
      actions={
        <button
          type="button"
          className="text-jade text-meta"
          onClick={() => {
            setAll(!all);
            pagination.reset();
          }}
        >
          {all ? 'Top 10 holders' : 'All holders →'}
        </button>
      }
    >
      <div
        className={
          distribution ? 'grid min-w-0 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]' : 'min-w-0'
        }
      >
        <div className="min-w-0">
          {holders.data ? (
            <>
              <AssetReadState error={holders.error} label="collection holders" retained />
              <ListTable
                label={all ? 'All collection holders' : 'Top 10 collection holders'}
                columns={wordHeads(columns)}
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
                      lock={
                        <LockKind
                          lockKind={holder.lockKind}
                          lockCodeHash={holder.lockCodeHash}
                          size={phoneSize}
                        />
                      }
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
                      wide={wide}
                    />
                  );
                }}
              />
              {all && (
                <CursorPagination
                  page={pagination.page}
                  pageSize={limit}
                  total={holders.data.total ?? undefined}
                  totalLabel="holders"
                  currentCount={holders.data.data.length}
                  hasMore={holders.data.hasMore}
                  hasPrevious={pagination.hasPrevious}
                  onNext={() => pagination.goToNext(holders.data.nextCursor)}
                  onPrevious={pagination.goToPrevious}
                  className="[&>div>span]:text-meta [&>span]:text-meta [&_button]:text-meta flex-wrap gap-3 [&>div]:flex-wrap [&>div]:gap-2 [&_button]:px-3"
                />
              )}
            </>
          ) : (
            <AssetReadState error={holders.error} read={holders} label="collection holders" />
          )}
        </div>
        {distribution && (
          <div className="min-w-0">
            <p className="text-text-dim text-meta mb-2 break-words">
              Full collection: <AssetCount value={totalHolders.toString()} /> holders ·{' '}
              <AssetCount value={totalItems.toString()} /> items
            </p>
            {readsDiffer && (
              <p role="note" className="text-text-dim text-meta mb-2">
                The holder list ({holders.data!.data.length.toLocaleString('en-US')} shown) and the
                distribution ({totalHolders.toLocaleString('en-US')} holder
                {totalHolders === BigInt(1) ? '' : 's'}) were read at different moments.
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
          </div>
        )}
      </div>
    </DetailSection>
  );
}
