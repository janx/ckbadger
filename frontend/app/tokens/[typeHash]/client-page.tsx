'use client';

import { useState } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { DataField } from '@/components/ui/data-field';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { CapacityOccupationSection } from '@/components/ui/capacity-occupation-section';
import { api, TokenHolder, TokenActivity, TokenTransferDetail } from '@/lib/api';
import { getOccupationRangeParams, OccupationRangeKey } from '@/lib/occupation-range';
import { formatTimeAgo, formatNumber } from '@/lib/utils';

function actionBadgeVariant(action: string): 'green' | 'red' | 'neutral' {
  if (action === 'mint') return 'green';
  if (action === 'burn') return 'red';
  return 'neutral';
}

export interface TokenDetailPageProps {
  typeHash: string;
}

export default function TokenDetailPage({ typeHash }: TokenDetailPageProps) {
  const [activeTab, setActiveTab] = useState('activities');
  const [occupationRange, setOccupationRange] = useState<OccupationRangeKey>('all');

  const holdersPagination = useCursorPagination();
  const activitiesPagination = useCursorPagination();
  const occupationRangeParams = getOccupationRangeParams(occupationRange);

  const {
    data: token,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['token', typeHash],
    queryFn: () => api.getToken(typeHash),
  });

  const { data: holders } = useQuery({
    queryKey: ['token-holders', typeHash, holdersPagination.cursor],
    queryFn: () => api.getTokenHolders(typeHash, { limit: 20, cursor: holdersPagination.cursor }),
    enabled: !!token && activeTab === 'holders',
    placeholderData: keepPreviousData,
  });

  const { data: activities } = useQuery({
    queryKey: ['token-activities', typeHash, activitiesPagination.cursor],
    queryFn: () =>
      api.getTokenActivities(typeHash, { limit: 20, cursor: activitiesPagination.cursor }),
    enabled: !!token && activeTab === 'activities',
    placeholderData: keepPreviousData,
  });

  const { data: occupationChart, isLoading: isOccupationChartLoading } = useQuery({
    queryKey: ['token-occupation-chart', typeHash, occupationRange],
    queryFn: () =>
      occupationRangeParams
        ? api.getTokenOccupationChart(typeHash, occupationRangeParams)
        : api.getTokenOccupationChart(typeHash),
    enabled: !!token,
  });

  const formatTokenAmount = (amount: string, decimals: number) => {
    const num = BigInt(amount);
    const divisor = BigInt(10 ** decimals);
    const whole = num / divisor;
    const remainder = num % divisor;
    const integer = whole.toLocaleString('en-US');
    if (decimals === 0) {
      return { integer, decimal: '' };
    }
    const decimal = remainder.toString().padStart(decimals, '0');
    return { integer, decimal };
  };

  const TokenAmount = ({ amount, decimals }: { amount: string; decimals: number }) => {
    const { integer, decimal } = formatTokenAmount(amount, decimals);
    return (
      <span className="font-mono tabular-nums">
        <span>{integer}</span>
        {decimal && <span className="text-emphasis-dim text-[0.85em]">.{decimal}</span>}
      </span>
    );
  };

  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="bg-base-surface h-20 w-full rounded" />
            <div className="grid gap-6 lg:grid-cols-2">
              <div className="bg-base-surface h-48 rounded" />
              <div className="bg-base-surface h-48 rounded" />
            </div>
            <div className="bg-base-surface h-96 rounded" />
          </div>
        </main>
      </div>
    );
  }

  if (error || !token) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-muted text-xl">Token not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title={token.symbol || token.name || 'Unknown Token'}
          subtitle={
            <div className="flex items-center gap-2">
              <HexDisplay value={token.typeScriptHash} truncate color="accent" size="sm" />
            </div>
          }
          badge={
            <div className="flex items-center gap-2">
              <Badge variant="neutral">{token.standard.toUpperCase()}</Badge>
              {token.published && (
                <span className="text-emphasis" title="Verified">
                  <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 20 20">
                    <path
                      fillRule="evenodd"
                      d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                      clipRule="evenodd"
                    />
                  </svg>
                </span>
              )}
              {token.famous && (
                <span className="text-warning" title="Famous">
                  <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 20 20">
                    <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                  </svg>
                </span>
              )}
            </div>
          }
        />

        {token.tags && token.tags.length > 0 && (
          <div className="mb-6 flex flex-wrap gap-2">
            {token.tags.map((tag) => (
              <Badge key={tag} variant="gray">
                {tag}
              </Badge>
            ))}
          </div>
        )}

        <div
          className={`mb-6 grid gap-6 ${token.operatorWebsite || token.manager || token.email ? 'lg:grid-cols-2' : ''}`}
        >
          <TerminalPanel glow>
            <TerminalPanelHeader indicator="active">Overview</TerminalPanelHeader>
            <TerminalPanelContent>
              <StatGrid columns={token.operatorWebsite || token.manager || token.email ? 2 : 3}>
                <StatBlock label="Holders" value={token.holdersCount} color="green" />
                <StatBlock label="Transfers" value={token.transfersCount} color="amber" />
                <StatBlock label="Decimals" value={token.decimals} color="white" />
                <StatBlock
                  label="Total Circulation"
                  value={(() => {
                    const { integer, decimal } = formatTokenAmount(
                      token.totalSupply,
                      token.decimals
                    );
                    return decimal ? `${integer}.${decimal}` : integer;
                  })()}
                  color="green"
                />
                <StatBlock
                  label="Maximum Supply"
                  value={(() => {
                    if (token.maximumSupplyStatus === 'unlimited') return 'Unlimited';
                    if (token.maximumSupplyStatus !== 'limited' || !token.maximumSupply) {
                      return 'Unknown';
                    }
                    const { integer, decimal } = formatTokenAmount(
                      token.maximumSupply,
                      token.decimals
                    );
                    return decimal ? `${integer}.${decimal}` : integer;
                  })()}
                  color="white"
                />
                {token.cellsCount != null && (
                  <StatBlock label="Cells" value={token.cellsCount} color="white" />
                )}
              </StatGrid>
              {token.description && (
                <div className="border-base-border mt-4 border-t pt-4">
                  <div className="text-text-muted font-mono text-xs uppercase tracking-wider">
                    Description
                  </div>
                  <div className="text-text-secondary mt-1 text-sm">{token.description}</div>
                </div>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          {(token.operatorWebsite || token.manager || token.email) && (
            <TerminalPanel>
              <TerminalPanelHeader indicator="none">Token Info</TerminalPanelHeader>
              <TerminalPanelContent>
                <div className="space-y-1">
                  {token.operatorWebsite && (
                    <DataField label="Website">
                      <a
                        href={token.operatorWebsite}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-emphasis hover:underline"
                      >
                        {token.operatorWebsite}
                      </a>
                    </DataField>
                  )}
                  {token.manager && (
                    <DataField label="Manager">
                      <Link
                        href={`/address/${token.manager}`}
                        className="text-emphasis font-mono text-sm hover:underline"
                      >
                        {token.manager.length > 40
                          ? `${token.manager.slice(0, 20)}...${token.manager.slice(-20)}`
                          : token.manager}
                      </Link>
                    </DataField>
                  )}
                  {token.email && (
                    <DataField label="Contact">
                      <a href={`mailto:${token.email}`} className="text-emphasis hover:underline">
                        {token.email}
                      </a>
                    </DataField>
                  )}
                  {token.udtType && (
                    <DataField label="UDT Type">
                      <span className="text-text-primary">{token.udtType}</span>
                    </DataField>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          )}
        </div>

        <CapacityOccupationSection
          className="mb-6"
          description="Daily cumulative live CKB occupation for token cells."
          occupationRange={occupationRange}
          onOccupationRangeChange={setOccupationRange}
          occupationChart={occupationChart}
          isOccupationChartLoading={isOccupationChartLoading}
          totalCapacity={token.totalCapacity}
          occupiedCapacity={token.totalOccupiedCapacity}
          totalCapacityLabel="Cells Capacity"
        />

        <TerminalPanel>
          <Tabs value={activeTab} onValueChange={setActiveTab}>
            <TerminalPanelHeader
              actions={
                <TabsList>
                  <TabsTrigger value="activities">
                    Activities ({formatNumber(token.transfersCount)})
                  </TabsTrigger>
                  <TabsTrigger value="holders">
                    Holders ({formatNumber(token.holdersCount)})
                  </TabsTrigger>
                </TabsList>
              }
            >
              {activeTab === 'activities' ? 'Activities' : 'Holders'}
            </TerminalPanelHeader>

            <TabsContent value="activities">
              <TerminalPanelContent padding="none">
                {activities?.data?.length ? (
                  <div className="space-y-3 p-4">
                    {activities.data.map((activity: TokenActivity) => (
                      <div
                        key={`${activity.txHash}-${activity.txIndex}`}
                        className="border-base-border bg-base-surface/40 space-y-2 rounded border p-3"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div className="text-text-muted font-mono text-xs">
                            Block{' '}
                            <Link
                              href={`/blocks/${activity.blockNumber}`}
                              className="text-emphasis hover:underline"
                            >
                              #{formatNumber(activity.blockNumber)}
                            </Link>
                            <span className="text-text-muted mx-1">&bull;</span>
                            Tx Index {activity.txIndex}
                          </div>
                          <div className="flex flex-wrap gap-1.5">
                            {activity.actions.map((action) => (
                              <Badge
                                key={`${activity.txHash}-${action}`}
                                variant={actionBadgeVariant(action)}
                              >
                                {action}
                              </Badge>
                            ))}
                          </div>
                        </div>
                        <div className="flex items-center justify-between gap-2">
                          <Link
                            href={`/tx/${activity.txHash}`}
                            className="text-text-secondary block font-mono text-xs hover:underline"
                          >
                            <HexDisplay
                              value={activity.txHash}
                              color="accent"
                              size="sm"
                              startChars={14}
                              endChars={10}
                            />
                          </Link>
                          <span className="text-text-muted font-mono text-xs">
                            {formatTimeAgo(activity.timestamp)}
                          </span>
                        </div>
                        {activity.transfers.length > 0 && (
                          <div className="border-base-border/50 space-y-1 border-t pt-2">
                            {activity.transfers.map(
                              (transfer: TokenTransferDetail, idx: number) => (
                                <div
                                  key={idx}
                                  className="flex flex-wrap items-center gap-1.5 font-mono text-xs"
                                >
                                  {transfer.isMint ? (
                                    <span className="text-positive">Mint</span>
                                  ) : transfer.fromAddress ? (
                                    <Address
                                      address={transfer.fromAddress}
                                      className="text-text-muted"
                                    />
                                  ) : transfer.fromLockHash ? (
                                    <Link href={`/address/${transfer.fromLockHash}`}>
                                      <HexDisplay
                                        value={transfer.fromLockHash}
                                        color="accent"
                                        startChars={6}
                                        endChars={4}
                                        className="hover:underline"
                                      />
                                    </Link>
                                  ) : (
                                    <span className="text-text-muted">-</span>
                                  )}
                                  <svg
                                    className="text-text-muted h-3 w-3 flex-shrink-0"
                                    fill="none"
                                    viewBox="0 0 24 24"
                                    stroke="currentColor"
                                    strokeWidth={2}
                                  >
                                    <path
                                      strokeLinecap="round"
                                      strokeLinejoin="round"
                                      d="M13 7l5 5m0 0l-5 5m5-5H6"
                                    />
                                  </svg>
                                  {transfer.isBurn ? (
                                    <span className="text-negative">Burn</span>
                                  ) : transfer.toAddress ? (
                                    <Address
                                      address={transfer.toAddress}
                                      className="text-text-muted"
                                    />
                                  ) : (
                                    <Link href={`/address/${transfer.toLockHash}`}>
                                      <HexDisplay
                                        value={transfer.toLockHash}
                                        color="accent"
                                        startChars={6}
                                        endChars={4}
                                        className="hover:underline"
                                      />
                                    </Link>
                                  )}
                                  <span className="text-text-primary ml-auto">
                                    <TokenAmount
                                      amount={transfer.amount}
                                      decimals={token.decimals}
                                    />
                                  </span>
                                </div>
                              )
                            )}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="text-text-muted py-8 text-center">No activities</div>
                )}
              </TerminalPanelContent>
              {activities?.data?.length ? (
                <TerminalPanelFooter>
                  <CursorPagination
                    total={activities.total ?? undefined}
                    totalLabel="activities"
                    pageSize={20}
                    page={activitiesPagination.page}
                    currentCount={activities.data?.length ?? 0}
                    hasMore={activities.hasMore}
                    hasPrevious={activitiesPagination.hasPrevious}
                    onNext={() => activitiesPagination.goToNext(activities.nextCursor)}
                    onPrevious={activitiesPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              ) : null}
            </TabsContent>

            <TabsContent value="holders">
              <TerminalPanelContent padding="none">
                {holders?.data?.length ? (
                  <>
                    <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                      <div className="flex-1">Address</div>
                      <div className="w-48 text-right">Balance</div>
                    </div>
                    {holders.data.map((holder: TokenHolder) => (
                      <TerminalRow key={holder.lockScriptHash}>
                        <div className="flex items-center">
                          <div className="flex-1">
                            {holder.address ? (
                              <Address address={holder.address} />
                            ) : (
                              <Link href={`/address/${holder.lockScriptHash}`}>
                                <HexDisplay
                                  value={holder.lockScriptHash}
                                  color="accent"
                                  className="hover:underline"
                                />
                              </Link>
                            )}
                          </div>
                          <div className="text-text-primary w-48 text-right">
                            <TokenAmount amount={holder.balance} decimals={token.decimals} />
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                  </>
                ) : (
                  <div className="text-text-muted py-8 text-center">No holders</div>
                )}
              </TerminalPanelContent>
              {holders?.data?.length ? (
                <TerminalPanelFooter>
                  <CursorPagination
                    total={holders.total ?? undefined}
                    totalLabel="holders"
                    pageSize={20}
                    page={holdersPagination.page}
                    hasMore={holders.hasMore}
                    hasPrevious={holdersPagination.hasPrevious}
                    onNext={() => holdersPagination.goToNext(holders.nextCursor)}
                    onPrevious={holdersPagination.goToPrevious}
                  />
                </TerminalPanelFooter>
              ) : null}
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
