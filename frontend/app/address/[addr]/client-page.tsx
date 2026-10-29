'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { Hash } from '@/components/ui/hash';
import Link from '@/components/ui/link';
import { ClassMark } from '@/components/ui/class-mark';
import {
  AddressBalance,
  AddressHoldingsSection,
  AddressLoadState,
  AddressStaleNotice,
  AddressPagination,
  ADDRESS_CLASSES,
} from '@/components/address/address-holdings';
import {
  AddressActivity,
  AddressTransactions,
  AddressLiveCells,
} from '@/components/address/address-history';
import { PoolTruncatedNotice, PoolUnavailableNotice } from '@/components/ui/pool-status';
import { ExactCkb } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, isNotFoundApiError, type AddressCellClass } from '@/lib/api';
import { useParams } from '@/src/navigation';
import { POOL_POLL_INTERVAL_MS, poolRefetchInterval } from '@/lib/pool-polling';
import { getIdentityItemDetailHref, getScriptDetailHref } from '@/lib/detail-routes';
import { cn } from '@/lib/utils';

const activityFilters = [
  ['all', 'All'],
  ['ckb', 'CKB'],
  ['token', 'Token'],
  ['object', 'Object'],
  ['identity', 'Identity'],
  ['dao', 'DAO'],
  ['type_call', 'Script calls (type)'],
  ['lock_call', 'Script calls (lock)'],
  ['protocol:rgbpp', 'RGB++'],
] as const;
type ActivityFilter = (typeof activityFilters)[number][0];
export default function AddressDetailPage() {
  const params = useParams();
  const addr = params.addr as string;
  return <AddressDetailPageContent key={addr} addr={addr} />;
}
function AddressIdentity({ value }: { value: string }) {
  const [copyState, setCopyState] = useState('Copy address');
  return (
    <span className="flex min-w-0 items-start gap-2 text-[13px]">
      <Hash
        hash={value}
        truncate={false}
        copyable={false}
        className="min-w-0 text-[13px] [overflow-wrap:anywhere]"
      />
      <button
        aria-label={copyState}
        title={copyState}
        className="hover:text-jade shrink-0 p-1"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setCopyState('Address copied');
          } catch {
            setCopyState('Unable to copy address');
          }
        }}
      >
        {copyState === 'Address copied' ? (
          '✓'
        ) : (
          <svg
            aria-hidden="true"
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
          >
            <rect x="5" y="5" width="8" height="8" rx="1" />
            <path d="M3 11H2V2h9v1" />
          </svg>
        )}
      </button>
    </span>
  );
}
function AddressDetailPageContent({ addr }: { addr: string }) {
  const [tab, setTab] = useState<'activities' | 'transactions' | 'cells'>('activities');
  const [filter, setFilter] = useState<ActivityFilter>('all');
  const [cellClass, setCellClass] = useState<AddressCellClass | undefined>();
  const activityPage = useCursorPagination(),
    txPage = useCursorPagination(),
    cellPage = useCursorPagination(),
    namesPage = useCursorPagination();
  const address = useQuery({
    queryKey: ['address', addr],
    queryFn: () => api.getAddress(addr),
    refetchInterval: POOL_POLL_INTERVAL_MS,
  });
  const lockHash = address.data?.lockScriptHash;
  const holdings = useQuery({
    queryKey: ['address-holdings', lockHash],
    queryFn: () => api.getAddressHoldings(lockHash!),
    enabled: !!lockHash,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const activities = useQuery({
    queryKey: ['address-activities', lockHash, activityPage.cursor, filter],
    queryFn: () =>
      api.getAddressActivities(lockHash!, { limit: 50, cursor: activityPage.cursor, filter }),
    enabled: !!lockHash && tab === 'activities',
    placeholderData: keepPreviousData,
    refetchInterval: poolRefetchInterval({
      isPageOne: !activityPage.cursor,
      isActiveTab: tab === 'activities',
    }),
  });
  const transactions = useQuery({
    queryKey: ['address-transactions', lockHash, txPage.cursor],
    queryFn: () => api.getAddressTransactions(lockHash!, { limit: 50, cursor: txPage.cursor }),
    enabled: !!lockHash && tab === 'transactions',
    placeholderData: keepPreviousData,
    refetchInterval: poolRefetchInterval({
      isPageOne: !txPage.cursor,
      isActiveTab: tab === 'transactions',
    }),
  });
  const cells = useQuery({
    queryKey: ['address-cells', lockHash, cellPage.cursor, cellClass],
    queryFn: () =>
      api.getAddressCells(lockHash!, { limit: 50, cursor: cellPage.cursor, class: cellClass }),
    enabled: !!lockHash && tab === 'cells',
    placeholderData: keepPreviousData,
  });
  const names = useQuery({
    queryKey: ['address-dotcell-names', lockHash, namesPage.cursor],
    queryFn: () => api.getAddressDotCellNames(lockHash!, { limit: 50, cursor: namesPage.cursor }),
    enabled: !!lockHash,
    placeholderData: keepPreviousData,
  });
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
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        {!a ? (
          <AddressLoadState label="address" error={address.error} loading={address.isLoading} />
        ) : (
          <DetailLayout
            title="Address"
            identity={<AddressIdentity value={a.address ?? a.lockScriptHash} />}
            status={
              <span className="text-text-dim flex flex-wrap items-center gap-x-4 gap-y-1 text-xs">
                {a.lockScript && (
                  <span>
                    lock{' '}
                    <Link
                      href={getScriptDetailHref({
                        name: a.lockScriptInfo?.name,
                        codeHash: a.lockScript.codeHash,
                        hashType: a.lockScript.hashType,
                        scriptKind: 'lock',
                      })}
                      className="text-text-bright"
                    >
                      {a.lockScriptInfo?.name ?? (
                        <Hash hash={a.lockScript.codeHash} className="text-xs" copyable={false} />
                      )}
                    </Link>
                  </span>
                )}
                {a.lockScriptInfo?.deprecated && <span className="text-negative">Deprecated</span>}
                <span>
                  {holdings.data
                    ? `${holdings.data.liveCellsCount.toLocaleString('en-US')} live cells · `
                    : ''}
                  {a.transactionsCount.toLocaleString('en-US')} transactions
                </span>
              </span>
            }
            tier2={
              <DetailSection title="Lock script">
                <dl className="grid grid-cols-[90px_minmax(0,1fr)] gap-x-4 gap-y-3 text-xs">
                  {a.lockScript ? (
                    <>
                      <dt className="text-text-dim">code hash</dt>
                      <dd className="break-all">
                        <Link
                          href={getScriptDetailHref({
                            name: a.lockScriptInfo?.name,
                            codeHash: a.lockScript.codeHash,
                            hashType: a.lockScript.hashType,
                            scriptKind: 'lock',
                          })}
                        >
                          <Hash
                            hash={a.lockScript.codeHash}
                            truncate={false}
                            copyable={false}
                            className="text-xs"
                          />
                        </Link>
                      </dd>
                      <dt className="text-text-dim">hash type</dt>
                      <dd>{a.lockScript.hashType}</dd>
                      <dt className="text-text-dim">args</dt>
                      <dd className="break-all">
                        <Hash hash={a.lockScript.args} truncate={false} className="text-xs" />
                      </dd>
                    </>
                  ) : (
                    <>
                      <dt>Script</dt>
                      <dd>Unavailable</dd>
                    </>
                  )}
                  <dt className="text-text-dim">lock hash</dt>
                  <dd className="break-all">
                    <Hash hash={a.lockScriptHash} truncate={false} className="text-xs" />
                  </dd>
                </dl>
              </DetailSection>
            }
          >
            {a && <AddressStaleNotice error={address.error} label="address metadata" />}
            {holdings.data && <AddressStaleNotice error={holdings.error} label="holdings" />}
            {holdings.data ? (
              <AddressBalance holdings={holdings.data} />
            ) : (
              <AddressLoadState
                label="holdings and balance partition"
                error={holdings.error}
                loading={holdings.isLoading}
              />
            )}
            {a.pendingSummary !== null && a.pendingSummary.txCount > 0 && (
              <aside
                aria-label="Unconfirmed"
                className="text-text-dim flex flex-wrap items-baseline gap-3 text-xs"
              >
                <span>Unconfirmed</span>
                <span>{a.pendingSummary.txCount} tx</span>
                {BigInt(a.pendingSummary.capacityDelta) !== BigInt(0) && (
                  <span>
                    <ExactCkb shannons={BigInt(a.pendingSummary.capacityDelta)} wrap /> pending
                  </span>
                )}
              </aside>
            )}
            {holdings.data && <AddressHoldingsSection holdings={holdings.data} />}
            {names.error ? (
              <AddressLoadState label=".cell named rights" error={names.error} loading={false} />
            ) : (
              names.data &&
              names.data.data.length > 0 && (
                <DetailSection title=".cell named rights">
                  <div data-testid="address-dotcell-names">
                    <p className="text-text-dim mb-3 text-xs">
                      Names this protocol assigns to the address. These rights do not add the name
                      cell’s capacity to this address’s physical holdings.
                    </p>
                    {names.data.data.map((n) => (
                      <div
                        key={n.identityId}
                        className="border-base-elevated flex flex-wrap justify-between gap-2 border-b py-2 text-xs"
                      >
                        <Link
                          href={getIdentityItemDetailHref('dotcell', n.identityId)}
                          className="text-text-bright"
                        >
                          {n.name}
                        </Link>
                        <span className="text-text-dim">
                          {n.expiredAt === null
                            ? 'Expiry unavailable'
                            : `expires ${new Date(n.expiredAt * 1000).toISOString().slice(0, 10)}`}
                        </span>
                      </div>
                    ))}
                    <AddressPagination
                      pagination={namesPage}
                      hasMore={names.data.hasMore}
                      nextCursor={names.data.nextCursor}
                      currentCount={names.data.data.length}
                      total={names.data.total}
                      label="names"
                    />
                  </div>
                </DetailSection>
              )
            )}
            {fiber.error ? (
              <AddressLoadState label="Fiber channels" error={fiber.error} loading={false} />
            ) : (
              fiber.data &&
              fiber.data.length > 0 && (
                <DetailSection title="Fiber channels">
                  <p className="text-text-dim mb-3 text-xs">
                    Up to 100 indexed channels associated with this address.
                  </p>
                  {fiber.data.map((c) => (
                    <div
                      key={c.channelId}
                      className="border-base-elevated flex flex-wrap justify-between gap-3 border-b py-3 text-xs"
                    >
                      <Link href={`/fiber/channels/${c.channelId}`}>
                        <Hash hash={c.channelId} className="text-xs" copyable={false} />
                      </Link>
                      <span>{c.state}</span>
                      <span className="flex flex-wrap gap-2">
                        {c.participants.map((p) => (
                          <Link href={`/address/${p}`} key={p}>
                            <Hash hash={p} copyable={false} className="text-xs" />
                          </Link>
                        ))}
                      </span>
                      <ExactCkb shannons={BigInt(c.capacity)} signed={false} tone="plain" wrap />
                    </div>
                  ))}
                </DetailSection>
              )
            )}
            <section aria-label="History" className="min-w-0">
              {tab === 'activities' && activities.data && (
                <AddressStaleNotice error={activities.error} label="activities" />
              )}
              {tab === 'transactions' && transactions.data && (
                <AddressStaleNotice error={transactions.error} label="transactions" />
              )}
              {tab === 'cells' && cells.data && (
                <AddressStaleNotice error={cells.error} label="live cells" />
              )}
              <div
                role="group"
                aria-label="History views"
                className="border-base-border-subtle flex flex-wrap gap-x-6 gap-y-1 border-b"
              >
                {(
                  [
                    ['activities', 'Activities'],
                    ['transactions', 'Transactions'],
                    ['cells', 'Live cells'],
                  ] as const
                ).map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    aria-pressed={tab === id}
                    onClick={() => setTab(id)}
                    className={cn(
                      'flex h-11 items-center gap-2 border-b-2 text-[15px] font-medium',
                      tab === id
                        ? 'border-jade text-text-bright'
                        : 'text-text-dim border-transparent'
                    )}
                  >
                    {label}
                    {id === 'transactions' && (
                      <span className="text-text-dim text-xs font-normal">
                        {a.transactionsCount.toLocaleString('en-US')}
                      </span>
                    )}
                    {id === 'cells' && holdings.data && (
                      <span className="text-text-dim text-xs font-normal">
                        {holdings.data.liveCellsCount.toLocaleString('en-US')}
                      </span>
                    )}
                  </button>
                ))}
              </div>
              {tab === 'activities' && (
                <div
                  role="group"
                  aria-label="Filter activity"
                  className="my-4 flex flex-wrap gap-2"
                >
                  {activityFilters.map(([value, label]) => (
                    <button
                      key={value}
                      aria-pressed={filter === value}
                      onClick={() => {
                        setFilter(value);
                        activityPage.reset();
                      }}
                      className={cn(
                        'rounded border px-2.5 py-1.5 text-xs',
                        filter === value
                          ? 'border-base-border bg-base-elevated text-text-bright'
                          : 'border-base-border-subtle text-text-dim'
                      )}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              )}
              {tab === 'cells' && (
                <div
                  role="group"
                  aria-label="Filter live cells"
                  className="my-4 flex flex-wrap gap-2"
                >
                  {[{ key: undefined, label: 'All', mark: undefined }, ...ADDRESS_CLASSES].map(
                    (c) => (
                      <button
                        key={c.key ?? 'all'}
                        aria-pressed={cellClass === c.key}
                        onClick={() => {
                          setCellClass(c.key);
                          cellPage.reset();
                        }}
                        className={cn(
                          'flex items-center gap-1.5 rounded border px-2.5 py-1.5 text-xs',
                          cellClass === c.key
                            ? 'border-base-border bg-base-elevated text-text-bright'
                            : 'border-base-border-subtle text-text-dim'
                        )}
                      >
                        {c.mark && <ClassMark kind={c.mark} size={12} />}
                        {c.label}
                        {holdings.data && c.key && (
                          <span>{holdings.data.classes[c.key].cellCount}</span>
                        )}
                      </button>
                    )
                  )}
                </div>
              )}
              {pool?.enabled && !pool.healthy && <PoolUnavailableNotice />}
              {pool?.enabled && pool.healthy && pool.truncated && <PoolTruncatedNotice />}
              {tab === 'activities' &&
                (activities.data ? (
                  <>
                    {activities.data.data.length ? (
                      activities.data.data.map((row) => (
                        <AddressActivity key={row.txHash} activity={row} address={a} />
                      ))
                    ) : (
                      <p className="text-text-dim py-6 text-xs">
                        {filter === 'all'
                          ? 'No activities.'
                          : `No ${activityFilters.find((f) => f[0] === filter)![1]} activities on this page.`}
                      </p>
                    )}
                    <AddressPagination
                      pagination={activityPage}
                      hasMore={activities.data.hasMore}
                      nextCursor={activities.data.nextCursor}
                      currentCount={
                        activities.data.data.filter((row) => row.poolStatus === undefined).length
                      }
                      label="activities"
                    />
                  </>
                ) : (
                  <AddressLoadState
                    label="activities"
                    error={activities.error}
                    loading={activities.isLoading}
                  />
                ))}
              {tab === 'transactions' &&
                (transactions.data ? (
                  <>
                    <AddressTransactions rows={transactions.data.data} />
                    <AddressPagination
                      pagination={txPage}
                      hasMore={transactions.data.hasMore}
                      nextCursor={transactions.data.nextCursor}
                      currentCount={
                        transactions.data.data.filter((row) => row.poolStatus === undefined).length
                      }
                      total={transactions.data.total}
                      label="transactions"
                    />
                  </>
                ) : (
                  <AddressLoadState
                    label="transactions"
                    error={transactions.error}
                    loading={transactions.isLoading}
                  />
                ))}
              {tab === 'cells' &&
                (cells.data ? (
                  <>
                    <AddressLiveCells rows={cells.data.data} />
                    <AddressPagination
                      pagination={cellPage}
                      hasMore={cells.data.hasMore}
                      nextCursor={cells.data.nextCursor}
                      currentCount={cells.data.data.length}
                      total={
                        cellClass && holdings.data
                          ? holdings.data.classes[cellClass].cellCount
                          : cells.data.total
                      }
                      label="live cells"
                    />
                  </>
                ) : (
                  <AddressLoadState
                    label="live cells"
                    error={cells.error}
                    loading={cells.isLoading}
                  />
                ))}
            </section>
          </DetailLayout>
        )}
      </main>
    </div>
  );
}
