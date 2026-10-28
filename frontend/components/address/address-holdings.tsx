'use client';

import { useState, type ReactNode } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import {
  api,
  type AddressHoldings,
  type AddressCellClass,
  type AddressObjectComposition,
} from '@/lib/api';
import { ClassMark } from '@/components/ui/class-mark';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import Link from '@/components/ui/link';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { splitTokenAmount } from '@/lib/format-asset';
import { formatCompositionTier } from '@/lib/asset-utils';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { cn, formatCkbAmount } from '@/lib/utils';

export const ADDRESS_CLASSES: { key: AddressCellClass; label: string; mark: AssetClass }[] = [
  { key: 'dao', label: 'Nervos DAO', mark: 'dao' },
  { key: 'token', label: 'Tokens', mark: 'tokens' },
  { key: 'object', label: 'Objects', mark: 'objects' },
  { key: 'identity', label: 'Identities', mark: 'identities' },
  { key: 'script', label: 'Other scripts', mark: 'other' },
  { key: 'plain', label: 'Plain CKB', mark: 'ckb' },
];
export function AddressLoadState({
  error,
  loading,
  label,
}: {
  error: Error | null;
  loading: boolean;
  label: string;
}) {
  return (
    <p className="text-text-dim py-3 text-xs" role={error ? 'alert' : 'status'}>
      {error
        ? `Could not load ${label}: ${error.message}`
        : loading
          ? `Loading ${label}…`
          : `${label} unavailable.`}
    </p>
  );
}
export function AddressStaleNotice({ error, label }: { error: Error | null; label: string }) {
  return error ? (
    <p role="alert" className="text-warning py-2 text-xs">
      Could not refresh {label}: {error.message}. Showing previously loaded data.
    </p>
  ) : null;
}
export function AddressPagination({
  pagination,
  hasMore,
  nextCursor,
  currentCount,
  total,
  label,
}: {
  pagination: ReturnType<typeof useCursorPagination>;
  hasMore: boolean;
  nextCursor?: string | null;
  currentCount: number;
  total?: number;
  label: string;
}) {
  const start = currentCount === 0 ? 0 : (pagination.page - 1) * 50 + 1;
  return (
    <nav
      aria-label={`${label} pages`}
      className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs"
    >
      <span className="text-text-dim">
        Showing {start.toLocaleString('en-US')}-
        {(currentCount ? start + currentCount - 1 : 0).toLocaleString('en-US')}
        {total === undefined ? '' : ` of ${total.toLocaleString('en-US')}`} {label}
      </span>
      {(hasMore || pagination.hasPrevious) && (
        <span className="flex flex-wrap items-center gap-3">
          <button
            className="border-base-border hover:text-jade rounded border px-3 py-2 disabled:opacity-40"
            disabled={!pagination.hasPrevious}
            onClick={pagination.goToPrevious}
          >
            Previous
          </button>
          <span className="text-text-dim">Page {pagination.page}</span>
          <button
            className="border-base-border hover:text-jade rounded border px-3 py-2 disabled:opacity-40"
            disabled={!hasMore}
            onClick={() => pagination.goToNext(nextCursor)}
          >
            Next
          </button>
        </span>
      )}
    </nav>
  );
}

const composition = [
  ['pureCkb', 'pure_ckb'],
  ['btcCkb', 'btc_ckb'],
  ['decentralizedMixture', 'decentralized_mixture'],
  ['centralizedMixture', 'centralized_mixture'],
  ['unknown', 'unknown'],
  ['decodePending', 'decode_pending'],
] as const;
export function ObjectCompositionLabel({ tier }: { tier: string | null }) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px]',
        tier === 'btc_ckb'
          ? 'border-[#dc8a5a]/70 bg-[#1a110d] text-[#ffe1cc]'
          : tier === 'pure_ckb'
            ? 'border-jade/40 text-jade'
            : 'border-base-border text-text-dim'
      )}
      title={
        tier === 'decode_pending'
          ? 'Awaiting DOB execution; composition has not been classified.'
          : undefined
      }
    >
      {(tier === 'btc_ckb' || tier === 'pure_ckb') && <ClassMark kind="objects" size={12} />}
      {tier === null ? 'Composition unavailable' : formatCompositionTier(tier)}
    </span>
  );
}
function Composition({ counts, total }: { counts: AddressObjectComposition; total: number }) {
  if (composition.reduce((sum, [key]) => sum + counts[key], 0) !== total)
    throw new Error('Address object composition counts do not equal object holdings');
  return (
    <div className="space-y-2 py-3">
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
        <span>
          <strong className="text-text-bright font-medium">
            {counts.pureCkb + counts.btcCkb} of {total}
          </strong>{' '}
          fully on-chain
        </span>
        <span className="flex flex-wrap gap-x-3 gap-y-2">
          {composition
            .filter(([key]) => counts[key] > 0)
            .map(([key, tier]) => (
              <span key={key} className="flex items-center gap-1">
                <ObjectCompositionLabel tier={tier} />
                {counts[key]}
              </span>
            ))}
        </span>
      </div>
      {total > 0 && (
        <div
          role="img"
          aria-label={`Composition of ${total} objects`}
          className="flex h-2 gap-0.5 overflow-hidden rounded"
        >
          {composition
            .filter(([key]) => counts[key] > 0)
            .map(([key]) => (
              <span
                key={key}
                title={`${key}: ${counts[key]}`}
                style={{
                  width: `${(counts[key] / total) * 100}%`,
                  background:
                    key === 'pureCkb'
                      ? 'linear-gradient(90deg,#3ee0b0,#f0fffa,#8ff3d6)'
                      : key === 'btcCkb'
                        ? 'linear-gradient(90deg,#a95a32,#ffe1cc,#dc8a5a,#8ff3d6)'
                        : '#2c3348',
                }}
              />
            ))}
        </div>
      )}
    </div>
  );
}
function TokenAmount({ amount, decimals }: { amount: string; decimals: number | null }) {
  const { integer, fraction } = splitTokenAmount(amount, decimals);
  const significant = fraction.replace(/0+$/, '');
  return (
    <>
      {integer}
      {fraction && (
        <>
          <span className="opacity-[0.78]">{significant ? '.' + significant : ''}</span>
          <span data-ghost className="text-text-ghost">
            {significant ? '' : '.'}
            {fraction.slice(significant.length)}
          </span>
        </>
      )}
      {decimals === null ? ' (raw)' : ''}
    </>
  );
}
export function AddressBalance({ holdings }: { holdings: AddressHoldings }) {
  return (
    <section aria-label="Balance" className="space-y-4">
      <div className="text-text-bright text-[18px] font-medium">
        <ExactCkb shannons={BigInt(holdings.balance)} signed={false} tone="plain" wrap />
      </div>
      <CapacityPartition
        subject="Address"
        showTotal={false}
        total={holdings.balance}
        occupied={holdings.occupiedCapacity}
        free={holdings.freeCapacity}
        daoFree={holdings.daoFreeCapacity}
      />
    </section>
  );
}
function HoldingSection({
  kind,
  summary,
  children,
  active,
  onActive,
}: {
  kind: AddressCellClass;
  summary: ReactNode;
  children: ReactNode;
  active: AddressCellClass | null;
  onActive: (v: AddressCellClass | null) => void;
}) {
  const cls = ADDRESS_CLASSES.find((c) => c.key === kind)!;
  return (
    <section
      id={`holdings-${kind}`}
      data-holding-class={kind}
      data-highlighted={active === kind || undefined}
      className={cn('min-w-0 transition-opacity', active && active !== kind && 'opacity-40')}
      onMouseEnter={() => onActive(kind)}
      onMouseLeave={() => onActive(null)}
      onFocus={() => onActive(kind)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) onActive(null);
      }}
    >
      <div className="border-base-border-subtle flex flex-wrap items-baseline justify-between gap-2 border-b pb-2">
        <h3 className="text-text-bright flex items-center gap-2 text-[15px] font-medium">
          <ClassMark kind={cls.mark} size={18} />
          {kind === 'dao' ? 'Nervos DAO' : cls.label}
        </h3>
        <span className="text-text-dim max-w-full text-xs">{summary}</span>
      </div>
      {children}
    </section>
  );
}

export function AddressHoldingsSection({ holdings }: { holdings: AddressHoldings }) {
  const addr = holdings.lockScriptHash;
  const [active, setActive] = useState<AddressCellClass | null>(null);
  const tokensPage = useCursorPagination(),
    daoPage = useCursorPagination(),
    objectsPage = useCursorPagination(),
    identitiesPage = useCursorPagination();
  const tokens = useQuery({
    queryKey: ['address-tokens', addr, tokensPage.cursor],
    queryFn: () => api.getAddressTokens(addr, { limit: 50, cursor: tokensPage.cursor }),
    enabled: holdings.holdings.token > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const deposits = useQuery({
    queryKey: ['address-holdings-cells', addr, 'dao', daoPage.cursor],
    queryFn: () => api.getAddressCells(addr, { class: 'dao', limit: 50, cursor: daoPage.cursor }),
    enabled: holdings.holdings.dao > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const objects = useQuery({
    queryKey: ['address-holdings-cells', addr, 'object', objectsPage.cursor],
    queryFn: () =>
      api.getAddressCells(addr, { class: 'object', limit: 50, cursor: objectsPage.cursor }),
    enabled: holdings.holdings.object > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const identities = useQuery({
    queryKey: ['address-holdings-cells', addr, 'identity', identitiesPage.cursor],
    queryFn: () =>
      api.getAddressCells(addr, { class: 'identity', limit: 50, cursor: identitiesPage.cursor }),
    enabled: holdings.holdings.identity > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const total = Object.values(holdings.holdings).reduce((a, b) => a + b, 0);
  const occupied = exactUnsignedInteger(holdings.occupiedCapacity, 'Address occupied');
  if (
    ADDRESS_CLASSES.reduce(
      (sum, c) => sum + BigInt(holdings.classes[c.key].occupiedCapacity),
      BigInt(0)
    ) !== occupied
  )
    throw new Error('Address class occupied capacities do not equal total');
  const summary = (kind: AddressCellClass) => (
    <>
      {holdings.classes[kind].cellCount.toLocaleString('en-US')} cells ·{' '}
      <ExactCkb
        shannons={BigInt(holdings.classes[kind].capacity)}
        signed={false}
        tone="plain"
        wrap
      />{' '}
      held
    </>
  );
  return (
    <section aria-labelledby="holdings-heading" className="space-y-6">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h2 id="holdings-heading" className="text-text-bright text-[15px] font-medium">
          Holdings
        </h2>
        <nav aria-label="Holdings" className="flex flex-wrap gap-x-5 gap-y-2 text-xs">
          {ADDRESS_CLASSES.slice(0, 4).map((c) => (
            <a
              key={c.key}
              href={`#holdings-${c.key}`}
              className="hover:text-jade flex items-center gap-1.5"
            >
              <ClassMark kind={c.mark} />
              {c.label}{' '}
              {holdings.holdings[c.key as keyof typeof holdings.holdings].toLocaleString('en-US')}
            </a>
          ))}
        </nav>
      </div>
      <div className="grid gap-8 lg:grid-cols-2 lg:gap-12">
        <div className="space-y-3">
          <p className="text-text-dim text-xs">
            {total.toLocaleString('en-US')} holdings, one mark each
            {total > 128 ? ' (first 128 per class shown)' : ''}
          </p>
          <div className="flex flex-wrap gap-x-5 gap-y-3">
            {ADDRESS_CLASSES.slice(0, 4).map((c) => (
              <a
                key={c.key}
                href={`#holdings-${c.key}`}
                aria-label={`${holdings.holdings[c.key as keyof typeof holdings.holdings]} ${c.label}`}
                className="flex max-w-full flex-wrap gap-1"
                onMouseEnter={() => setActive(c.key)}
                onMouseLeave={() => setActive(null)}
                onFocus={() => setActive(c.key)}
                onBlur={() => setActive(null)}
              >
                {Array.from(
                  {
                    length: Math.min(
                      holdings.holdings[c.key as keyof typeof holdings.holdings],
                      128
                    ),
                  },
                  (_, i) => (
                    <ClassMark key={i} kind={c.mark} size={16} />
                  )
                )}
                {holdings.holdings[c.key as keyof typeof holdings.holdings] > 128 && (
                  <span>+{holdings.holdings[c.key as keyof typeof holdings.holdings] - 128}</span>
                )}
              </a>
            ))}
          </div>
        </div>
        <div className="space-y-2">
          <div className="flex flex-wrap justify-between gap-2 text-xs">
            <span className="text-text-dim">On-chain space they take</span>
            <span>
              {formatCkbAmount(occupied).full.replace(/\.0{8}$/, '')} CKB occupied in{' '}
              {holdings.liveCellsCount} cells
            </span>
          </div>
          <div
            role="img"
            aria-label="Occupied capacity by class"
            className="flex h-2.5 gap-0.5 overflow-hidden rounded"
          >
            {ADDRESS_CLASSES.map((c) => (
              <span
                key={c.key}
                data-space-class={c.key}
                data-highlighted={active === c.key || undefined}
                style={{
                  width:
                    occupied === BigInt(0)
                      ? '0%'
                      : `${(Number(BigInt(holdings.classes[c.key].occupiedCapacity)) / Number(occupied)) * 100}%`,
                  background: CLASS_COLORS[c.mark],
                  opacity: active && active !== c.key ? 0.25 : 1,
                }}
              />
            ))}
          </div>
          <div className="grid grid-cols-1 gap-x-4 gap-y-1 text-xs sm:grid-cols-2">
            {ADDRESS_CLASSES.map((c) => (
              <button
                key={c.key}
                className="flex flex-wrap items-center gap-x-1.5 text-left"
                onMouseEnter={() => setActive(c.key)}
                onMouseLeave={() => setActive(null)}
                onFocus={() => setActive(c.key)}
                onBlur={() => setActive(null)}
              >
                <ClassMark kind={c.mark} size={12} />
                {c.label}
                <span className="text-text-bright">
                  {formatCkbAmount(holdings.classes[c.key].occupiedCapacity).full.replace(
                    /\.0{8}$/,
                    ''
                  )}
                </span>
                {occupied > BigInt(0) && (
                  <span className="text-text-dim">
                    {formatScaledInteger(
                      BigInt(holdings.classes[c.key].occupiedCapacity) * BigInt(100),
                      occupied,
                      2
                    )}
                    %
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="grid items-start gap-x-12 gap-y-9 lg:grid-cols-2">
        <HoldingSection kind="dao" summary={summary('dao')} active={active} onActive={setActive}>
          {holdings.holdings.dao === 0 ? (
            <p className="text-text-dim py-4 text-xs">No live DAO holdings.</p>
          ) : (
            <>
              <p className="text-text-dim pt-3 text-xs">
                {holdings.depositCellsCount} deposits · {holdings.withdrawalRequestCellsCount}{' '}
                withdrawal requests
              </p>
              <dl className="grid gap-3 py-3 text-xs sm:grid-cols-3">
                <div>
                  <dt className="text-text-dim">CKB held</dt>
                  <dd className="mt-1">
                    <ExactCkb
                      shannons={BigInt(holdings.classes.dao.capacity)}
                      signed={false}
                      tone="plain"
                      wrap
                    />
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Unclaimed compensation</dt>
                  <dd className="mt-1">
                    <ExactCkb
                      shannons={BigInt(holdings.daoUnclaimedCompensation)}
                      signed={false}
                      tone="plain"
                      wrap
                    />
                  </dd>
                </div>
                <div>
                  <dt className="text-text-dim">Estimated APC</dt>
                  <dd className="mt-1">
                    {holdings.estimatedApc === null ? 'Unavailable' : `${holdings.estimatedApc}%`}
                  </dd>
                </div>
              </dl>
              {deposits.data && <AddressStaleNotice error={deposits.error} label="DAO holdings" />}
              {deposits.data ? (
                <>
                  {deposits.data.data.map((cell) => {
                    const d = cell.dao;
                    if (!d)
                      throw new Error(
                        `DAO cell ${cell.txHash}:${cell.outputIndex} has no deposit evidence`
                      );
                    if (d.status !== 'deposited' && d.status !== 'withdrawing')
                      throw new Error(
                        `Live DAO cell ${cell.txHash}:${cell.outputIndex} has invalid phase ${d.status}`
                      );
                    const eventBlock =
                      d.status === 'withdrawing' ? d.withdrawRequestBlock : d.depositBlockNumber;
                    const eventTimestamp =
                      d.status === 'withdrawing' ? d.withdrawRequestTimestamp : d.depositTimestamp;
                    if (eventBlock === null || eventTimestamp === null)
                      throw new Error(
                        `DAO cell ${cell.txHash}:${cell.outputIndex} is missing ${d.status} evidence`
                      );
                    return (
                      <div
                        key={`${cell.txHash}:${cell.outputIndex}`}
                        className="border-base-elevated flex flex-wrap items-baseline justify-between gap-2 border-t py-2 text-xs"
                      >
                        <span className="text-text-dim">
                          <Link href={`/cell/${cell.txHash}-${cell.outputIndex}`}>
                            {d.status === 'deposited'
                              ? 'deposited'
                              : d.status === 'withdrawing'
                                ? 'withdraw requested'
                                : 'withdrawn'}
                          </Link>{' '}
                          at{' '}
                          <Link href={`/blocks/${eventBlock}`}>
                            #{eventBlock.toLocaleString('en-US')}
                          </Link>{' '}
                          · {new Date(eventTimestamp).toISOString().slice(0, 10)}
                        </span>
                        <ExactCkb
                          shannons={BigInt(cell.capacity)}
                          signed={false}
                          tone="plain"
                          wrap
                        />
                      </div>
                    );
                  })}
                  <AddressPagination
                    pagination={daoPage}
                    hasMore={deposits.data.hasMore}
                    nextCursor={deposits.data.nextCursor}
                    currentCount={deposits.data.data.length}
                    total={deposits.data.total}
                    label="DAO cells"
                  />
                </>
              ) : (
                <AddressLoadState
                  label="DAO deposits"
                  error={deposits.error}
                  loading={deposits.isLoading}
                />
              )}
            </>
          )}
        </HoldingSection>
        <HoldingSection
          kind="token"
          summary={
            <>
              {holdings.holdings.token} kinds · {summary('token')}
            </>
          }
          active={active}
          onActive={setActive}
        >
          {tokens.data && <AddressStaleNotice error={tokens.error} label="tokens" />}
          {holdings.holdings.token === 0 ? (
            <p className="text-text-dim py-4 text-xs">No tokens held.</p>
          ) : tokens.data ? (
            <>
              {tokens.data.data.map((t) => (
                <div
                  key={t.typeScriptHash}
                  className="border-base-elevated flex flex-wrap items-baseline justify-between gap-2 border-b py-2 text-xs"
                >
                  <span className="min-w-0">
                    <Link
                      href={`/tokens/${t.typeScriptHash}`}
                      className="text-text-bright hover:text-jade"
                    >
                      {t.name ?? t.symbol ?? (
                        <Hash hash={t.typeScriptHash} copyable={false} className="text-xs" />
                      )}
                    </Link>{' '}
                    <span className="text-text-dim">
                      {t.standard === 'xudt' ? 'xUDT' : t.standard === 'sudt' ? 'sUDT' : t.standard}
                    </span>
                    {t.name && (
                      <span className="ml-2 text-[11px]">
                        <Hash
                          hash={t.typeScriptHash}
                          startChars={4}
                          endChars={2}
                          copyable={false}
                          className="text-[11px]"
                        />
                      </span>
                    )}
                  </span>
                  <span className="text-text-bright max-w-full text-right tabular-nums [overflow-wrap:anywhere]">
                    <TokenAmount amount={t.balance} decimals={t.decimals} />{' '}
                    <span className="text-text-dim">{t.symbol}</span>
                  </span>
                </div>
              ))}
              <AddressPagination
                pagination={tokensPage}
                hasMore={tokens.data.hasMore}
                nextCursor={tokens.data.nextCursor}
                currentCount={tokens.data.data.length}
                total={holdings.holdings.token}
                label="token kinds"
              />
            </>
          ) : (
            <AddressLoadState label="tokens" error={tokens.error} loading={tokens.isLoading} />
          )}
        </HoldingSection>
        <HoldingSection
          kind="object"
          summary={
            <>
              {holdings.holdings.object} in {holdings.objectCollections.length} collections ·{' '}
              {summary('object')}
            </>
          }
          active={active}
          onActive={setActive}
        >
          <Composition counts={holdings.objectComposition} total={holdings.holdings.object} />
          {objects.data && <AddressStaleNotice error={objects.error} label="objects" />}
          {holdings.holdings.object === 0 ? (
            <p className="text-text-dim py-4 text-xs">No objects held.</p>
          ) : objects.data ? (
            <>
              {holdings.objectCollections.map((g) => {
                const items = objects.data.data.filter(
                  (c) => c.display?.collectionId === g.id && c.display.standard === g.standard
                );
                const href =
                  g.id === null
                    ? null
                    : g.standard === 'm-nft'
                      ? `/classes/${g.id}`
                      : `/clusters/${g.id}`;
                return (
                  <div
                    key={`${g.standard}:${g.id}`}
                    className="border-base-elevated space-y-2 border-b py-3"
                  >
                    <div className="flex flex-wrap justify-between gap-2 text-xs">
                      <span>
                        {href ? (
                          <Link href={href} className="text-text-bright">
                            {g.name ?? <Hash hash={g.id!} copyable={false} className="text-xs" />}
                          </Link>
                        ) : (
                          <span>Uncollected {formatStandard(g.standard)}</span>
                        )}{' '}
                        <span className="text-text-dim">{g.itemCount}</span>
                      </span>
                      <span className="flex flex-wrap gap-2">
                        {composition
                          .filter(([key]) => g.compositionCounts[key] > 0)
                          .map(([key, tier]) => (
                            <ObjectCompositionLabel key={key} tier={tier} />
                          ))}
                        <span className="text-text-dim">{formatStandard(g.standard)}</span>
                      </span>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {items.map((c) => {
                        const d = c.display!;
                        return (
                          <Link
                            key={`${c.txHash}:${c.outputIndex}`}
                            href={
                              d.standard === 'm-nft' ? `/objects/mnft/${d.id}` : `/objects/${d.id}`
                            }
                            title={`${d.name ?? d.id} · ${d.compositionTier === null ? 'Composition unavailable' : formatCompositionTier(d.compositionTier)}`}
                            aria-label={d.name ?? `Object ${d.id}`}
                            className={cn(
                              'bg-base-surface flex h-[72px] w-[72px] flex-col items-center justify-between rounded border p-1.5 text-[11px]',
                              d.compositionTier === 'btc_ckb'
                                ? 'border-[#dc8a5a]/70'
                                : d.compositionTier === 'pure_ckb'
                                  ? 'border-jade/60'
                                  : 'border-base-border'
                            )}
                          >
                            <ClassMark kind="objects" size={20} />
                            <Hash
                              hash={d.id}
                              startChars={4}
                              endChars={2}
                              copyable={false}
                              className="text-[11px]"
                            />
                          </Link>
                        );
                      })}
                    </div>
                    {!items.length && (
                      <p className="text-text-dim text-[11px]">Items are on another page.</p>
                    )}
                  </div>
                );
              })}
              <AddressPagination
                pagination={objectsPage}
                hasMore={objects.data.hasMore}
                nextCursor={objects.data.nextCursor}
                currentCount={objects.data.data.length}
                total={holdings.holdings.object}
                label="objects"
              />
            </>
          ) : (
            <AddressLoadState label="objects" error={objects.error} loading={objects.isLoading} />
          )}
        </HoldingSection>
        <HoldingSection
          kind="identity"
          summary={summary('identity')}
          active={active}
          onActive={setActive}
        >
          {identities.data && <AddressStaleNotice error={identities.error} label="identities" />}
          {holdings.holdings.identity === 0 ? (
            <p className="text-text-dim py-4 text-xs">No identity cells held.</p>
          ) : identities.data ? (
            <>
              {identities.data.data.map((c) => {
                const d = c.display;
                if (!d || d.kind !== 'identity' || c.heldSinceBlock === null)
                  throw new Error(
                    `Identity cell ${c.txHash}:${c.outputIndex} has no identity descriptor`
                  );
                return (
                  <div
                    key={`${c.txHash}:${c.outputIndex}`}
                    className="border-base-elevated space-y-1 border-b py-3"
                  >
                    <div className="flex flex-wrap justify-between gap-2">
                      <Link
                        href={getIdentityItemDetailHref(d.standard, d.id)}
                        className="text-text-bright text-[15px]"
                      >
                        {d.name ?? <Hash hash={d.id} copyable={false} />}
                      </Link>
                      <span className="text-text-dim text-xs">{formatStandard(d.standard)}</span>
                    </div>
                    <div className="text-text-dim flex flex-wrap justify-between gap-2 text-xs">
                      <span>
                        {c.expiresAt === null
                          ? 'Expiry unavailable'
                          : `expires ${new Date(c.expiresAt * 1000).toISOString().slice(0, 10)}`}{' '}
                        · held since{' '}
                        <Link href={`/blocks/${c.heldSinceBlock!}`}>
                          #{c.heldSinceBlock!.toLocaleString('en-US')}
                        </Link>
                      </span>
                      <span>live</span>
                    </div>
                  </div>
                );
              })}
              <AddressPagination
                pagination={identitiesPage}
                hasMore={identities.data.hasMore}
                nextCursor={identities.data.nextCursor}
                currentCount={identities.data.data.length}
                total={holdings.holdings.identity}
                label="identities"
              />
            </>
          ) : (
            <AddressLoadState
              label="identities"
              error={identities.error}
              loading={identities.isLoading}
            />
          )}
        </HoldingSection>
      </div>
    </section>
  );
}
