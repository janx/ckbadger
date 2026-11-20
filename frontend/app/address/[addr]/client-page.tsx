'use client';

import { useId, useState } from 'react';
import { useInfiniteQuery, useQueries, useQuery, keepPreviousData } from '@tanstack/react-query';
import { NotFoundPage } from '@/components/not-found-page';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { Hash } from '@/components/ui/hash';
import { HeadId } from '@/components/ui/head-id';
import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { FilterChips } from '@/components/ui/filter-chips';
import { Pager } from '@/components/ui/pager';
import { Tabs } from '@/components/ui/tabs';
import {
  AddressBalance,
  addressLockName,
  addressStripFacts,
  AddressHoldingsSection,
  AddressLoadState,
  AddressStaleNotice,
  AddressPagination,
  ADDRESS_CLASSES,
} from '@/components/address/address-holdings';
import {
  AddressActivities,
  AddressTransactions,
  AddressLiveCells,
} from '@/components/address/address-history';
import { PoolTruncatedNotice, PoolUnavailableNotice } from '@/components/ui/pool-status';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError, type AddressCellClass } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { ReadFailure } from '@/components/read-failure';
import { useParams } from '@/src/navigation';
import { POOL_POLL_INTERVAL_MS, poolRefetchInterval } from '@/lib/pool-polling';
import { getIdentityItemDetailHref, getScriptDetailHref } from '@/lib/detail-routes';
import { formatUtc8Date } from '@/lib/utils';
import { CLASS_KEYS } from '@/lib/design-tokens';
import type { PaletteKey } from '@/lib/palette';
import { shortenAddress, shortenHash } from '@/lib/shorten';
import { apiBaseFor } from '@/lib/active-network';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { usePageSize } from '@/hooks/usePageSize';
import { ENTITY_PAGE_SIZE } from '@/lib/pagination';
import { titleAddress, titleHash } from '@/lib/use-document-title';
import { ScriptByteMap, ScriptNames } from '@/components/ui/script-byte-map';
import { unitNode } from '@/components/ui/units';

const activityFilters = [
  ['all', 'All'],
  ['ckb', 'CKB'],
  ['token', 'Token'],
  ['object', 'Object'],
  ['identity', 'Identity'],
  ['dao', 'DAO'],
  // R2b-a 8 (BA-35): one "Script" chip — a type call or a lock call, as the Address board.
  ['script', 'Script'],
  ['protocol:rgbpp', 'RGB++'],
] as const;
type ActivityFilter = (typeof activityFilters)[number][0];
/**
 * C14: what a filter chip's one-row page says of its count. The API counts only the unfiltered
 * feed, so a filtered page tells 0 or 1 committed activities exactly, and otherwise only that
 * there are more than one — `'many'`, which a chip shows as no number at all (lane P: exact or
 * none). (Page one also carries the pool's unconfirmed rows; they are not counted, as the
 * unfiltered total does not count them.)
 */
function chipCount(page: {
  data: readonly { poolStatus?: unknown }[];
  hasMore: boolean;
}): 0 | 1 | 'many' {
  const committed = page.data.filter((row) => row.poolStatus === undefined).length;
  if (committed > 1)
    throw new Error(`a filter chip's one-row page holds ${committed} committed activities`);
  if (page.hasMore) return 'many';
  return committed === 0 ? 0 : 1;
}
/** A chip's title when its count is not exact: what its one-row page proved. */
const MANY_TITLE = 'more than one; the API keeps no count of filtered activity';
/** The kinds a 7 px mark leads, in their class colour (the controls board's address in place). */
const ACTIVITY_MARKS: Partial<Record<ActivityFilter, PaletteKey>> = {
  token: CLASS_KEYS.tokens,
  object: CLASS_KEYS.objects,
  identity: CLASS_KEYS.identities,
  dao: CLASS_KEYS.dao,
};
/** One address activity page; the list and the tab count share it, so they share its cache entry. */
const activityPageQuery = (
  lockHash: string | undefined,
  cursor: string | undefined,
  filter: ActivityFilter,
  limit: number
) => ({
  queryKey: ['address-activities', lockHash, cursor, filter, limit] as const,
  queryFn: () => api.getAddressActivities(lockHash!, { limit, cursor, filter }),
});
export default function AddressDetailPage() {
  const params = useParams();
  const addr = params.addr as string;
  return <AddressDetailPageContent key={addr} addr={addr} />;
}
/**
 * The address's id line: the full address and its copy button, in the plate's voice (seals by
 * kind, C10 round 2: the plate's 13 px, 7.8 px a character). The address is the head's full id (A4
 * Chrome F3): one line where the measured line holds it (97 characters, 756.6 px), else in equal
 * unbroken lines — never the own-box scroller it was. Without an address the page is its lock hash,
 * a hash. The copy is HeadId's (the controls board's CopyButton, 8 px after the id): its icon in a
 * 24 px box from 640 and a 32 px one below it, its answer (`copied`, `not copied`) out of the
 * line's flow under the whole id (lane c2-finish, rulings round 1). The id measures the line the
 * box leaves it: from md, beside the 73.5 px stone, 1232 − 73.5 − 20 − 32 = 1106.5 px at 1280, one
 * line; at 640, 608 − 8 − 24 = 576 px, 73 characters, two lines of 49 and 48; at 390, 358 − 8 − 32
 * = 318 px, 40 characters, three of 33, 33 and 31. The answer takes no room, so a copy moves
 * nothing.
 */
function AddressIdentity({ value, kind }: { value: string; kind: 'address' | 'hash' }) {
  return (
    <HeadId
      value={value}
      kind={kind}
      copy={kind === 'address' ? 'copy address' : 'copy lock script hash'}
    />
  );
}
function AddressDetailPageContent({ addr }: { addr: string }) {
  const network = useActiveNetwork();
  const [tab, setTab] = useState<'activities' | 'transactions' | 'cells'>('activities');
  const [filter, setFilter] = useState<ActivityFilter>('all');
  const [cellClass, setCellClass] = useState<AddressCellClass | undefined>();
  const pageSize = usePageSize(ENTITY_PAGE_SIZE);
  // a change of page size at md starts each walk again (lane c2-finish, 1d), and so does a change
  // of the list a walk pages: the kind, the class (lane cd-fixes-controls)
  const activityPage = useCursorPagination(pageSize, { list: [filter] }),
    txPage = useCursorPagination(pageSize),
    cellPage = useCursorPagination(pageSize, { list: [cellClass] });
  const historyId = useId();
  const address = useQuery({
    queryKey: ['address', addr],
    queryFn: () => api.getAddress(addr),
    // R11: an id that is not one never becomes one; its 400 is not polled
    refetchInterval: (query) =>
      isBadRequestApiError(query.state.error) ? false : POOL_POLL_INTERVAL_MS,
  });
  const lockHash = address.data?.lockScriptHash;
  const holdings = useQuery({
    queryKey: ['address-holdings', lockHash],
    queryFn: () => api.getAddressHoldings(lockHash!),
    enabled: !!lockHash,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  // the list's page is asked by its walk (lane cd-fixes-controls, item 1: the page a response was
  // asked for travels with it), so it keeps a cache entry of its own beside the count's
  const activities = useQuery({
    queryKey: [
      ...activityPageQuery(lockHash, activityPage.cursor, filter, pageSize).queryKey,
      'page',
    ],
    queryFn: activityPage.ask((cursor) =>
      api.getAddressActivities(lockHash!, { limit: pageSize, cursor, filter })
    ),
    enabled: !!lockHash && tab === 'activities',
    placeholderData: keepPreviousData,
    refetchInterval: poolRefetchInterval({
      isPageOne: !activityPage.cursor,
      isActiveTab: tab === 'activities',
    }),
  });
  // contracts W5-1 (BA-35): the Activities tab counts the address's activities — the `total` of
  // the unfiltered first page (its transactions minus its cellbases), whatever filter is on.
  const activitiesCount = useQuery({
    ...activityPageQuery(lockHash, undefined, 'all', pageSize),
    enabled: !!lockHash,
    select: (page) => page.total,
  });
  // C14 (Address.dc.html): every filter chip carries its count — one one-row page per filter.
  const chipPages = useQueries({
    queries: activityFilters
      .filter(([value]) => value !== 'all')
      .map(([value]) => ({
        ...activityPageQuery(lockHash, undefined, value, 1),
        enabled: !!lockHash,
      })),
  });
  const transactions = useQuery({
    queryKey: ['address-transactions', lockHash, txPage.cursor, pageSize],
    queryFn: txPage.ask((cursor) =>
      api.getAddressTransactions(lockHash!, { limit: pageSize, cursor })
    ),
    enabled: !!lockHash && tab === 'transactions',
    placeholderData: keepPreviousData,
    refetchInterval: poolRefetchInterval({
      isPageOne: !txPage.cursor,
      isActiveTab: tab === 'transactions',
    }),
  });
  const cells = useQuery({
    queryKey: ['address-cells', lockHash, cellPage.cursor, cellClass, pageSize],
    queryFn: cellPage.ask((cursor) =>
      api.getAddressCells(lockHash!, {
        limit: pageSize,
        cursor,
        class: cellClass,
      })
    ),
    enabled: !!lockHash && tab === 'cells',
    placeholderData: keepPreviousData,
  });
  // The .cell names are a short list the API sends no total of, read in name order: the pager's
  // short-list form, each load added under the names already shown (`next 20 ↓`).
  const names = useInfiniteQuery({
    queryKey: ['address-dotcell-names', lockHash, pageSize],
    queryFn: ({ pageParam }) =>
      api.getAddressDotCellNames(lockHash!, { limit: pageSize, cursor: pageParam }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => (page.hasMore && page.nextCursor ? page.nextCursor : undefined),
    enabled: !!lockHash,
  });
  const nameRows = names.data?.pages.flatMap((page) => page.data);
  const fiber = useQuery({
    queryKey: ['address-fiber-channels', lockHash],
    queryFn: () => api.getAddressFiberChannels(lockHash!, { limit: 100 }),
    enabled: !!lockHash,
  });
  const pool =
    tab === 'activities' && !activityPage.cursor
      ? activities.data?.pool
      : tab === 'transactions' && !txPage.cursor
        ? transactions.data?.pool
        : undefined;
  const a = address.data;
  // the lock's name, the one decision's (lane P1 follow-up): the strip and the byte map write it
  const lockName = a?.lockScript ? addressLockName(a) : null;
  if (!a && isNotFoundApiError(address.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'address',
          identifier: addr,
          checkedEndpoint: `/addresses/${encodeURIComponent(addr)}`,
        }}
      />
    );
  }
  if (!a && isBadRequestApiError(address.error)) {
    return <NotFoundPage notAnId={{ kind: 'address', reason: address.error.apiMessage }} />;
  }
  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        {!a && address.error ? (
          <ReadFailure what="address" error={address.error} />
        ) : !a ? (
          <DetailSkeleton />
        ) : (
          <DetailLayout
            // C13: the page is named by its short address (U-09: an h1 is the head size); the full
            // one and its copy are beneath.
            title={
              a.address ? shortenAddress(a.address, 'head') : shortenHash(a.lockScriptHash, 'head')
            }
            documentTitle={[
              a.address ? titleAddress(a.address) : titleHash(a.lockScriptHash),
              'address',
            ]}
            identity={
              a.address ? (
                <AddressIdentity value={a.address} kind="address" />
              ) : (
                <AddressIdentity value={a.lockScriptHash} kind="hash" />
              )
            }
            // An address is its lock script: the page presses the script's stone (the tall rounded
            // one) of the lock script's hash, which the address read already carries; nothing else
            // is fetched for the seal. Its name plate (seals by kind, C10) opens on "Address".
            seal={{ id: { kind: 'script', hash: a.lockScriptHash }, kind: 'Address' }}
            // The status strip under the id line (lane D2, PA-13): the lock, the live cells, the
            // transactions and the holdings, said once (they stood on the kind line and again in
            // the metrics row under the balance)
            strip={{ label: 'Address status', facts: addressStripFacts(a, holdings.data) }}
            tier2={
              // Lane E (C6, Address board tier 2): the lock script as its bytes, then its two names.
              <div className="grid min-w-0 items-start gap-x-16 gap-y-10 xl:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
                <DetailSection
                  title="The lock script this address names"
                  actions={
                    <span className="text-small">
                      {/* F2 blocks (BA-38): the address JSON (its lock script) and this page as Markdown. */}
                      {/* the API's JSON, a way out of the site: its absolute URL (lane P1
                          follow-up; the site's Link would prefix a path with the network) */}
                      <ExternalLink
                        look="inline"
                        href={
                          new URL(
                            `${apiBaseFor(network)}/addresses/${encodeURIComponent(addr)}`,
                            window.location.origin
                          ).href
                        }
                      >
                        JSON
                      </ExternalLink>
                      {' · '}
                      <Link
                        look="inline"
                        href={`/address/${addr}.md`}
                        target="_blank"
                        rel="noreferrer"
                      >
                        Markdown
                      </Link>
                    </span>
                  }
                >
                  {a.lockScript ? (
                    <ScriptByteMap
                      kind="lock"
                      script={a.lockScript}
                      name={lockName}
                      href={getScriptDetailHref({
                        name: lockName,
                        codeHash: a.lockScript.codeHash,
                        hashType: a.lockScript.hashType,
                        scriptKind: 'lock',
                      })}
                    />
                  ) : (
                    <p className="text-text-dim text-small">Script unavailable</p>
                  )}
                </DetailSection>
                <ScriptNames lockHash={a.lockScriptHash} address={a.address ?? null} />
              </div>
            }
          >
            {a && <AddressStaleNotice error={address.error} label="address metadata" />}
            {holdings.data && <AddressStaleNotice error={holdings.error} label="holdings" />}
            {holdings.data ? (
              // Lane P (Address.dc.html): the balance hero (its four metrics are the head's status
              // strip since lane D2)
              <AddressBalance holdings={holdings.data} />
            ) : (
              <AddressLoadState
                label="holdings and balance partition"
                error={holdings.error}
                read={holdings}
              />
            )}
            {a.pendingSummary !== null && a.pendingSummary.txCount > 0 && (
              <aside
                aria-label="Unconfirmed"
                className="text-text-dim text-small flex flex-wrap items-baseline gap-3"
              >
                <span>Unconfirmed</span>
                <span>{a.pendingSummary.txCount.toLocaleString('en-US')} tx</span>
                {BigInt(a.pendingSummary.capacityDelta) !== BigInt(0) && (
                  <span>
                    <CkbAmount
                      form="summary"
                      shannons={BigInt(a.pendingSummary.capacityDelta)}
                      wrap
                    />{' '}
                    pending
                  </span>
                )}
              </aside>
            )}
            {holdings.data && <AddressHoldingsSection holdings={holdings.data} />}
            {names.error ? (
              <AddressLoadState label=".cell named rights" error={names.error} read={names} />
            ) : (
              nameRows &&
              nameRows.length > 0 && (
                <DetailSection title=".cell named rights">
                  <div data-testid="address-dotcell-names">
                    {/* a statement of what the list holds: the reading ink (R3; P1 follow-up) */}
                    <p className="text-text text-small max-w-measure mb-3">
                      Names this protocol assigns to the address. These rights do not add the name
                      cell’s capacity to this address’s physical holdings.
                    </p>
                    {nameRows.map((n) => (
                      <div
                        key={n.identityId}
                        className="border-row-rule text-small flex flex-wrap justify-between gap-2 border-b py-2"
                      >
                        <Link look="key" href={getIdentityItemDetailHref('dotcell', n.identityId)}>
                          {n.name}
                        </Link>
                        <span className="text-text-dim">
                          {unitNode(
                            n.expiredAt === null
                              ? 'Expiry unavailable'
                              : `expires ${formatUtc8Date(n.expiredAt * 1000)}`
                          )}
                        </span>
                      </div>
                    ))}
                    {names.hasNextPage && (
                      <div className="mt-3">
                        <Pager
                          mode="more"
                          next={{
                            count: pageSize,
                            onShow: () => void names.fetchNextPage({ cancelRefetch: false }),
                          }}
                        />
                      </div>
                    )}
                  </div>
                </DetailSection>
              )
            )}
            {fiber.error ? (
              <AddressLoadState label="Fiber channels" error={fiber.error} read={fiber} />
            ) : (
              fiber.data &&
              fiber.data.length > 0 && (
                <DetailSection
                  title="Fiber channels"
                  args={fiber.data.length.toLocaleString('en-US')}
                >
                  <p className="text-text text-small mb-3">
                    Up to 100 indexed channels associated with this address.
                  </p>
                  {fiber.data.map((c) => (
                    <div
                      key={c.channelId}
                      className="border-row-rule text-small flex flex-wrap justify-between gap-3 border-b py-3"
                    >
                      <Link look="inline" href={`/fiber/channels/${c.channelId}`}>
                        <Hash hash={c.channelId} size="row" className="text-small" />
                      </Link>
                      <span>{c.state}</span>
                      <span className="flex flex-wrap gap-2">
                        {c.participants.map((p) => (
                          <Link look="inline" href={`/address/${p}`} key={p}>
                            <Hash hash={p} size="row" className="text-small" />
                          </Link>
                        ))}
                      </span>
                      <CkbAmount
                        form="column"
                        shannons={BigInt(c.capacity)}
                        signed={false}
                        tone="plain"
                        wrap
                      />
                    </div>
                  ))}
                </DetailSection>
              )
            )}
            <section aria-labelledby={historyId} className="min-w-0">
              {/* The history opens on its command line, the tab row (the controls board's first
                  composition in place); the section keeps its name for the outline. */}
              <h2 id={historyId} className="sr-only">
                History
              </h2>
              {tab === 'activities' && activities.data && (
                <AddressStaleNotice error={activities.error} label="activities" />
              )}
              {tab === 'transactions' && transactions.data && (
                <AddressStaleNotice error={transactions.error} label="transactions" />
              )}
              {tab === 'cells' && cells.data && (
                <AddressStaleNotice error={cells.error} label="live cells" />
              )}
              <Tabs
                label="history"
                command
                tabs={[
                  { key: 'activities', label: 'activities', count: activitiesCount.data },
                  { key: 'transactions', label: 'transactions', count: a.transactionsCount },
                  { key: 'cells', label: 'live cells', count: holdings.data?.liveCellsCount },
                ]}
                value={tab}
                onChange={setTab}
              >
                {/* The filter under the row, 14 px down (the board's address in place). */}
                {tab === 'activities' && (
                  <FilterChips
                    label="kind"
                    values={activityFilters.map(([value, label], i) => {
                      const page = value === 'all' ? undefined : chipPages[i - 1].data;
                      // Exact or none: "all" is the address's total, a filter 0 or 1, else no
                      // number, its chip saying why on hover.
                      const count =
                        value === 'all' ? activitiesCount.data : page && chipCount(page);
                      return {
                        key: value,
                        label: label.toLowerCase(),
                        count: typeof count === 'number' ? count : undefined,
                        title: count === 'many' ? MANY_TITLE : undefined,
                        mark: ACTIVITY_MARKS[value],
                      };
                    })}
                    value={filter}
                    onChange={setFilter}
                    counted
                    className="mt-3.5"
                  />
                )}
                {/* The classes are offered from the first draw, so the list under them never moves
                    (lane P1: they appeared once the holdings answered and pushed it down); each
                    class's cells are counted of the holdings read, added in place when it has
                    answered. */}
                {tab === 'cells' && (
                  <FilterChips
                    label="class"
                    values={[
                      { key: 'all', label: 'all' },
                      ...ADDRESS_CLASSES.map((c) => ({
                        key: c.key,
                        label: c.label.toLowerCase(),
                        count: holdings.data?.classes[c.key].cellCount,
                        mark: CLASS_KEYS[c.mark],
                      })),
                    ]}
                    value={cellClass ?? 'all'}
                    onChange={(value) => setCellClass(value === 'all' ? undefined : value)}
                    counted
                    className="mt-3.5"
                  />
                )}
                {pool?.enabled && !pool.healthy && <PoolUnavailableNotice />}
                {pool?.enabled && pool.healthy && pool.truncated && <PoolTruncatedNotice />}
                {tab === 'activities' &&
                  (activities.data ? (
                    <>
                      {activities.data.data.length ? (
                        // B4: its runs folded, within the page shown
                        <AddressActivities
                          rows={activities.data.data}
                          address={a}
                          more={activities.data.hasMore}
                        />
                      ) : (
                        <p className="text-text-dim text-small max-w-measure py-6">
                          {filter === 'all'
                            ? 'No activities.'
                            : `No ${activityFilters.find((f) => f[0] === filter)![1]} activities on this page.`}
                        </p>
                      )}
                      <AddressPagination
                        pagination={activityPage}
                        read={activities.data}
                        label="activities"
                        order="newest first"
                      />
                    </>
                  ) : (
                    <AddressLoadState
                      label="activities"
                      error={activities.error}
                      read={activities}
                    />
                  ))}
                {tab === 'transactions' &&
                  (transactions.data ? (
                    <>
                      <AddressTransactions rows={transactions.data.data} />
                      <AddressPagination
                        pagination={txPage}
                        read={transactions.data}
                        label="transactions"
                        order="newest first"
                      />
                    </>
                  ) : (
                    <AddressLoadState
                      label="transactions"
                      error={transactions.error}
                      read={transactions}
                    />
                  ))}
                {tab === 'cells' &&
                  (cells.data ? (
                    <>
                      <AddressLiveCells rows={cells.data.data} />
                      {/* counted where the page's own response counts its cells; a class
                          filter's count is the holdings' read, stated by the class's chip
                          (lane c2-finish, 1c) */}
                      <AddressPagination
                        pagination={cellPage}
                        read={cells.data}
                        label="live cells"
                      />
                    </>
                  ) : (
                    <AddressLoadState label="live cells" error={cells.error} read={cells} />
                  ))}
              </Tabs>
            </section>
          </DetailLayout>
        )}
      </main>
    </div>
  );
}
