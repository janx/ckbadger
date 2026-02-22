'use client';

import { useState, useMemo } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams } from 'next/navigation';
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
import {
  api,
  type AddressToken,
  type DaoDeposit,
  type Activity,
  type ActivityAssetChange,
  type StackedAreaChartResponse,
} from '@/lib/api';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';
import { formatTimeAgo, formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { formatTokenBalance } from '@/lib/format-asset';

export default function AddressDetailPage() {
  const params = useParams();
  const addr = params.addr as string;

  const [selectedToken, setSelectedToken] = useState<AddressToken | null>(null);
  const [selectedDao, setSelectedDao] = useState(false);
  const [activeTab, setActiveTab] = useState<'activities' | 'cells' | 'transactions'>('activities');
  const [activityFilter, setActivityFilter] = useState<'all' | 'ckb' | 'token' | 'nft' | 'dao'>(
    'all'
  );
  const [cellFilter, setCellFilter] = useState<'all' | 'ckb' | 'token' | 'dao'>('all');

  const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';

  const activitiesPagination = useCursorPagination();
  const cellsPagination = useCursorPagination();
  const txPagination = useCursorPagination();
  const daoPagination = useCursorPagination();

  const { data: address, isLoading } = useQuery({
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

  const { data: statsHistory } = useQuery<StackedAreaChartResponse>({
    queryKey: ['address-stats-history', address?.lockScriptHash],
    queryFn: () => api.getAddressStatsHistory(address!.lockScriptHash),
    enabled: !!address,
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

  const daoTxHashes = useMemo(() => {
    const set = new Set<string>();
    if (daoDeposits?.data) {
      daoDeposits.data.forEach((d) => set.add(d.txHash));
    }
    return set;
  }, [daoDeposits?.data]);

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
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse">
            <div className="mb-8 h-12 w-64 rounded bg-slate-900" />
            <div className="mb-8 grid gap-4 md:grid-cols-3">
              <div className="h-32 rounded bg-slate-900" />
              <div className="h-32 rounded bg-slate-900" />
              <div className="h-32 rounded bg-slate-900" />
            </div>
            <div className="h-96 rounded bg-slate-900" />
          </div>
        </main>
      </div>
    );
  }

  if (!address) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Address not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const getTxTypeBadge = (txType: string) => {
    switch (txType) {
      case 'received':
        return <Badge variant="green">Received</Badge>;
      case 'sent':
        return <Badge variant="red">Sent</Badge>;
      case 'internal':
        return <Badge variant="gray">Internal</Badge>;
      default:
        return null;
    }
  };

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
        return <Badge variant="amber">Withdrawing</Badge>;
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

  const AssetChangeBadge = ({ change }: { change: ActivityAssetChange }) => {
    switch (change.type) {
      case 'token': {
        const isPositive = !change.delta.startsWith('-');
        const isZero = change.delta === '0';
        const absDelta = change.delta.startsWith('-') ? change.delta.slice(1) : change.delta;
        const formatted = formatTokenBalance(absDelta, change.decimals ?? 0);
        const sign = isZero ? '' : isPositive ? '+' : '-';
        const color = isZero ? 'text-slate-500' : isPositive ? 'text-green-400' : 'text-red-400';
        const tokenLabel = change.symbol?.trim()
          ? change.symbol.trim()
          : shortHash(change.typeScriptHash);
        return (
          <span className={`font-mono text-xs ${color}`}>
            {sign}
            {formatted}{' '}
            <Link href={`/tokens/${change.typeScriptHash}`} className="hover:underline">
              {tokenLabel}
            </Link>
          </span>
        );
      }
      case 'dob':
        return (
          <Badge variant="neutral">
            {change.action.charAt(0).toUpperCase() + change.action.slice(1)} DOB
          </Badge>
        );
      case 'nft':
        return (
          <Badge variant="neutral">
            {change.action.charAt(0).toUpperCase() + change.action.slice(1)} NFT
          </Badge>
        );
      case 'daoDeposit':
        return <Badge variant="neutral">DAO Deposit</Badge>;
      case 'daoWithdrawRequest':
        return <Badge variant="amber">DAO Withdraw Request</Badge>;
      case 'daoWithdrawComplete':
        return (
          <span className="flex items-center gap-1">
            <Badge variant="green">DAO Withdraw</Badge>
            <span className="font-mono text-xs text-green-400">
              +{formatCkbAmount(change.compensation).full} CKB
            </span>
          </span>
        );
    }
  };

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Address"
          hash={address.address || address.lockScriptHash}
          badge={
            <div className="flex items-center gap-2">
              <Badge variant="green">Active</Badge>
              {address.lockScriptInfo && (
                <Link
                  href={`/scripts/${encodeURIComponent(address.lockScriptInfo.name)}`}
                  className="inline-flex items-center gap-1.5 rounded border border-slate-700 bg-slate-800/70 px-2 py-0.5 text-xs font-medium text-slate-300 transition-colors hover:bg-slate-800"
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
                color="green"
                className="col-span-2"
              />
              <StatBlock label="Live Cells" value={address.liveCellsCount} color="amber" />
              <StatBlock label="Transactions" value={address.transactionsCount} color="white" />
            </div>
            {(() => {
              const balanceBig = BigInt(address.balance);
              const occupiedBig = BigInt(address.occupiedCapacity);
              if (balanceBig <= BigInt(0) || occupiedBig <= BigInt(0)) return null;
              const freeBig = balanceBig - occupiedBig;
              const ratio = Number((occupiedBig * BigInt(10000)) / balanceBig) / 100;
              return (
                <div className="mt-6">
                  <div className="mb-2 flex items-center justify-between">
                    <span className="font-mono text-xs uppercase tracking-wider text-slate-500">
                      Capacity Utilization
                    </span>
                    <span className="font-mono text-xs text-slate-400">
                      {ratio.toFixed(1)}% occupied
                    </span>
                  </div>
                  <div className="flex h-3 w-full overflow-hidden rounded-sm bg-slate-800">
                    <div
                      className="bg-amber transition-all duration-300"
                      style={{ width: `${Math.max(ratio, 0.5)}%` }}
                    />
                    <div className="bg-terminal-green/30 flex-1" />
                  </div>
                  <div className="mt-1.5 flex items-center justify-between">
                    <span
                      className="text-amber font-mono text-xs"
                      title={formatCkbAmount(address.occupiedCapacity).full + ' CKB'}
                    >
                      Occupied: {formatCkbCompact(address.occupiedCapacity).value} CKB
                    </span>
                    <span
                      className="text-terminal-green font-mono text-xs"
                      title={formatCkbAmount(freeBig.toString()).full + ' CKB'}
                    >
                      Unoccupied: {formatCkbCompact(freeBig.toString()).value} CKB
                    </span>
                  </div>
                </div>
              );
            })()}
          </TerminalPanelContent>
        </TerminalPanel>

        {statsHistory && statsHistory.data.length > 0 && (
          <TerminalPanel className="mb-8">
            <TerminalPanelHeader>Address Stats History</TerminalPanelHeader>
            <TerminalPanelContent>
              <MultiSeriesLineChart
                data={statsHistory.data}
                series={statsHistory.series}
                height={300}
              />
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {daoSummary?.hasDaoActivity && (
          <TerminalPanel className="mb-8" variant="elevated">
            <TerminalPanelHeader>
              <div className="flex items-center gap-2">
                <div className="flex h-5 w-5 items-center justify-center rounded-full bg-slate-800 text-xs text-slate-300">
                  D
                </div>
                <Link href="/dao" className="hover:text-terminal-green transition-colors">
                  Nervos DAO
                </Link>
                {daoSummary.estimatedApc && (
                  <span className="text-xs font-normal text-green-400">
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
                  color="white"
                  subtext={formatCkbAmount(daoSummary.totalLockedCapacity).full}
                />
                <StatBlock
                  label="Active Deposits"
                  value={daoSummary.activeDepositsCount}
                  color="green"
                />
                <StatBlock
                  label="Pending Withdrawals"
                  value={daoSummary.pendingWithdrawalsCount}
                  color={daoSummary.pendingWithdrawalsCount > 0 ? 'amber' : 'white'}
                />
                <StatBlock
                  label="Compensation Earned"
                  value={`+${formatCkbCompact(daoSummary.totalCompensationEarned).value}`}
                  suffix=" CKB"
                  color="green"
                  subtext={formatCkbAmount(daoSummary.totalCompensationEarned).full}
                />
              </StatGrid>
            </TerminalPanelContent>
            {daoDeposits?.data && daoDeposits.data.length > 0 && (
              <>
                <TerminalPanelContent padding="none">
                  <div className="min-w-full overflow-x-auto">
                    <div
                      className="grid items-center gap-x-4 border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500"
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
                          className="grid w-full items-start gap-x-4"
                          style={{ gridTemplateColumns: '10rem 6rem 1fr 1fr 6rem 5.5rem' }}
                        >
                          <div>
                            <Link href={`/tx/${deposit.txHash}`}>
                              <HexDisplay
                                value={deposit.txHash}
                                truncate
                                startChars={6}
                                endChars={6}
                                color="accent"
                              />
                            </Link>
                            <Link
                              href={`/blocks/${deposit.depositBlockNumber}`}
                              className="block font-mono text-xs text-slate-500 hover:text-slate-300"
                            >
                              #{deposit.depositBlockNumber.toLocaleString()}
                            </Link>
                          </div>
                          <div className="self-center">{getDaoStatusBadge(deposit.status)}</div>
                          <div className="self-center text-right">
                            <Capacity value={deposit.capacity} className="text-white" />
                          </div>
                          <div className="self-center text-right">
                            {deposit.compensation ? (
                              <span className="font-mono text-sm text-green-400">
                                +{formatCkbAmount(deposit.compensation).full} CKB
                              </span>
                            ) : deposit.status === 'deposited' ? (
                              <span className="text-sm text-slate-500">Accruing...</span>
                            ) : (
                              <span className="text-slate-600">-</span>
                            )}
                          </div>
                          <div className="self-center font-mono text-sm text-slate-400">
                            {formatDaoDuration(
                              deposit.depositTimestamp,
                              deposit.withdrawTimestamp || deposit.withdrawRequestTimestamp
                            )}
                          </div>
                          <div className="self-center text-right text-sm text-slate-500">
                            {formatTimeAgo(deposit.depositTimestamp)}
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
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="flex-1">Asset</div>
                  <div className="w-32">Standard</div>
                  <div className="w-48 text-right">Balance</div>
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
                        className={`cursor-pointer ${isSelected ? 'bg-slate-800/80' : ''}`}
                      >
                        <div
                          className="flex w-full items-center"
                          onClick={() => handleTokenSelect(isSelected ? null : token)}
                        >
                          <div className="flex flex-1 items-center gap-3">
                            {token.iconUrl && (
                              <img
                                src={token.iconUrl}
                                alt={token.symbol || token.name || 'Token'}
                                className="h-6 w-6 rounded-full"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <div>
                              <Link
                                href={`/tokens/${token.typeScriptHash}`}
                                onClick={(e) => e.stopPropagation()}
                                className="text-terminal-green font-medium hover:underline"
                              >
                                {tokenDisplayName(token)}
                              </Link>
                              {token.symbol && token.name && (
                                <span className="ml-2 text-xs text-slate-500">{token.symbol}</span>
                              )}
                            </div>
                          </div>
                          <div className="w-32">
                            <Badge variant="gray">{token.standard}</Badge>
                          </div>
                          <div className="w-48 text-right">
                            <span className="font-mono text-white">
                              {formatTokenBalance(token.balance, token.decimals)}
                            </span>
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
                      ? 'bg-terminal-green/15 text-terminal-green'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  Activities
                  <span className="ml-1 opacity-75">({address.transactionsCount})</span>
                </button>
                <button
                  onClick={() => setActiveTab('cells')}
                  className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                    activeTab === 'cells'
                      ? 'bg-terminal-green/15 text-terminal-green'
                      : 'text-slate-400 hover:text-white'
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
                      ? 'bg-terminal-green/15 text-terminal-green'
                      : 'text-slate-400 hover:text-white'
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
                  <Badge variant="amber" className="ml-2">
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
            <div className="flex items-center justify-between border-b border-slate-800 bg-slate-900/50 px-4 py-2">
              <span className="text-sm text-slate-400">
                Showing cells for{' '}
                <span className="text-amber">{tokenDisplayName(selectedToken)}</span>
              </span>
              <button
                onClick={() => handleTokenSelect(null)}
                className="text-xs text-red-400 hover:underline"
              >
                Clear Filter
              </button>
            </div>
          )}

          {selectedDao && activeTab === 'cells' && (
            <div className="flex items-center justify-between border-b border-slate-800 bg-slate-900/50 px-4 py-2">
              <span className="text-sm text-slate-400">
                Showing cells for <span className="text-slate-300">Nervos DAO</span>
              </span>
              <button
                onClick={() => setSelectedDao(false)}
                className="text-xs text-red-400 hover:underline"
              >
                Clear Filter
              </button>
            </div>
          )}

          <TerminalPanelContent padding="none">
            {activeTab === 'activities' && (
              <>
                <div className="flex items-center gap-1.5 border-b border-slate-800 px-4 py-2">
                  {(['all', 'ckb', 'token', 'nft', 'dao'] as const).map((f) => (
                    <button
                      key={f}
                      onClick={() => {
                        setActivityFilter(f);
                        activitiesPagination.reset();
                      }}
                      className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                        activityFilter === f
                          ? 'bg-terminal-green/15 text-terminal-green'
                          : 'text-slate-500 hover:text-slate-300'
                      }`}
                    >
                      {{ all: 'All', ckb: 'CKB', token: 'Token', nft: 'NFT/DOB', dao: 'DAO' }[f]}
                    </button>
                  ))}
                </div>
                {activitiesLoading ? (
                  <div className="py-12 text-center text-slate-500">Loading activities...</div>
                ) : activities?.data && activities?.data.length > 0 ? (
                  <>
                    <div className="overflow-x-auto">
                      <div className="min-w-[640px]">
                        <div className="flex items-center gap-4 border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                          <div className="flex-1">Transaction</div>
                          <div className="w-20">Type</div>
                          <div className="w-44 text-right">CKB Change</div>
                          <div className="w-32 text-right lg:w-48">Assets</div>
                          <div className="w-20 text-right">Time</div>
                        </div>
                        {activities?.data.map((activity: Activity) => {
                          const delta = BigInt(activity.ckbDelta);
                          const isPositive = delta > BigInt(0);
                          const isNegative = delta < BigInt(0);
                          const deltaColor = isPositive
                            ? 'text-green-400'
                            : isNegative
                              ? 'text-red-400'
                              : 'text-slate-500';
                          return (
                            <TerminalRow key={`${activity.txHash}-${activity.txIndex}`}>
                              <div className="flex w-full items-center gap-4">
                                <div className="min-w-0 flex-1">
                                  <Link href={`/tx/${activity.txHash}`}>
                                    <HexDisplay
                                      value={activity.txHash}
                                      truncate
                                      startChars={6}
                                      endChars={6}
                                      color="accent"
                                    />
                                  </Link>
                                  <Link
                                    href={`/blocks/${activity.blockNumber}`}
                                    className="block font-mono text-xs text-slate-500 hover:text-slate-300"
                                  >
                                    #{activity.blockNumber.toLocaleString()}
                                  </Link>
                                </div>
                                <div className="w-20">
                                  {activity.isCellbase ? (
                                    <Badge variant="amber">Coinbase</Badge>
                                  ) : isPositive ? (
                                    <Badge variant="green">Received</Badge>
                                  ) : isNegative ? (
                                    <Badge variant="red">Sent</Badge>
                                  ) : (
                                    <Badge variant="gray">Self</Badge>
                                  )}
                                </div>
                                <div className="w-44 whitespace-nowrap text-right">
                                  <span className={`font-mono text-sm ${deltaColor}`}>
                                    {isPositive && '+'}
                                    {formatCkbAmount(activity.ckbDelta).full} CKB
                                  </span>
                                </div>
                                <div className="flex w-32 min-w-0 flex-wrap items-center justify-end gap-1 lg:w-48">
                                  {activity.assetChanges.map((change, i) => (
                                    <AssetChangeBadge key={i} change={change} />
                                  ))}
                                </div>
                                <div className="w-20 text-right text-sm text-slate-500">
                                  {formatTimeAgo(Number(activity.timestamp))}
                                </div>
                              </div>
                            </TerminalRow>
                          );
                        })}
                      </div>
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
                  <div className="py-12 text-center text-slate-500">
                    {activityFilter === 'all'
                      ? 'No activities'
                      : `No ${activityFilter === 'ckb' ? 'CKB' : activityFilter === 'nft' ? 'NFT/DOB' : activityFilter.toUpperCase()} activities on this page`}
                  </div>
                )}
              </>
            )}

            {activeTab === 'cells' && (
              <>
                <div className="flex items-center gap-1.5 border-b border-slate-800 px-4 py-2">
                  {(['all', 'ckb', 'token', 'dao'] as const).map((f) => (
                    <button
                      key={f}
                      onClick={() => setCellFilter(f)}
                      className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                        cellFilter === f
                          ? 'bg-terminal-green/15 text-terminal-green'
                          : 'text-slate-500 hover:text-slate-300'
                      }`}
                    >
                      {{ all: 'All', ckb: 'CKB', token: 'Token', dao: 'DAO' }[f]}
                    </button>
                  ))}
                </div>
                <div className="p-4">
                  {cellsLoading ? (
                    <div className="py-12 text-center text-slate-500">Loading cells...</div>
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
                                    className="text-terminal-green hover:underline"
                                  >
                                    <HexDisplay
                                      value={`${cell.txHash}:${cell.outputIndex}`}
                                      truncate
                                      startChars={6}
                                      endChars={6}
                                      size="sm"
                                      color="accent"
                                    />
                                  </Link>
                                  <span className="font-mono text-xs text-slate-500">
                                    #{cell.createdAtBlock.toLocaleString()}
                                  </span>
                                </div>

                                <div className="mb-2 rounded border border-slate-800 bg-slate-900/50 p-2">
                                  <Capacity value={cell.capacity} className="text-lg text-white" />
                                </div>

                                {cellIsDao && (
                                  <div className="rounded border border-slate-800 bg-slate-900/50 px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="font-mono text-slate-300">Nervos DAO</span>
                                      {daoDepositInfo && (
                                        <Badge
                                          variant={
                                            daoDepositInfo.status === 'deposited'
                                              ? 'green'
                                              : daoDepositInfo.status === 'withdrawing'
                                                ? 'amber'
                                                : 'gray'
                                          }
                                        >
                                          {daoDepositInfo.status === 'deposited'
                                            ? 'Active'
                                            : daoDepositInfo.status === 'withdrawing'
                                              ? 'Withdrawing'
                                              : 'Completed'}
                                        </Badge>
                                      )}
                                    </div>
                                    {daoDepositBlock !== null && daoDepositBlock > 0 && (
                                      <div className="mt-1 text-xs text-slate-500">
                                        Deposit Block: #{daoDepositBlock.toLocaleString()}
                                      </div>
                                    )}
                                    {daoDepositInfo?.compensation && (
                                      <div className="mt-1 text-xs text-green-400">
                                        +{formatCkbAmount(daoDepositInfo.compensation).full} CKB
                                        compensation
                                      </div>
                                    )}
                                  </div>
                                )}

                                {cellToken && cell.udtAmount && (
                                  <div className="rounded border border-amber-900/30 bg-amber-900/10 px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="text-amber-dim font-mono">
                                        {formatTokenBalance(cell.udtAmount, cellToken.decimals)}
                                      </span>
                                      <span className="text-amber text-xs">
                                        {tokenDisplayName(cellToken)}
                                      </span>
                                    </div>
                                  </div>
                                )}

                                {!cellToken && !cellIsDao && cell.udtAmount && (
                                  <div className="rounded border border-slate-700 bg-slate-800/50 px-2 py-1.5">
                                    <div className="flex items-center justify-between text-sm">
                                      <span className="font-mono text-slate-400">
                                        {formatTokenBalance(cell.udtAmount, 0)}
                                      </span>
                                      {cell.typeScriptHash ? (
                                        <Link
                                          href={`/tokens/${cell.typeScriptHash}`}
                                          className="text-xs text-slate-500 hover:text-slate-300 hover:underline"
                                        >
                                          {shortHash(cell.typeScriptHash)}
                                        </Link>
                                      ) : (
                                        <span className="text-xs text-slate-500">Token</span>
                                      )}
                                    </div>
                                  </div>
                                )}

                                {!cellToken &&
                                  !cellIsDao &&
                                  !cell.udtAmount &&
                                  cell.dataSize > 0 && (
                                    <div className="rounded border border-slate-700 bg-slate-800/30 px-2 py-1">
                                      <span className="font-mono text-xs text-slate-500">
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
                    <div className="py-12 text-center text-slate-500">
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
                  <div className="py-12 text-center text-slate-500">Loading transactions...</div>
                ) : transactions?.data && transactions.data.length > 0 ? (
                  <>
                    <div className="overflow-x-auto">
                      <div className="min-w-[700px]">
                        <div className="flex items-center gap-4 border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
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
                              <div className="flex w-full items-center gap-4">
                                <div className="min-w-0 flex-1">
                                  <div className="flex flex-wrap items-center gap-1.5">
                                    <Link href={`/tx/${tx.txHash}`}>
                                      <HexDisplay
                                        value={tx.txHash}
                                        truncate
                                        startChars={6}
                                        endChars={6}
                                        color="accent"
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
                                    className="block font-mono text-xs text-slate-500 hover:text-slate-300"
                                  >
                                    #{tx.blockNumber.toLocaleString()}
                                  </Link>
                                </div>
                                <div className="w-20 text-center font-mono text-slate-400">
                                  <span className="text-terminal-dim">{tx.inputsCount}</span>
                                  <span className="mx-1 text-slate-600">→</span>
                                  <span className="text-terminal-dim">{tx.outputsCount}</span>
                                </div>
                                <div className="w-32 whitespace-nowrap text-right">
                                  {tx.isCellbase ? (
                                    <span className="text-slate-600">—</span>
                                  ) : (
                                    <div>
                                      <Capacity value={tx.fee} className="text-slate-400" />
                                      {feeRate != null && (
                                        <div className="font-mono text-xs text-slate-600">
                                          {feeRate.toFixed(1)} shannons/B
                                        </div>
                                      )}
                                    </div>
                                  )}
                                </div>
                                <div className="hidden w-28 whitespace-nowrap text-right font-mono text-xs text-slate-500 xl:block">
                                  {tx.txSize != null ? (
                                    <span>
                                      {tx.txSize >= 1000
                                        ? `${(tx.txSize / 1000).toFixed(1)} kB`
                                        : `${tx.txSize} B`}
                                    </span>
                                  ) : (
                                    <span className="text-slate-600">—</span>
                                  )}
                                  {tx.cycles != null && (
                                    <>
                                      <span className="mx-1 text-slate-700">/</span>
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
                                    className={isPositive ? 'text-green-400' : 'text-red-400'}
                                    showSign
                                  />
                                </div>
                                <div className="w-20 text-right text-sm text-slate-500">
                                  {formatTimeAgo(tx.timestamp)}
                                </div>
                              </div>
                            </TerminalRow>
                          );
                        })}
                      </div>
                    </div>
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
                  <div className="py-12 text-center text-slate-500">No transactions</div>
                )}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
