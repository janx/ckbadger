'use client';
import { useQuery } from '@tanstack/react-query';
import { api, isNotFoundApiError } from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetHistory } from '@/components/asset/asset-history';
import { AssetFeed } from '@/components/asset/asset-feed';
import { AssetAmount, AssetCount } from '@/components/asset/inventory-table';
import { TokenHolders } from '@/components/asset/token-holders';
import { DetailSection } from '@/components/ui/detail-layout';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import { AppLink } from '@/components/ui/app-link';
import { formatTimeAgo } from '@/lib/utils';
import { ExactCkb, formatStandard, activityHeadline } from '@/components/activity-event-row';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
/** The supply-event route always reports its total; a page without one is a contract break. */
function supplyEventsTotal(page: { total?: number }): number {
  if (page.total === undefined) throw new Error('supply events page has no total');
  return page.total;
}
export default function TokenDetailPage({ typeHash }: { typeHash: string }) {
  const token = useQuery({ queryKey: ['token', typeHash], queryFn: () => api.getToken(typeHash) });
  const history = useQuery({
    queryKey: ['token-history', typeHash],
    queryFn: () => api.getTokenHistory(typeHash),
  });
  const supplyEvents = useQuery({
    queryKey: ['token-supply-events', typeHash],
    queryFn: () => api.getTokenSupplyEvents(typeHash, { limit: 100 }),
  });
  const latest = useQuery({
    queryKey: ['token-latest-activity', typeHash],
    queryFn: () => api.getTokenActivityFeed(typeHash, { limit: 1 }),
  });
  const concentration = useQuery({
    queryKey: ['token-distribution', typeHash],
    queryFn: () => api.getTokenDistribution(typeHash),
  });
  const t = token.data;
  if (!t && isNotFoundApiError(token.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'token',
          identifier: typeHash,
          checkedEndpoint: `/tokens/${encodeURIComponent(typeHash)}`,
        }}
      />
    );
  }
  if (!t)
    return (
      <AssetDetailShell
        title="Token"
        kind="tokens"
        breadcrumb={[{ label: 'Inventory', href: '/inventory/tokens' }]}
      >
        <AssetReadState error={token.error} loading={token.isLoading} label="token" />
      </AssetDetailShell>
    );
  const held =
    t.totalCapacity === null ? null : exactUnsignedInteger(t.totalCapacity, 'token held capacity');
  const occupied =
    t.totalCommonKnowledgeSize === null
      ? null
      : exactUnsignedInteger(t.totalCommonKnowledgeSize, 'token occupied capacity');
  if (held !== null && occupied !== null && occupied > held)
    throw new Error(`${typeHash}: token occupied capacity exceeds held capacity`);
  const tokenLabel = t.symbol ?? t.name ?? 'Token';
  const maximum =
    t.maximumSupplyStatus === 'limited' ? (
      t.maximumSupply === null ? (
        <span>Unavailable</span>
      ) : (
        <AssetAmount amount={t.maximumSupply} decimals={t.decimals} />
      )
    ) : t.maximumSupplyStatus === 'unlimited' ? (
      'Unlimited'
    ) : (
      'Unknown'
    );
  return (
    <AssetDetailShell
      kind="tokens"
      title={tokenLabel}
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/tokens' },
        { label: 'Tokens', href: '/inventory/tokens' },
      ]}
      identity={
        <>
          {formatStandard(t.standard)} ·{' '}
          {t.decimals === null ? 'Decimals unknown' : `${t.decimals} decimals`}
          {t.name && t.name !== t.symbol && <> · {t.name}</>}
        </>
      }
      intro={
        <div className="space-y-3 text-xs">
          <AssetReadState error={token.error} label="token" retained />
          {t.description && (
            <p className="text-text-dim">
              {t.description}
              {t.operatorWebsite && (
                <>
                  {' '}
                  ·{' '}
                  <a
                    href={t.operatorWebsite}
                    target="_blank"
                    rel="noreferrer"
                    className="text-jade break-all"
                  >
                    {t.operatorWebsite} ↗
                  </a>
                </>
              )}
            </p>
          )}
          <p>
            <AssetAmount amount={t.totalSupply} decimals={t.decimals} /> {t.symbol} exist, held by{' '}
            <AssetCount value={t.holdersCount} /> addresses.{' '}
            {concentration.data && BigInt(concentration.data.totalSupply) > BigInt(0) && (
              <>
                The largest holder owns{' '}
                {formatScaledInteger(
                  BigInt(concentration.data.largestHolderBalance) * BigInt(100),
                  BigInt(concentration.data.totalSupply),
                  2
                )}
                % of the supply.{' '}
              </>
            )}
            {held !== null && (
              <>
                Its cells hold <ExactCkb shannons={held} tone="plain" signed={false} wrap />
                {occupied !== null && held > BigInt(0) && (
                  <>, {formatScaledInteger(occupied * BigInt(100), held, 2)}% occupied</>
                )}
                .
              </>
            )}
          </p>
          {latest.data ? (
            <>
              <AssetReadState error={latest.error} label="last token movement" retained />
              {latest.data.data[0] ? (
                <p>
                  Last transaction involving this token{' '}
                  <AppLink href={`/tx/${latest.data.data[0].txHash}`}>
                    {formatTimeAgo(latest.data.data[0].timestamp)}
                  </AppLink>
                  : {activityHeadline(latest.data.data[0])}.
                </p>
              ) : (
                <p className="text-text-dim">No recorded token movements.</p>
              )}
            </>
          ) : (
            <AssetReadState
              error={latest.error}
              loading={latest.isLoading}
              label="last token movement"
            />
          )}
          {t.sameSymbolTokens === undefined ? (
            <p className="text-text-dim">Same-symbol comparison unavailable.</p>
          ) : (
            t.sameSymbolTokens.length > 0 && (
              <p className="border-token/40 border-l-2 pl-3">
                Other tokens also call themselves {t.symbol}:{' '}
                {t.sameSymbolTokens.map((other, i) => (
                  <span key={other.typeScriptHash}>
                    {i > 0 && '; '}
                    <AppLink href={`/tokens/${other.typeScriptHash}`}>
                      <Hash hash={other.typeScriptHash} copyable={false} className="text-xs" />
                    </AppLink>
                    , <AssetCount value={other.holdersCount} /> holders
                  </span>
                ))}
                . Tell them apart by type hash.
              </p>
            )
          )}
        </div>
      }
      facts={[
        {
          label: 'Supply',
          value: (
            <>
              <AssetAmount amount={t.totalSupply} decimals={t.decimals} /> {t.symbol}
              <div className="text-text-dim mt-1 text-[11px]">Maximum supply: {maximum}</div>
            </>
          ),
        },
        { label: 'Holders', value: <AssetCount value={t.holdersCount} /> },
        {
          label: 'Transfers',
          value: (
            <>
              <AssetCount value={t.transfersCount} />
              <span className="text-text-dim text-[11px]">
                {' '}
                · <AssetCount value={t.transfers24h} /> in 24 h
              </span>
            </>
          ),
        },
        {
          label: 'Issuer',
          value: t.issuer ? (
            <>
              <AppLink href={`/address/${t.issuer.address ?? t.issuer.lockHash}`}>
                <Hash
                  hash={t.issuer.address ?? t.issuer.lockHash}
                  startChars={12}
                  endChars={6}
                  copyable={false}
                  className="text-xs"
                />
              </AppLink>
              <p className="text-text-dim mt-1 text-[11px]">
                {t.issuer.lockKind ?? 'Lock kind unavailable'} · owner lock proven by token script
              </p>
            </>
          ) : (
            <span className="text-text-dim text-xs">Not established by this token’s script</span>
          ),
        },
        {
          label: 'CKB held in its cells',
          value:
            held !== null && occupied !== null ? (
              <CapacityPartition
                subject="Token cells"
                total={held.toString()}
                occupied={occupied.toString()}
                free={(held - occupied).toString()}
              />
            ) : (
              <span className="text-text-dim">Capacity unavailable</span>
            ),
        },
        { label: 'Live cells', value: <AssetCount value={t.cellsCount} /> },
        {
          label: 'Type hash',
          value: <Hash hash={t.typeScriptHash} truncate={false} className="break-all text-xs" />,
        },
      ]}
      feed={
        <AssetFeed
          queryKey={`token:${typeHash}`}
          scope={`Every transaction that moved ${tokenLabel}`}
          loadActivities={(p) => api.getTokenActivityFeed(typeHash, p)}
          loadTransactions={(p) => api.getTokenTransactions(typeHash, p)}
          token={{ typeHash, symbol: t.symbol, decimals: t.decimals }}
        />
      }
      tier2={
        <DetailSection title="Type script">
          <dl className="space-y-4 text-xs">
            <div>
              <dt className="text-text-dim">Code hash · {t.typeHashType}</dt>
              <dd className="mt-1">
                <AppLink
                  href={getScriptDetailHref({
                    codeHash: t.typeCodeHash,
                    hashType: t.typeHashType,
                    scriptKind: 'type',
                  })}
                >
                  <Hash hash={t.typeCodeHash} truncate={false} className="break-all text-xs" />
                </AppLink>
              </dd>
            </div>
            <div>
              <dt className="text-text-dim">Args</dt>
              <dd className="mt-1">
                <Hash hash={t.typeArgs} truncate={false} className="break-all text-xs" />
              </dd>
            </div>
            <div>
              <dt className="text-text-dim">Name · symbol · decimals</dt>
              <dd className="mt-1">
                {t.name ?? 'Name unavailable'} · {t.symbol ?? 'Symbol unavailable'} ·{' '}
                {t.decimals ?? 'Decimals unknown'}
              </dd>
              <dd className="text-text-dim mt-1 text-[11px]">
                Metadata provenance not recorded. These displayed values do not establish whether
                the source was an imported label or on-chain information.
              </dd>
            </div>
          </dl>
        </DetailSection>
      }
    >
      {history.data ? (
        <>
          <AssetReadState error={history.error} label="token history" retained />
          <AssetHistory
            daily={history.data.daily}
            weekly={history.data.weeklyTransactions.map((w) => ({
              weekStart: w.weekStart,
              count: w.transactionsCount,
            }))}
            weeklyNoun="transactions"
            completeThrough={history.data.completeThrough}
            supplyEvents={
              supplyEvents.data
                ? { rows: supplyEvents.data.data, total: supplyEventsTotal(supplyEvents.data) }
                : undefined
            }
            token={{ symbol: t.symbol, decimals: t.decimals }}
          />
          <AssetReadState error={supplyEvents.error} label="supply events" retained />
          <p className="text-text-dim -mt-5 text-xs">
            <AssetCount value={history.data.mintCount} /> mints ·{' '}
            <AssetCount value={history.data.burnCount} /> burns ·{' '}
            <AssetCount value={history.data.transfersCount} /> transfers.
            {held !== null && occupied !== null && occupied > BigInt(0) && (
              <> H-multiplier ×{formatScaledInteger(held, occupied, 2)}: CKB held ÷ occupied.</>
            )}
          </p>
        </>
      ) : (
        <AssetReadState error={history.error} loading={history.isLoading} label="token history" />
      )}
      <TokenHolders typeHash={typeHash} symbol={t.symbol} decimals={t.decimals} />
    </AssetDetailShell>
  );
}
