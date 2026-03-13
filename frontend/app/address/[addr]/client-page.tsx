'use client';
import { useState, useMemo } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import Image from '@/components/ui/image';
import { Header } from '@/components/layout/header';
import { PageHeader, Badge } from '@/components/ui/page-header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { HexDisplay } from '@/components/ui/hex-display';
import { Capacity } from '@/components/ui/capacity';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type AddressToken, type DaoDeposit, type Activity } from '@/lib/api';
import { ActivityEventGroup } from '@/components/activity-event-row';
import { useParams } from '@/src/navigation';
import { formatTimeAgo, formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { formatTokenBalance } from '@/lib/format-asset';
export default function AddressDetailPage() {
  const params = useParams();
  const addr = params.addr as string;
  return <AddressDetailPageContent key={addr} addr={addr} />;
}
function AddressDetailPageContent({ addr }: { addr: string }) {
  const [selectedToken, setSelectedToken] = useState<AddressToken | null>(null);
  const [selectedDao, setSelectedDao] = useState(false);
  const [activeTab, setActiveTab] = useState<'activities' | 'cells' | 'transactions'>('activities');
  const [activityFilter, setActivityFilter] = useState<
    'all' | 'ckb' | 'token' | 'object' | 'identity' | 'dao' | 'script_call'
  >('all');
  const [cellFilter, setCellFilter] = useState<'all' | 'ckb' | 'token' | 'dao'>('all');
  const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';
  const activitiesPagination = useCursorPagination();
  const cellsPagination = useCursorPagination();
  const txPagination = useCursorPagination();
  const daoPagination = useCursorPagination();
  const {
    data: address,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['address', addr],
    queryFn: () => api.getAddress(addr),
  });
  const { data: tokens } = useQuery({
    queryKey: ['address-tokens', address?.lockScriptHash],
    queryFn: () =>
      api.getAddressTokens(address!.lockScriptHash, {
        limit: 100,
      }),
    enabled: !!address,
  });
  const { data: daoSummary } = useQuery({
    queryKey: ['address-dao-summary', address?.lockScriptHash],
    queryFn: () => api.getAddressDaoSummary(address!.lockScriptHash),
    enabled: !!address,
  });
  const { data: daoDeposits } = useQuery({
    queryKey: ['address-dao-deposits', address?.lockScriptHash, daoPagination.cursor],
    queryFn: () =>
      api.getDaoDepositsByAddress(address!.lockScriptHash, {
        limit: 100,
        cursor: daoPagination.cursor,
      }),
    enabled: !!address && !!daoSummary?.hasDaoActivity,
    placeholderData: keepPreviousData,
  });
  const { data: activities, isLoading: activitiesLoading } = useQuery({
    queryKey: [
      'address-activities',
      address?.lockScriptHash,
      activitiesPagination.cursor,
      activityFilter,
    ],
    queryFn: () =>
      api.getAddressActivities(address!.lockScriptHash, {
        limit: 20,
        cursor: activitiesPagination.cursor,
        filter: activityFilter,
      }),
    enabled: !!address,
    placeholderData: keepPreviousData,
  });
  const tokenMap = useMemo(() => {
    const map = new Map<string, AddressToken>();
    if (tokens?.data) {
      tokens.data.forEach((token) => {
        map.set(token.typeScriptHash, token);
      });
    }
    return map;
  }, [tokens?.data]);
  const cellsTypeFilter = selectedDao ? null : selectedToken?.typeScriptHash;
  const cellsCodeHashFilter = selectedDao ? DAO_CODE_HASH : undefined;
  const { data: cells, isLoading: cellsLoading } = useQuery({
    queryKey: [
      'address-cells',
      address?.lockScriptHash,
      cellsTypeFilter,
      cellsCodeHashFilter,
      cellsPagination.cursor,
    ],
    queryFn: () =>
      api.getLiveCells({
        lockScriptHash: address!.lockScriptHash,
        typeScriptHash: cellsTypeFilter ?? undefined,
        typeCodeHash: cellsCodeHashFilter,
        limit: 20,
        cursor: cellsPagination.cursor,
      }),
    enabled: !!address,
    placeholderData: keepPreviousData,
  });
  const filteredCells = useMemo(() => {
    if (!cells?.data || cellFilter === 'all') return cells?.data;
    return cells.data.filter((cell) => {
      switch (cellFilter) {
        case 'ckb':
          return !cell.typeScriptHash && !cell.udtAmount;
        case 'token':
          return !!cell.udtAmount;
        case 'dao':
          return cell.typeCodeHash?.toLowerCase() === DAO_CODE_HASH.toLowerCase();
        default:
          return true;
      }
    });
  }, [cells?.data, cellFilter]);
  const { data: transactions, isLoading: txLoading } = useQuery({
    queryKey: ['address-transactions', address?.lockScriptHash, txPagination.cursor],
    queryFn: () =>
      api.getAddressTransactions(address!.lockScriptHash, {
        limit: 20,
        cursor: txPagination.cursor,
      }),
    enabled: !!address,
    placeholderData: keepPreviousData,
  });
  const handleTokenSelect = (token: AddressToken | null) => {
    setSelectedToken(token);
    setSelectedDao(false);
    cellsPagination.reset();
    if (token) {
      setActiveTab('cells');
    }
  };
  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <div className="animate-pulse">
            <div className="bg-base-surface mb-8 h-12 w-64 rounded" />
            <div className="mb-8 grid gap-4 md:grid-cols-3">
              <div className="bg-base-surface h-32 rounded" />
              <div className="bg-base-surface h-32 rounded" />
              <div className="bg-base-surface h-32 rounded" />
            </div>
            <div className="bg-base-surface h-96 rounded" />
          </div>
        </main>
      </div>
    );
  }
  if (error || !address) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">Address not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  const shortHash = (value: string) => {
    if (value.length <= 20) return value;
    return `${value.slice(0, 10)}...${value.slice(-8)}`;
  };
  const tokenDisplayName = (token: {
    symbol?: string | null;
    name?: string | null;
    typeScriptHash?: string;
  }) => {
    const symbol = token.symbol?.trim();
    if (symbol) return symbol;
    const name = token.name?.trim();
    if (name) return name;
    return token.typeScriptHash ? shortHash(token.typeScriptHash) : 'Token';
  };
  const parseDaoCellData = (dataHex: string | undefined): number | null => {
    if (!dataHex || dataHex === '0x' || dataHex.length < 18) return null;
    const hex = dataHex.startsWith('0x') ? dataHex.slice(2) : dataHex;
    if (hex.length < 16) return null;
    const bytes = [];
    for (let i = 0; i < 16; i += 2) {
      bytes.push(parseInt(hex.slice(i, i + 2), 16));
    }
    let blockNumber = 0;
    for (let i = 7; i >= 0; i--) {
      blockNumber = blockNumber * 256 + bytes[i];
    }
    return blockNumber;
  };
  const isDaoCell = (cell: { typeCodeHash?: string }): boolean => {
    if (!cell.typeCodeHash) return false;
    return cell.typeCodeHash.toLowerCase() === DAO_CODE_HASH.toLowerCase();
  };
  const getDaoStatusBadge = (status: string) => {
    switch (status) {
      case 'deposited':
        return <Badge variant="green">Active</Badge>;
      case 'withdrawing':
        return <Badge variant="gold">Withdraw Request</Badge>;
      case 'withdrawn':
        return <Badge variant="gray">Completed</Badge>;
      default:
        return <Badge variant="gray">{status}</Badge>;
    }
  };
  const formatDaoDuration = (depositTimestamp: string, endTimestamp?: string | null): string => {
    const start = new Date(depositTimestamp).getTime();
    const end = endTimestamp ? new Date(endTimestamp).getTime() : Date.now();
    const days = Math.floor((end - start) / (1000 * 60 * 60 * 24));
    if (days < 1) return '<1 day';
    if (days === 1) return '1 day';
    return `${days} days`;
  };
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Address"
          hash={address.address || address.lockScriptHash}
          badge={
            <div className="flex items-center gap-2">
              <Badge variant="green">Active</Badge>
              {address.lockScriptInfo && (
                <Link
                  href={`/scripts/${encodeURIComponent(address.lockScriptInfo.name)}`}
                  className="border-base-border bg-base-elevated/70 text-text hover:bg-base-elevated inline-flex items-center gap-1.5 rounded border px-2 py-0.5 text-xs font-medium transition-colors"
                >
                  {address.lockScriptInfo.name}
                </Link>
              )}
              {address.lockScriptInfo?.deprecated && <Badge variant="red">Deprecated</Badge>}
            </div>
          }
        />
        <TerminalPanel className="mb-8">
          <TerminalPanelContent>
            <div className="grid grid-cols-2 gap-6 md:grid-cols-4">
              <StatBlock
                label="Balance"
                value={formatCkbAmount(address.balance).full}
                suffix=" CKB"
                color="jade"
                className="col-span-2"
              />
              <StatBlock label="Live Cells" value={address.liveCellsCount} color="gold" />
              <StatBlock label="Transactions" value={address.transactionsCount} color="default" />
            </div>
            {(() => {
              const balanceBig = BigInt(address.balance);
              const usedBig = BigInt(address.usedCapacity);
              if (balanceBig <= BigInt(0) || usedBig <= BigInt(0)) return null;
              const freeBig = balanceBig - usedBig;
              const ratio = Number((usedBig * BigInt(10000)) / balanceBig) / 100;
              return (
                <div className="mt-6">
                  <div className="mb-2 flex items-center justify-between">
                    <span className="text-text-dim font-mono text-xs uppercase tracking-wider">
                      Capacity Utilization
                    </span>
                    <span className="text-text font-mono text-xs">{ratio.toFixed(1)}% used</span>
                  </div>
                  <div className="bg-base-elevated flex h-3 w-full overflow-hidden rounded-sm">
                    <div
                      className="bg-warning transition-all duration-300"
                      style={{ width: `${Math.max(ratio, 0.5)}%` }}
                    />
                    <div className="bg-emphasis/30 flex-1" />
                  </div>
                  <div className="mt-1.5 flex items-center justify-between">
                    <span
                      className="text-warning font-mono text-xs"
                      title={formatCkbAmount(address.usedCapacity).full + ' CKB'}
                    >
                      Used: {formatCkbCompact(address.usedCapacity).value} CKB
                    </span>
                    <span
                      className="text-emphasis font-mono text-xs"
                      title={formatCkbAmount(freeBig.toString()).full + ' CKB'}
                    >
                      Unused: {formatCkbCompact(freeBig.toString()).value} CKB
                    </span>
                  </div>
                </div>
              );
            })()}
          </TerminalPanelContent>
        </TerminalPanel>
        {daoSummary?.hasDaoActivity && (
          <TerminalPanel className="mb-8" variant="elevated">
            <TerminalPanelHeader>
              <div className="flex items-center gap-2">
                <div className="bg-base-elevated text-text flex h-5 w-5 items-center justify-center rounded-full text-xs">
                  D
                </div>
                <Link href="/dao" className="hover:text-emphasis transition-colors">
                  Nervos DAO
                </Link>
                {daoSummary.estimatedApc && (
                  <span className="text-positive text-xs font-normal">
                    {daoSummary.estimatedApc}% APC
                  </span>
                )}
              </div>
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <StatGrid columns={4}>
                <StatBlock
                  label="Total Locked"
                  value={formatCkbCompact(daoSummary.totalLockedCapacity).value}
                  suffix=" CKB"
                  color="default"
                  subtext={formatCkbAmount(daoSummary.totalLockedCapacity).full}
                />
                <StatBlock
                  label="Active Deposits"
                  value={daoSummary.activeDepositsCount}
                  color="jade"
                />
                <StatBlock
                  label="Pending Withdrawals"
                  value={daoSummary.pendingWithdrawalsCount}
                  color={daoSummary.pendingWithdrawalsCount > 0 ? 'gold' : 'default'}
                />
                <StatBlock
                  label="Compensation Earned"
                  value={`+${formatCkbCompact(daoSummary.totalCompensationEarned).value}`}
                  suffix=" CKB"
                  color="jade"
                  subtext={formatCkbAmount(daoSummary.totalCompensationEarned).full}
                />
              </StatGrid>
            </TerminalPanelContent>
            {daoDeposits?.data && daoDeposits.data.length > 0 && (
              <>
                <TerminalPanelContent padding="none">
                  <div className="min-w-full overflow-x-auto">
                    <div
                      className="border-base-border bg-base-surface/50 text-text-dim hidden items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider md:grid"
                      style={{ gridTemplateColumns: '10rem 6rem 1fr 1fr 6rem 5.5rem' }}
                    >
                      <div>Deposit</div>
                      <div>Status</div>
                      <div className="text-right">Capacity</div>
                      <div className="text-right">Compensation</div>
                      <div>Duration</div>
                      <div className="text-right">Time</div>
                    </div>
                    {daoDeposits.data.map((deposit: DaoDeposit) => (
                      <TerminalRow key={`dao-${deposit.txHash}-${deposit.outputIndex}`}>
                        <div
                          className="hidden w-full items-start gap-x-4 md:grid"
                          style={{ gridTemplateColumns: '10rem 6rem 1fr 1fr 6rem 5.5rem' }}
                        >
                          <div>
                            <Link href={`/tx/${deposit.txHash}`}>
                              <HexDisplay
                                value={deposit.txHash}
                                truncate
                                startChars={6}
                                endChars={6}
                              />
                            </Link>
                            <Link
                              href={`/blocks/${deposit.depositBlockNumber}`}
                              className="text-text-dim hover:text-text block font-mono text-xs"
                            >
                              #{deposit.depositBlockNumber.toLocaleString()}
                            </Link>
                          </div>
                          <div className="self-center">{getDaoStatusBadge(deposit.status)}</div>
                          <div className="self-center text-right">
                            <Capacity value={deposit.capacity} className="text-text-bright" />
                          </div>
                          <div className="self-center text-right">
                            {deposit.compensation ? (
                              <span className="text-positive font-mono text-sm">
                                +{formatCkbAmount(deposit.compensation).full} CKB
                              </span>
                            ) : deposit.status === 'deposited' ? (
                              <span className="text-text-dim text-sm">Accruing...</span>
                            ) : (
                              <span className="text-text-dim">-</span>
                            )}
                          </div>
                          <div className="text-text self-center font-mono text-sm">
                            {formatDaoDuration(
                              deposit.depositTimestamp,
                              deposit.withdrawTimestamp || deposit.withdrawRequestTimestamp
                            )}
                          </div>
                          <div className="text-text-dim self-center text-right text-sm">
                            {formatTimeAgo(deposit.depositTimestamp)}
                          </div>
                        </div>
                        <div className="space-y-1.5 md:hidden">
                          <div className="flex items-center justify-between gap-2">
                            <Link href={`/tx/${deposit.txHash}`}>
                              <HexDisplay
                                value={deposit.txHash}
                                truncate
                                startChars={6}
                                endChars={6}
                              />
                            </Link>
                            {getDaoStatusBadge(deposit.status)}
                          </div>
                          <div className="flex items-center justify-between gap-2 text-sm">
                            <Capacity value={deposit.capacity} className="text-text-bright" />
                            {deposit.compensation ? (
                              <span className="text-positive font-mono">
                                +{formatCkbAmount(deposit.compensation).full} CKB
                              </span>
                            ) : deposit.status === 'deposited' ? (
                              <span className="text-text-dim">Accruing...</span>
                            ) : null}
                          </div>
                          <div className="text-text-dim flex items-center gap-3 text-xs">
                            <span>
                              {formatDaoDuration(
                                deposit.depositTimestamp,
                                deposit.withdrawTimestamp || deposit.withdrawRequestTimestamp
                              )}
                            </span>
                            <span>{formatTimeAgo(deposit.depositTimestamp)}</span>
                            <Link
                              href={`/blocks/${deposit.depositBlockNumber}`}
                              className="text-text-dim font-mono hover:underline"
                            >
                              #{deposit.depositBlockNumber.toLocaleString()}
                            </Link>
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                  </div>
                </TerminalPanelContent>
                {(daoDeposits.hasMore || daoPagination.hasPrevious) && (
                  <TerminalPanelFooter className="flex justify-center">
                    <CursorPagination
                      total={daoDeposits.total}
                      totalLabel="deposits"
                      pageSize={100}
                      hasMore={daoDeposits.hasMore}
                      hasPrevious={daoPagination.hasPrevious}
                      page={daoPagination.page}
                      onNext={() => daoPagination.goToNext(daoDeposits.nextCursor)}
                      onPrevious={daoPagination.goToPrevious}
                    />
                  </TerminalPanelFooter>
                )}
              </>
            )}
          </TerminalPanel>
        )}
        {tokens?.data && tokens.data.length > 0 && (
          <TerminalPanel className="mb-8" variant="elevated">
            <TerminalPanelHeader>Holdings ({tokens?.data?.length || 0})</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="min-w-full">
                <div className="border-base-border bg-base-surface/50 text-text-dim hidden border-b px-4 py-2 font-mono text-xs uppercase tracking-wider sm:flex">
                  <div className="min-w-0 flex-1">Asset</div>
                  <div className="w-28 shrink-0">Standard</div>
                  <div className="w-44 shrink-0 text-right">Balance</div>
                </div>
                {[...(tokens?.data ?? [])]
                  .sort((a, b) => {
                    const nameA = (a.name || a.symbol || '').toLowerCase();
                    const nameB = (b.name || b.symbol || '').toLowerCase();
                    return nameA.localeCompare(nameB);
                  })
                  .map((token) => {
                    const isSelected = selectedToken?.typeScriptHash === token.typeScriptHash;
                    return (
                      <TerminalRow
                        key={token.typeScriptHash}
                        className={`cursor-pointer ${isSelected ? 'bg-base-elevated/80' : ''}`}
                      >
                        <div
                          className="hidden w-full items-center sm:flex"
                          onClick={() => handleTokenSelect(isSelected ? null : token)}
                        >
                          <div className="flex min-w-0 flex-1 items-center gap-3">
                            {token.iconUrl && (
                              <Image
                                src={token.iconUrl}
                                alt={token.symbol || token.name || 'Token'}
                                className="h-6 w-6 rounded-full"
                                width={24}
                                height={24}
                                unoptimized
                                onError={(event) => {
                                  event.currentTarget.style.display = 'none';
                                }}
                              />
                            )}
                            <div>
                              <Link
                                href={`/tokens/${token.typeScriptHash}`}
                                onClick={(e) => e.stopPropagation()}
                                className="text-emphasis font-medium hover:underline"
                              >
                                {tokenDisplayName(token)}
                              </Link>
                              {token.symbol && token.name && (
                                <span className="text-text-dim ml-2 text-xs">{token.symbol}</span>
                              )}
                            </div>
                          </div>
                          <div className="w-28 shrink-0">
                            <Badge variant="gray">{token.standard}</Badge>
                          </div>
                          <div className="w-44 shrink-0 text-right">
                            <span className="text-text-bright font-mono">
                              {formatTokenBalance(token.balance, token.decimals)}
                            </span>
                          </div>
                        </div>
                        <div
                          className="w-full space-y-1 sm:hidden"
                          onClick={() => handleTokenSelect(isSelected ? null : token)}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <div className="flex min-w-0 items-center gap-2">
                              {token.iconUrl && (
                                <Image
                                  src={token.iconUrl}
                                  alt={token.symbol || token.name || 'Token'}
                                  className="h-6 w-6 rounded-full"
                                  width={24}
                                  height={24}
                                  unoptimized
                                  onError={(event) => {
                                    event.currentTarget.style.display = 'none';
                                  }}
                                />
                              )}
                              <Link
                                href={`/tokens/${token.typeScriptHash}`}
                                onClick={(e) => e.stopPropagation()}
                                className="text-emphasis truncate font-medium hover:underline"
                              >
                                {tokenDisplayName(token)}
                              </Link>
                              <Badge variant="gray">{token.standard}</Badge>
                            </div>
                          </div>
                          <div className="text-text-bright text-right font-mono text-sm">
                            {formatTokenBalance(token.balance, token.decimals)}
                          </div>
                        </div>
                      </TerminalRow>
                    );
                  })}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}
        <TerminalPanel>
          <TerminalPanelHeader
            actions={
              <div className="flex gap-2">
                <button
                  onClick={() => setActiveTab('activities')}
                  className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                    activeTab === 'activities'
                      ? 'bg-emphasis/15 text-emphasis'
                      : 'text-text hover:text-text-bright'
                  }`}
                >
                  Activities
                  <span className="ml-1 opacity-75">({address.transactionsCount})</span>
                </button>
                <button
                  onClick={() => setActiveTab('cells')}
                  className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                    activeTab === 'cells'
                      ? 'bg-emphasis/15 text-emphasis'
                      : 'text-text hover:text-text-bright'
                  }`}
                >
                  Live Cells
                  {selectedToken || selectedDao ? (
                    <span className="ml-1 opacity-75">({cells?.total ?? '...'})</span>
                  ) : (
                    <span className="ml-1 opacity-75">({address.liveCellsCount})</span>
                  )}
                </button>
                <button
                  onClick={() => setActiveTab('transactions')}
                  className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                    activeTab === 'transactions'
                      ? 'bg-emphasis/15 text-emphasis'
                      : 'text-text hover:text-text-bright'
                  }`}
                >
                  Transactions
                  <span className="ml-1 opacity-75">({address.transactionsCount})</span>
                </button>
              </div>
            }
          >
            {activeTab === 'activities' ? (
              'Activities'
            ) : activeTab === 'cells' ? (
              <div className="flex items-center gap-2">
                <span>Cells</span>
                {selectedToken && (
                  <Badge variant="gold" className="ml-2">
                    Filter: {tokenDisplayName(selectedToken)}
                  </Badge>
                )}
                {selectedDao && (
                  <Badge variant="neutral" className="ml-2">
                    Filter: Nervos DAO
                  </Badge>
                )}
              </div>
            ) : (
              'Transactions'
            )}
          </TerminalPanelHeader>
          {selectedToken && activeTab === 'cells' && (
            <div className="border-base-border bg-base-surface/50 flex items-center justify-between border-b px-4 py-2">
              <span className="text-text text-sm">
                Showing cells for{' '}
                <span className="text-warning">{tokenDisplayName(selectedToken)}</span>
              </span>
              <button
                onClick={() => handleTokenSelect(null)}
                className="text-negative text-xs hover:underline"
              >
                Clear Filter
              </button>
            </div>
          )}
          {selectedDao && activeTab === 'cells' && (
            <div className="border-base-border bg-base-surface/50 flex items-center justify-between border-b px-4 py-2">
              <span className="text-text text-sm">
                Showing cells for <span className="text-text">Nervos DAO</span>
              </span>
              <button
                onClick={() => setSelectedDao(false)}
                className="text-negative text-xs hover:underline"
              >
                Clear Filter
              </button>
            </div>
          )}
          <TerminalPanelContent padding="none">
            {activeTab === 'activities' && (
              <>
                <div className="border-base-border flex items-center gap-2 border-b px-4 py-2">
                  <label
                    htmlFor="activity-filter"
                    className="text-text-dim font-mono text-xs uppercase tracking-wider"
                  >
                    Filter
                  </label>
                  <select
                    id="activity-filter"
                    value={activityFilter}
                    onChange={(e) => {
                      setActivityFilter(e.target.value as typeof activityFilter);
                      activitiesPagination.reset();
                    }}
                    className="border-base-border bg-base-elevated text-text focus:ring-jade/50 rounded border px-2 py-1 font-mono text-xs focus:outline-none focus:ring-1"
                  >
                    <option value="all">All</option>
                    <option value="ckb">CKB</option>
                    <option value="token">Token</option>
                    <option value="object">Object</option>
                    <option value="identity">Identity</option>
                    <option value="dao">DAO</option>
                    <option value="script_call">Script Call</option>
                  </select>
                </div>
                {activitiesLoading ? (
                  <div className="text-text-dim py-12 text-center">Loading activities...</div>
                ) : activities?.data && activities?.data.length > 0 ? (
                  <>
                    <div
                      className="md:grid md:items-baseline md:gap-x-4"
                      style={{ gridTemplateColumns: '13rem 1fr auto 5rem' }}
                    >
                      {activities?.data.map((activity: Activity, idx: number) => (
                        <ActivityEventGroup
                          key={`${activity.txHash}-${activity.txIndex}`}
                          activity={activity}
                          formatTimeAgo={(ts) => formatTimeAgo(Number(ts))}
                          isFirst={idx === 0}
                        />
                      ))}
                    </div>
                    {(activities?.hasMore || activitiesPagination.hasPrevious) && (
                      <TerminalPanelFooter className="flex justify-center">
                        <CursorPagination
                          total={activityFilter === 'all' ? address.transactionsCount : undefined}
                          totalLabel="activities"
                          pageSize={20}
                          hasMore={activities?.hasMore ?? false}
                          hasPrevious={activitiesPagination.hasPrevious}
                          page={activitiesPagination.page}
                          onNext={() => activitiesPagination.goToNext(activities?.nextCursor)}
                          onPrevious={activitiesPagination.goToPrevious}
                        />
                      </TerminalPanelFooter>
                    )}
                  </>
                ) : (
                  <div className="text-text-dim py-12 text-center">
                    {activityFilter === 'all'
                      ? 'No activities'
                      : `No ${activityFilter === 'ckb' ? 'CKB' : activityFilter === 'script_call' ? 'Script Call' : activityFilter.charAt(0).toUpperCase() + activityFilter.slice(1)} activities on this page`}
                  </div>
                )}
              </>
            )}
            {activeTab === 'cells' && (
              <>
                <div className="border-base-border flex items-center gap-1.5 border-b px-4 py-2">
                  {(['all', 'ckb', 'token', 'dao'] as const).map((f) => (
                    <button
                      key={f}
                      onClick={() => setCellFilter(f)}
                      className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                        cellFilter === f
                          ? 'bg-emphasis/15 text-emphasis'
                          : 'text-text-dim hover:text-text'
                      }`}
                    >
                      {{ all: 'All', ckb: 'CKB', token: 'Token', dao: 'DAO' }[f]}
                    </button>
                  ))}
                </div>
                <div className="p-4">
                  {cellsLoading ? (
                    <div className="text-text-dim py-12 text-center">Loading cells...</div>
                  ) : filteredCells && filteredCells.length > 0 ? (
                    <>
                      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
                        {filteredCells.map((cell) => {
                          const cellToken = cell.typeScriptHash
                            ? tokenMap.get(cell.typeScriptHash)
                            : undefined;
                          const cellIsDao = isDaoCell(cell);
                          const daoDepositBlock = cellIsDao ? parseDaoCellData(cell.data) : null;
                          const daoDepositInfo = cellIsDao
                            ? daoDeposits?.data?.find(
                                (d) =>
                                  d.txHash === cell.txHash && d.outputIndex === cell.outputIndex
                              )
                            : null;
                          return (
                            <TerminalPanel
                              key={`${cell.txHash}-${cell.outputIndex}`}
                              variant="inset"
                              className="h-full"
                            >
                              <TerminalPanelContent padding="sm">
                                <div className="mb-3 flex items-center justify-between">
                                  <Link
                                    href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                                    className="text-emphasis hover:underline"
                                  >
                                    <HexDisplay
                                      value={`${cell.txHash}:${cell.outputIndex}`}
                                      truncate
                                      startChars={6}
                                      endChars={6}
                                      size="sm"
                                    />
                                  </Link>
                                  <span className="text-text-dim font-mono text-xs">
                                    #{cell.createdAtBlock.toLocaleString()}
                                  </span>
                                </div>
                                <div className="border-base-border bg-base-surface/50 mb-2 rounded border p-2">
                                  <Capacity
                                    value={cell.capacity}
                                    className="text-text-bright text-lg"
                                  />
                                </div>
                                {cellIsDao && (
                                  <div className="border-base-border bg-base-surface/50 rounded border px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="text-text font-mono">Nervos DAO</span>
                                      {daoDepositInfo && (
                                        <Badge
                                          variant={
                                            daoDepositInfo.status === 'deposited'
                                              ? 'green'
                                              : daoDepositInfo.status === 'withdrawing'
                                                ? 'gold'
                                                : 'gray'
                                          }
                                        >
                                          {daoDepositInfo.status === 'deposited'
                                            ? 'Active'
                                            : daoDepositInfo.status === 'withdrawing'
                                              ? 'Withdraw Request'
                                              : 'Completed'}
                                        </Badge>
                                      )}
                                    </div>
                                    {daoDepositBlock !== null && daoDepositBlock > 0 && (
                                      <div className="text-text-dim mt-1 text-xs">
                                        Deposit Block: #{daoDepositBlock.toLocaleString()}
                                      </div>
                                    )}
                                    {daoDepositInfo?.compensation && (
                                      <div className="text-positive mt-1 text-xs">
                                        +{formatCkbAmount(daoDepositInfo.compensation).full} CKB
                                        compensation
                                      </div>
                                    )}
                                  </div>
                                )}
                                {cellToken && cell.udtAmount && (
                                  <div className="bg-gold/10 border-gold-dim/30 rounded border px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="text-warning-dim font-mono">
                                        {formatTokenBalance(cell.udtAmount, cellToken.decimals)}
                                      </span>
                                      <span className="text-warning text-xs">
                                        {tokenDisplayName(cellToken)}
                                      </span>
                                    </div>
                                  </div>
                                )}
                                {!cellToken && !cellIsDao && cell.udtAmount && (
                                  <div className="border-base-border bg-base-elevated/50 rounded border px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="text-text font-mono">
                                        {formatTokenBalance(cell.udtAmount, 0)}
                                      </span>
                                      {cell.typeScriptHash ? (
                                        <Link
                                          href={`/tokens/${cell.typeScriptHash}`}
                                          className="text-text-dim hover:text-text text-xs hover:underline"
                                        >
                                          {shortHash(cell.typeScriptHash)}
                                        </Link>
                                      ) : (
                                        <span className="text-text-dim text-xs">Token</span>
                                      )}
                                    </div>
                                  </div>
                                )}
                                {!cellToken &&
                                  !cellIsDao &&
                                  !cell.udtAmount &&
                                  cell.dataSize > 0 && (
                                    <div className="border-base-border bg-base-elevated/30 rounded border px-2 py-1">
                                      <span className="text-text-dim font-mono text-xs">
                                        {cell.dataSize} bytes data
                                      </span>
                                    </div>
                                  )}
                              </TerminalPanelContent>
                            </TerminalPanel>
                          );
                        })}
                      </div>
                      {(cells?.hasMore || cellsPagination.hasPrevious) && (
                        <TerminalPanelFooter className="mt-4 flex justify-center border-t-0">
                          <CursorPagination
                            total={
                              !selectedToken && !selectedDao ? address.liveCellsCount : undefined
                            }
                            totalLabel="cells"
                            pageSize={20}
                            hasMore={cells?.hasMore ?? false}
                            hasPrevious={cellsPagination.hasPrevious}
                            page={cellsPagination.page}
                            onNext={() => cellsPagination.goToNext(cells?.nextCursor)}
                            onPrevious={cellsPagination.goToPrevious}
                          />
                        </TerminalPanelFooter>
                      )}
                    </>
                  ) : (
                    <div className="text-text-dim py-12 text-center">
                      {cellFilter !== 'all'
                        ? `No ${cellFilter === 'ckb' ? 'CKB' : cellFilter.toUpperCase()} cells on this page`
                        : selectedToken
                          ? `No cells found for ${selectedToken.symbol || selectedToken.name}`
                          : 'No live cells'}
                    </div>
                  )}
                </div>
              </>
            )}
            {activeTab === 'transactions' && (
              <>
                {txLoading ? (
                  <div className="text-text-dim py-12 text-center">Loading transactions...</div>
                ) : transactions?.data && transactions.data.length > 0 ? (
                  <>
                    <div className="border-base-border bg-base-surface/50 text-text-dim hidden items-center gap-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider lg:flex">
                      <div className="flex-1">Transaction</div>
                      <div className="w-20 text-center">In/Out</div>
                      <div className="w-32 text-right">Fee</div>
                      <div className="hidden w-28 text-right xl:block">Size/Cycles</div>
                      <div className="w-44 text-right">CKB Change</div>
                      <div className="w-20 text-right">Time</div>
                    </div>
                    {transactions.data.map((tx) => {
                      const isPositive = !tx.capacityChange.startsWith('-');
                      const fee = Number(tx.fee);
                      const feeRate =
                        tx.txSize && tx.txSize > 0 && fee > 0 ? fee / tx.txSize : null;
                      return (
                        <TerminalRow key={tx.txHash}>
                          <div className="hidden w-full items-center gap-4 lg:flex">
                            <div className="min-w-0 flex-1">
                              <div className="flex flex-wrap items-center gap-1.5">
                                <Link href={`/tx/${tx.txHash}`}>
                                  <HexDisplay
                                    value={tx.txHash}
                                    truncate
                                    startChars={6}
                                    endChars={6}
                                  />
                                </Link>
                                {tx.isCellbase && <Badge variant="neutral">Cellbase</Badge>}
                                {tx.scriptLabels.map((label) => (
                                  <Badge key={label} variant="neutral">
                                    {label}
                                  </Badge>
                                ))}
                              </div>
                              <Link
                                href={`/blocks/${tx.blockNumber}`}
                                className="text-text-dim hover:text-text block font-mono text-xs"
                              >
                                #{tx.blockNumber.toLocaleString()}
                              </Link>
                            </div>
                            <div className="text-text w-20 text-center font-mono">
                              <span className="text-emphasis/70">{tx.inputsCount}</span>
                              <span className="text-text-dim mx-1">→</span>
                              <span className="text-emphasis/70">{tx.outputsCount}</span>
                            </div>
                            <div className="w-32 whitespace-nowrap text-right">
                              {tx.isCellbase ? (
                                <span className="text-text-dim">{'\u2014'}</span>
                              ) : (
                                <div>
                                  <Capacity value={tx.fee} className="text-text" />
                                  {feeRate != null && (
                                    <div className="text-text-dim font-mono text-xs">
                                      {feeRate.toFixed(1)} shannons/B
                                    </div>
                                  )}
                                </div>
                              )}
                            </div>
                            <div className="text-text-dim hidden w-28 whitespace-nowrap text-right font-mono text-xs xl:block">
                              {tx.txSize != null ? (
                                <span>
                                  {tx.txSize >= 1000
                                    ? `${(tx.txSize / 1000).toFixed(1)} kB`
                                    : `${tx.txSize} B`}
                                </span>
                              ) : (
                                <span className="text-text-dim">{'\u2014'}</span>
                              )}
                              {tx.cycles != null && (
                                <>
                                  <span className="text-text-dim mx-1">/</span>
                                  <span>
                                    {tx.cycles >= 1000000
                                      ? `${(tx.cycles / 1000000).toFixed(1)}M`
                                      : `${(tx.cycles / 1000).toFixed(0)}k`}
                                  </span>
                                </>
                              )}
                            </div>
                            <div className="w-44 whitespace-nowrap text-right">
                              <Capacity
                                value={tx.capacityChange}
                                className={isPositive ? 'text-positive' : 'text-negative'}
                                showSign
                              />
                            </div>
                            <div className="text-text-dim w-20 text-right text-sm">
                              {formatTimeAgo(tx.timestamp)}
                            </div>
                          </div>
                          <div className="space-y-1.5 lg:hidden">
                            <div className="flex items-center justify-between gap-2">
                              <Link href={`/tx/${tx.txHash}`}>
                                <HexDisplay
                                  value={tx.txHash}
                                  truncate
                                  startChars={8}
                                  endChars={6}
                                />
                              </Link>
                              <span className="text-text-dim shrink-0 text-xs">
                                {formatTimeAgo(tx.timestamp)}
                              </span>
                            </div>
                            <div className="flex items-center justify-between gap-2">
                              <div className="text-text-dim flex items-center gap-3 font-mono text-xs">
                                <span>
                                  <span className="text-emphasis-dim">{tx.inputsCount}</span>
                                  <span className="mx-1">→</span>
                                  <span className="text-emphasis-dim">{tx.outputsCount}</span>
                                </span>
                                <span>Fee: {formatCkbAmount(tx.fee).full}</span>
                              </div>
                              <span
                                className={`font-mono text-sm ${tx.capacityChange.startsWith('-') ? 'text-negative' : 'text-positive'}`}
                              >
                                {!tx.capacityChange.startsWith('-') && '+'}
                                {formatCkbAmount(tx.capacityChange).full} CKB
                              </span>
                            </div>
                          </div>
                        </TerminalRow>
                      );
                    })}
                    {(transactions.hasMore || txPagination.hasPrevious) && (
                      <TerminalPanelFooter className="flex justify-center">
                        <CursorPagination
                          total={address.transactionsCount}
                          totalLabel="transactions"
                          pageSize={20}
                          hasMore={transactions.hasMore}
                          hasPrevious={txPagination.hasPrevious}
                          page={txPagination.page}
                          onNext={() => txPagination.goToNext(transactions.nextCursor)}
                          onPrevious={txPagination.goToPrevious}
                        />
                      </TerminalPanelFooter>
                    )}
                  </>
                ) : (
                  <div className="text-text-dim py-12 text-center">No transactions</div>
                )}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
