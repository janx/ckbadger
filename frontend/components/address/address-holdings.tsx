'use client';

import { useState, type ReactNode } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { api, type AddressHoldings, type AddressCellClass } from '@/lib/api';
import { ClassMark } from '@/components/ui/class-mark';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import Link from '@/components/ui/link';
import { ExactCkb, formatStandard } from '@/components/activity-event-row';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePageSize } from '@/hooks/usePageSize';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { splitTokenAmount } from '@/lib/format-asset';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { cn, formatCkbAmount, formatUtc8Date } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { CompositionSet, compositionCountsFromAddress } from '@/components/ui/composition-set';
import { CursorPagination } from '@/components/ui/cursor-pagination';

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
    <p className="text-text-dim text-small py-3" role={error ? 'alert' : 'status'}>
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
    <p role="alert" className="text-warning text-small py-2">
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
      className="text-small mt-4 flex flex-wrap items-center justify-between gap-3"
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
/** "free part of 5 deposits": what the DAO free figure is made of (BA-37). */
function daoFreeNote(holdings: AddressHoldings): string | undefined {
  const { depositCellsCount: deposits, withdrawalRequestCellsCount: requests } = holdings;
  if (deposits + requests === 0) return undefined;
  const parts = [
    ...(deposits ? [formatCount(deposits, 'deposit')] : []),
    ...(requests ? [formatCount(requests, 'withdrawal request')] : []),
  ];
  return `free part of ${parts.join(' and ')}`;
}

/**
 * Display order (BA-34): the four asset classes by the space they take, largest first; other
 * scripts and plain CKB stay last.
 */
function spaceOrder(holdings: AddressHoldings) {
  const assets = ADDRESS_CLASSES.slice(0, 4);
  const occupied = (key: AddressCellClass) => BigInt(holdings.classes[key].occupiedCapacity);
  return [
    ...[...assets].sort((a, b) =>
      occupied(b.key) > occupied(a.key) ? 1 : occupied(b.key) < occupied(a.key) ? -1 : 0
    ),
    ...ADDRESS_CLASSES.slice(4),
  ];
}

/**
 * Largest collections first; among equals the fully on-chain ones (BTC+CKB, then Pure CKB),
 * then by name, so the order never depends on the API's id order (BA-34).
 */
function collectionOrder(groups: AddressHoldings['objectCollections']) {
  const key = (g: AddressHoldings['objectCollections'][number]) => g.name ?? g.id ?? '';
  return [...groups].sort(
    (a, b) =>
      b.itemCount - a.itemCount ||
      b.compositionCounts.btcCkb - a.compositionCounts.btcCkb ||
      b.compositionCounts.pureCkb - a.compositionCounts.pureCkb ||
      key(a).localeCompare(key(b), 'en')
  );
}

export function AddressBalance({ holdings }: { holdings: AddressHoldings }) {
  return (
    <section aria-label="Balance" className="space-y-4">
      <div className="text-text-bright text-hero font-medium">
        <ExactCkb shannons={BigInt(holdings.balance)} signed={false} tone="plain" wrap />
      </div>
      <CapacityPartition
        subject="Address"
        showTotal={false}
        total={holdings.balance}
        occupied={holdings.occupiedCapacity}
        free={holdings.freeCapacity}
        daoFree={holdings.daoFreeCapacity}
        minSegmentWidth={3}
        daoFreeNote={daoFreeNote(holdings)}
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
        <h3 className="text-text-bright text-body flex items-center gap-2 font-medium">
          <ClassMark kind={cls.mark} size={16} />
          {kind === 'dao' ? 'Nervos DAO deposits' : cls.label}
        </h3>
        <span className="text-text-dim text-small max-w-full">{summary}</span>
      </div>
      {children}
    </section>
  );
}

export function AddressHoldingsSection({ holdings }: { holdings: AddressHoldings }) {
  const addr = holdings.lockScriptHash;
  const [active, setActive] = useState<AddressCellClass | null>(null);
  const pageSize = usePageSize(50);
  const tokensPage = useCursorPagination(),
    daoPage = useCursorPagination(),
    identitiesPage = useCursorPagination();
  const tokens = useQuery({
    queryKey: ['address-tokens', addr, tokensPage.cursor, pageSize],
    queryFn: () => api.getAddressTokens(addr, { limit: pageSize, cursor: tokensPage.cursor }),
    enabled: holdings.holdings.token > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const deposits = useQuery({
    queryKey: ['address-holdings-cells', addr, 'dao', daoPage.cursor, pageSize],
    // R2b-a 9 (BA-34): largest deposit first, ordered and paged by the API.
    queryFn: () =>
      api.getAddressCells(addr, {
        class: 'dao',
        order: 'capacity',
        limit: pageSize,
        cursor: daoPage.cursor,
      }),
    enabled: holdings.holdings.dao > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const identities = useQuery({
    queryKey: ['address-holdings-cells', addr, 'identity', identitiesPage.cursor, pageSize],
    queryFn: () =>
      api.getAddressCells(addr, {
        class: 'identity',
        limit: pageSize,
        cursor: identitiesPage.cursor,
      }),
    enabled: holdings.holdings.identity > 0,
    placeholderData: keepPreviousData,
    staleTime: 60000,
    refetchInterval: 60000,
  });
  const occupied = exactUnsignedInteger(holdings.occupiedCapacity, 'Address occupied');
  if (
    ADDRESS_CLASSES.reduce(
      (sum, c) => sum + BigInt(holdings.classes[c.key].occupiedCapacity),
      BigInt(0)
    ) !== occupied
  )
    throw new Error('Address class occupied capacities do not equal total');
  // The Address board's class headers: "5 cells · X CKB held", "9 kinds in 15 cells · X CKB held",
  // "14 in 7 collections · X CKB held" (an object is its own cell).
  const held_ = (kind: AddressCellClass) => (
    <>
      <ExactCkb
        shannons={BigInt(holdings.classes[kind].capacity)}
        signed={false}
        tone="plain"
        wrap
      />{' '}
      held
    </>
  );
  const summary = (kind: AddressCellClass) => (
    <>
      {formatCount(holdings.classes[kind].cellCount, 'cell')} ·{' '}
      <ExactCkb
        shannons={BigInt(holdings.classes[kind].capacity)}
        signed={false}
        tone="plain"
        wrap
      />{' '}
      held
    </>
  );
  // Consistency round C14 (Address.dc.html): the classes it holds are one line of links by the
  // heading, the classes it does not hold one dim line at its right — never an empty section.
  const lines = HOLDING_LINE.map((c) => ({ ...c, count: holdings.holdings[c.key] }));
  const held = lines.filter((c) => c.count > 0);
  const missing = lines.filter((c) => c.count === 0);
  return (
    <section aria-labelledby="holdings-heading" className="space-y-7">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1">
          <h2 id="holdings-heading" className="text-text-bright text-section font-medium">
            Holdings
          </h2>
          {held.length > 0 && (
            <nav aria-label="Holdings" className="text-text-dim text-small">
              ·{' '}
              {held.map((c, i) => (
                <span key={c.key}>
                  {i > 0 && ' · '}
                  <a
                    href={`#holdings-${c.key}`}
                    className="hover:text-jade tabular-nums"
                    onMouseEnter={() => setActive(c.key)}
                    onMouseLeave={() => setActive(null)}
                    onFocus={() => setActive(c.key)}
                    onBlur={() => setActive(null)}
                  >
                    {formatCount(c.count, c.one, c.many)}
                  </a>
                </span>
              ))}
            </nav>
          )}
        </div>
        {missing.length > 0 && (
          <span className="text-text-dim text-small">{missing.map((c) => c.none).join(' · ')}</span>
        )}
      </div>
      {holdings.holdings.object > 0 && (
        // C8: what the objects are made of leads the section (Address.dc.html).
        <CompositionSet
          counts={compositionCountsFromAddress(holdings.objectComposition)}
          total={holdings.holdings.object}
        />
      )}
      <div className="space-y-2">
        <div className="text-small flex flex-wrap justify-between gap-2">
          <span className="text-text-dim">On-chain space they take</span>
          <span>
            {formatCkbAmount(occupied).full.replace(/\.0{8}$/, '')} CKB occupied in{' '}
            {formatCount(holdings.liveCellsCount, 'cell')}
          </span>
        </div>
        <div
          role="img"
          aria-label="Occupied capacity by class"
          className="flex h-2.5 gap-0.5 overflow-hidden rounded"
        >
          {spaceOrder(holdings).map((c) => (
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
        <div className="text-small grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
          {spaceOrder(holdings).map((c) => (
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
      {holdings.holdings.object > 0 && (
        <HoldingSection
          kind="object"
          summary={
            <>
              {holdings.holdings.object.toLocaleString('en-US')} in{' '}
              {formatCount(holdings.objectCollections.length, 'collection')} · {held_('object')}
            </>
          }
          active={active}
          onActive={setActive}
        >
          <AddressCollections holdings={holdings} />
        </HoldingSection>
      )}
      <div className="grid items-start gap-x-12 gap-y-9 lg:grid-cols-2">
        {holdings.holdings.dao > 0 && (
          <HoldingSection kind="dao" summary={summary('dao')} active={active} onActive={setActive}>
            {holdings.holdings.dao === 0 ? (
              <p className="text-text-dim text-small py-4">No live DAO holdings.</p>
            ) : (
              <>
                <p className="text-text-dim text-small pt-3">
                  {holdings.depositCellsCount} deposits · {holdings.withdrawalRequestCellsCount}{' '}
                  withdrawal requests
                </p>
                <dl className="text-small grid gap-3 py-3 sm:grid-cols-3">
                  <div>
                    <dt className="text-text-dim">
                      {holdings.withdrawalRequestCellsCount === 0 ? 'Deposited' : 'CKB held'}
                    </dt>
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
                {deposits.data && (
                  <AddressStaleNotice error={deposits.error} label="DAO holdings" />
                )}
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
                        d.status === 'withdrawing'
                          ? d.withdrawRequestTimestamp
                          : d.depositTimestamp;
                      if (eventBlock === null || eventTimestamp === null)
                        throw new Error(
                          `DAO cell ${cell.txHash}:${cell.outputIndex} is missing ${d.status} evidence`
                        );
                      return (
                        <div
                          key={`${cell.txHash}:${cell.outputIndex}`}
                          data-dao-cell
                          className="border-base-elevated text-small flex flex-wrap items-baseline justify-between gap-2 border-t py-2"
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
                              {formatBlockNumber(eventBlock)}
                            </Link>{' '}
                            · {formatUtc8Date(eventTimestamp)}
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
                      total={holdings.holdings.dao}
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
        )}
        {holdings.holdings.token > 0 && (
          <HoldingSection
            kind="token"
            summary={
              <>
                {holdings.holdings.token.toLocaleString('en-US')} kinds in{' '}
                {formatCount(holdings.classes.token.cellCount, 'cell')} · {held_('token')}
              </>
            }
            active={active}
            onActive={setActive}
          >
            {tokens.data && <AddressStaleNotice error={tokens.error} label="tokens" />}
            {holdings.holdings.token === 0 ? (
              <p className="text-text-dim text-small py-4">No tokens held.</p>
            ) : tokens.data ? (
              <>
                {tokens.data.data.map((t) => (
                  <div
                    key={t.typeScriptHash}
                    data-token-row={t.typeScriptHash}
                    className="border-base-elevated text-small flex flex-wrap items-baseline justify-between gap-2 border-b py-2"
                  >
                    <span className="min-w-0">
                      <Link
                        href={`/tokens/${t.typeScriptHash}`}
                        className="text-text-bright hover:text-jade"
                      >
                        {tokenName(t) ?? (
                          <Hash hash={t.typeScriptHash} copyable={false} className="text-small" />
                        )}
                      </Link>{' '}
                      <span className="text-text-dim">{formatStandard(t.standard)}</span>
                      {sharedNames(tokens.data.data).has(tokenName(t) ?? '') && (
                        <span data-type-hash className="text-meta ml-2">
                          <Hash
                            hash={t.typeScriptHash}
                            startChars={4}
                            endChars={2}
                            copyable={false}
                            className="text-meta"
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
        )}
        {holdings.holdings.identity > 0 && (
          <HoldingSection
            kind="identity"
            summary={summary('identity')}
            active={active}
            onActive={setActive}
          >
            {identities.data && <AddressStaleNotice error={identities.error} label="identities" />}
            {holdings.holdings.identity === 0 ? (
              <p className="text-text-dim text-small py-4">No identity cells held.</p>
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
                          className="text-text-bright text-body"
                        >
                          {d.name ?? <Hash hash={d.id} copyable={false} />}
                        </Link>
                        <span className="text-text-dim text-small">
                          {formatStandard(d.standard)}
                        </span>
                      </div>
                      <div className="text-text-dim text-small flex flex-wrap justify-between gap-2">
                        <span>
                          {c.expiresAt === null
                            ? 'Expiry unavailable'
                            : `expires ${formatUtc8Date(c.expiresAt * 1000)}`}{' '}
                          · held since{' '}
                          <Link href={`/blocks/${c.heldSinceBlock!}`}>
                            {formatBlockNumber(c.heldSinceBlock!)}
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
        )}
      </div>
    </section>
  );
}

/** The four asset classes as the Holdings line words them (C14). */
const HOLDING_LINE: {
  key: 'dao' | 'token' | 'object' | 'identity';
  one: string;
  many?: string;
  none: string;
}[] = [
  { key: 'dao', one: 'DAO cell', none: 'no DAO deposits' },
  { key: 'token', one: 'token', none: 'no tokens' },
  { key: 'object', one: 'object', none: 'no objects' },
  { key: 'identity', one: 'identity', many: 'identities', none: 'no identities' },
];

/** Objects shown when a collection is opened in place (C14). */
const COLLECTION_PAGE = 10;
const COLLECTION_GRID =
  'grid min-w-0 grid-cols-[minmax(0,1fr)_56px_auto] items-center gap-x-4 sm:grid-cols-[minmax(0,1.2fr)_minmax(0,2fr)_56px_minmax(0,1fr)_96px]';

/**
 * Consistency round C14 (Address.dc.html): one row per collection — mark, name, a count bar,
 * the count and what its objects are made of — each opening in place to the objects of that
 * collection this address holds (the API filters by collection before paging).
 */
function AddressCollections({ holdings }: { holdings: AddressHoldings }) {
  const [open, setOpen] = useState<string | null>(null);
  const groups = collectionOrder(holdings.objectCollections);
  const max = groups.reduce((m, g) => Math.max(m, g.itemCount), 0);
  return (
    <div className="min-w-0 pt-2">
      <div className={cn(COLLECTION_GRID, 'text-text-dim text-meta h-7')}>
        <span>Collection</span>
        <span className="hidden sm:block">
          Items <span className="text-text-ghost">0 — {max.toLocaleString('en-US')}</span>
        </span>
        <span className="text-right">Items</span>
        <span className="hidden sm:block">Made of</span>
        <span />
      </div>
      {groups.map((g) => {
        const key = `${g.standard}:${g.id}`;
        const isOpen = open === key;
        const label =
          g.name ??
          (g.id === null
            ? `Uncollected ${formatStandard(g.standard)}`
            : `${g.id.slice(0, 10)}…${g.id.slice(-4)}`);
        const href =
          g.id === null ? null : g.standard === 'm-nft' ? `/classes/${g.id}` : `/clusters/${g.id}`;
        return (
          <div key={key}>
            <div
              data-collection-row={g.name ?? g.id ?? `uncollected ${g.standard}`}
              onClick={() => setOpen(isOpen ? null : key)}
              className={cn(
                COLLECTION_GRID,
                'border-base-border-subtle text-small min-h-[41px] cursor-pointer border-t py-1',
                isOpen && 'bg-base-surface'
              )}
            >
              <span className="flex min-w-0 items-center gap-2">
                <ClassMark kind="objects" size={16} />
                {href ? (
                  <Link
                    href={href}
                    className="text-text-bright text-body min-w-0 truncate"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {label}
                  </Link>
                ) : (
                  <span className="text-text-bright text-body min-w-0 truncate">{label}</span>
                )}
              </span>
              <span aria-hidden="true" className="bg-base-border hidden h-2 rounded-[1px] sm:block">
                <span
                  className="bg-text block h-full rounded-[1px]"
                  style={{ width: `${max === 0 ? 0 : (g.itemCount / max) * 100}%` }}
                />
              </span>
              <span className="text-text-bright text-right tabular-nums">
                {g.itemCount.toLocaleString('en-US')}
              </span>
              <span className="hidden flex-wrap gap-1.5 sm:flex">
                {composition
                  .filter(([k]) => g.compositionCounts[k] > 0)
                  .map(([k, tier]) => (
                    <CompositionBadge key={k} tier={tier} size="sm" />
                  ))}
              </span>
              <button
                type="button"
                aria-expanded={isOpen}
                aria-label={`${isOpen ? 'Close' : 'Open'} ${label}`}
                className="text-text-dim text-meta hover:text-text col-start-3 text-right sm:col-start-auto"
              >
                {isOpen ? 'close ↑' : 'objects ↓'}
              </button>
            </div>
            {isOpen && (
              <AddressCollectionObjects
                addr={holdings.lockScriptHash}
                group={g}
                label={label}
                href={href}
              />
            )}
          </div>
        );
      })}
      <p className="text-text-dim text-small mt-3.5 max-w-[640px]">
        Open a collection to list its objects here; every object is one live cell of this address.
      </p>
    </div>
  );
}

const OBJECT_GRID =
  'grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-1 sm:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1fr)_72px]';

/** The objects of one collection this address holds, ten a page (C14). */
function AddressCollectionObjects({
  addr,
  group,
  label,
  href,
}: {
  addr: string;
  group: AddressHoldings['objectCollections'][number];
  label: string;
  href: string | null;
}) {
  const pages = useCursorPagination();
  const objects = useQuery({
    queryKey: ['address-collection-objects', addr, group.standard, group.id, pages.cursor],
    queryFn: () =>
      api.getAddressCells(addr, {
        class: 'object',
        collection: group.id ?? 'none',
        limit: COLLECTION_PAGE,
        cursor: pages.cursor,
      }),
    placeholderData: keepPreviousData,
  });
  const rows = objects.data?.data ?? [];
  for (const c of rows)
    if (!c.display || c.display.kind !== 'object' || c.display.collectionId !== group.id)
      throw new Error(
        `Object cell ${c.txHash}:${c.outputIndex} is not in collection ${group.id ?? 'none'}`
      );
  return (
    <div className="bg-base-surface pl-7">
      {objects.data ? (
        <>
          <AddressStaleNotice error={objects.error} label={`${label} objects`} />
          <div role="table" aria-label={`${label} objects held here`} className="text-small">
            <div role="row" className={cn(OBJECT_GRID, 'text-text-dim text-meta h-7')}>
              <span role="columnheader">Object</span>
              <span role="columnheader">Made of</span>
              <span role="columnheader" className="hidden sm:block">
                Created
              </span>
              <span role="columnheader" className="text-right">
                Cell
              </span>
            </div>
            {rows.map((c) => {
              const d = c.display!;
              return (
                <div
                  key={`${c.txHash}:${c.outputIndex}`}
                  role="row"
                  className={cn(
                    OBJECT_GRID,
                    'border-base-border-subtle min-h-[41px] border-t py-1'
                  )}
                >
                  <span role="cell" className="flex min-w-0 items-center gap-2">
                    <ClassMark kind="objects" size={12} />
                    <Link
                      href={d.standard === 'm-nft' ? `/objects/mnft/${d.id}` : `/objects/${d.id}`}
                      aria-label={d.name ?? `Object ${d.id}`}
                      title={d.id}
                      className="text-text-bright text-body min-w-0 truncate"
                    >
                      {d.name ?? `${d.id.slice(0, 10)}…${d.id.slice(-4)}`}
                    </Link>
                  </span>
                  <span role="cell" className="min-w-0">
                    {d.compositionTier === null ? (
                      <span className="text-text-dim">Composition unavailable</span>
                    ) : (
                      <CompositionBadge tier={d.compositionTier} size="sm" />
                    )}
                  </span>
                  <span role="cell" className="text-text-dim hidden tabular-nums sm:block">
                    <Link href={`/blocks/${c.createdAtBlock}`}>
                      {formatBlockNumber(c.createdAtBlock)}
                    </Link>
                  </span>
                  <span role="cell" className="text-right">
                    <Link
                      href={`/cell/${c.txHash}-${c.outputIndex}`}
                      title={`${c.txHash}:${c.outputIndex}`}
                    >
                      cell →
                    </Link>
                  </span>
                </div>
              );
            })}
          </div>
          <div className="border-base-border-subtle flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-t py-2">
            <CursorPagination
              total={group.itemCount}
              totalLabel="objects"
              pageSize={COLLECTION_PAGE}
              page={pages.page}
              currentCount={rows.length}
              hasMore={objects.data.hasMore}
              hasPrevious={pages.hasPrevious}
              onNext={() => pages.goToNext(objects.data!.nextCursor)}
              onPrevious={pages.goToPrevious}
            />
            {href && (
              <Link href={href} className="text-small">
                All {group.itemCount.toLocaleString('en-US')} in the collection →
              </Link>
            )}
          </div>
        </>
      ) : (
        <AddressLoadState
          label={`${label} objects`}
          error={objects.error}
          loading={objects.isLoading}
        />
      )}
    </div>
  );
}

// ---- F2 blocks (BA-32): a token's type hash only where two rows would read the same ----

/** The name a token row shows: its name, else its symbol; null when it has neither. */
function tokenName(t: { name: string | null; symbol: string | null }): string | null {
  return t.name ?? t.symbol;
}

/** Names that two or more of these rows share; only those rows carry the type hash. */
function sharedNames(rows: readonly { name: string | null; symbol: string | null }[]) {
  const seen = new Map<string, number>();
  for (const row of rows) {
    const name = tokenName(row);
    if (name !== null) seen.set(name, (seen.get(name) ?? 0) + 1);
  }
  return new Set([...seen].filter(([, n]) => n > 1).map(([name]) => name));
}
