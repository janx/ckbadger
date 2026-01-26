'use client';

import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
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
import { api, type AddressToken, type AssetTransfer, type DaoDeposit } from '@/lib/api';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';

function groupAssetTransfersByTx(transfers: AssetTransfer[]): Map<string, AssetTransfer[]> {
  const map = new Map<string, AssetTransfer[]>();
  for (const t of transfers) {
    const existing = map.get(t.txHash) || [];
    existing.push(t);
    map.set(t.txHash, existing);
  }
  return map;
}

export default function AddressDetailPage() {
  const params = useParams();
  const addr = params.addr as string;

  const [selectedToken, setSelectedToken] = useState<AddressToken | null>(null);
  const [selectedDao, setSelectedDao] = useState(false);
  const [activeTab, setActiveTab] = useState<'cells' | 'transactions' | 'dao'>('cells');

  const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';

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

  const { data: cells } = useQuery({
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
  });

  const { data: transactions } = useQuery({
    queryKey: ['address-transactions', address?.lockScriptHash, txPagination.cursor],
    queryFn: () =>
      api.getAddressTransactions(address!.lockScriptHash, {
        limit: 20,
        cursor: txPagination.cursor,
      }),
    enabled: !!address,
  });

  const { data: assetTransfers } = useQuery({
    queryKey: ['address-asset-transfers', address?.lockScriptHash, txPagination.cursor],
    queryFn: () =>
      api.getAddressAssetTransfers(address!.lockScriptHash, {
        limit: 100,
        cursor: txPagination.cursor,
      }),
    enabled: !!address && activeTab === 'transactions',
  });

  const assetTransfersByTx = useMemo(
    () => groupAssetTransfersByTx(assetTransfers?.data || []),
    [assetTransfers?.data]
  );

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

  const handleDaoSelect = () => {
    setSelectedDao(true);
    setSelectedToken(null);
    cellsPagination.reset();
    setActiveTab('cells');
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

  const formatTokenBalance = (balance: string, decimals: number): string => {
    if (decimals === 0) return BigInt(balance).toLocaleString();
    const balanceBigInt = BigInt(balance);
    const divisor = BigInt(10 ** decimals);
    const wholePart = balanceBigInt / divisor;
    const fractionalPart = balanceBigInt % divisor;
    const fractionalStr = fractionalPart.toString().padStart(decimals, '0');
    const trimmedFractional = fractionalStr.replace(/0+$/, '');
    if (trimmedFractional === '') {
      return wholePart.toLocaleString();
    }
    return `${wholePart.toLocaleString()}.${trimmedFractional}`;
  };

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

  const formatAssetAmount = (transfer: AssetTransfer): string => {
    if (!transfer.amount) return '1';
    const decimals = transfer.tokenDecimals ?? 0;
    if (decimals === 0) return BigInt(transfer.amount).toLocaleString();
    const balanceBigInt = BigInt(transfer.amount);
    const divisor = BigInt(10 ** decimals);
    const wholePart = balanceBigInt / divisor;
    const fractionalPart = balanceBigInt % divisor;
    const fractionalStr = fractionalPart.toString().padStart(decimals, '0');
    const trimmedFractional = fractionalStr.replace(/0+$/, '');
    if (trimmedFractional === '') return wholePart.toLocaleString();
    return `${wholePart.toLocaleString()}.${trimmedFractional}`;
  };

  const getAssetLabel = (transfer: AssetTransfer): string => {
    if (transfer.tokenSymbol) return transfer.tokenSymbol;
    if (transfer.tokenName) return transfer.tokenName;
    switch (transfer.assetType) {
      case 'spore':
        return 'Spore';
      case 'dob/0':
      case 'dob/1':
        return 'DOB';
      case 'mnft':
        return 'M-NFT';
      case 'dotbit':
        return '.bit';
      case 'dao':
        return 'DAO';
      default:
        return transfer.assetType.toUpperCase();
    }
  };

  const getAssetBadgeVariant = (
    category: string
  ): 'green' | 'amber' | 'red' | 'gray' | 'purple' => {
    switch (category) {
      case 'token':
        return 'amber';
      case 'dob':
        return 'purple';
      case 'nft':
        return 'green';
      case 'dao':
        return 'gray';
      default:
        return 'gray';
    }
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
                  className="bg-terminal-green/20 text-terminal-green hover:bg-terminal-green/30 inline-flex items-center gap-1.5 rounded border border-transparent px-2 py-0.5 text-xs font-medium transition-colors"
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
            <StatGrid columns={3}>
              <StatBlock
                label="Balance"
                value={formatCkbAmount(address.balance).full}
                suffix=" CKB"
                color="green"
              />
              <StatBlock label="Live Cells" value={address.liveCellsCount} color="amber" />
              <StatBlock label="Transactions" value={address.transactionsCount} color="white" />
            </StatGrid>
          </TerminalPanelContent>
        </TerminalPanel>

        {((tokens?.data && tokens.data.length > 0) || daoSummary?.hasDaoActivity) && (
          <TerminalPanel className="mb-8" variant="elevated">
            <TerminalPanelHeader>
              Asset Holdings ({(tokens?.total || 0) + (daoSummary?.hasDaoActivity ? 1 : 0)})
            </TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="min-w-full">
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="flex-1">Asset</div>
                  <div className="w-32">Standard</div>
                  <div className="w-48 text-right">Balance</div>
                </div>

                {daoSummary?.hasDaoActivity && (
                  <TerminalRow className={`cursor-pointer ${selectedDao ? 'bg-slate-800/80' : ''}`}>
                    <div
                      className="flex w-full items-center"
                      onClick={() => (selectedDao ? handleDaoSelect() : handleDaoSelect())}
                    >
                      <div className="flex flex-1 items-center gap-3">
                        <div className="flex h-6 w-6 items-center justify-center rounded-full bg-purple-900/50 text-xs text-purple-400">
                          D
                        </div>
                        <div>
                          <span className="text-terminal-green font-medium">Nervos DAO</span>
                          {daoSummary.estimatedApc && (
                            <span className="ml-2 text-xs text-green-400">
                              {daoSummary.estimatedApc}% APC
                            </span>
                          )}
                          <div className="text-xs text-slate-500">
                            {daoSummary.activeDepositsCount} active
                            {daoSummary.pendingWithdrawalsCount > 0 &&
                              ` · ${daoSummary.pendingWithdrawalsCount} pending`}
                            {daoSummary.unclaimedCompensation !== '0' &&
                              ` · +${formatCkbAmount(daoSummary.unclaimedCompensation).full} unclaimed`}
                          </div>
                        </div>
                      </div>
                      <div className="w-32">
                        <Badge variant="purple">DAO</Badge>
                      </div>
                      <div className="w-48 text-right">
                        <span className="font-mono text-white">
                          {formatCkbAmount(daoSummary.totalLockedCapacity).full}
                        </span>
                        <span className="ml-2 text-xs text-slate-500">CKB</span>
                      </div>
                    </div>
                  </TerminalRow>
                )}

                {tokens?.data?.map((token) => {
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
                          {token.iconUrl ? (
                            <img
                              src={token.iconUrl}
                              alt={token.symbol || token.name || 'Token'}
                              className="h-6 w-6 rounded-full"
                              onError={(e) => {
                                (e.target as HTMLImageElement).style.display = 'none';
                              }}
                            />
                          ) : (
                            <div className="flex h-6 w-6 items-center justify-center rounded-full bg-slate-800 text-xs text-slate-400">
                              {token.symbol?.[0] || token.name?.[0] || '?'}
                            </div>
                          )}
                          <div>
                            <Link
                              href={`/tokens/${token.typeScriptHash}`}
                              onClick={(e) => e.stopPropagation()}
                              className="text-terminal-green font-medium hover:underline"
                            >
                              {token.name || token.symbol || 'Unknown Token'}
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
                          {token.symbol && (
                            <span className="ml-2 text-xs text-slate-500">{token.symbol}</span>
                          )}
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
                  onClick={() => setActiveTab('cells')}
                  className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                    activeTab === 'cells'
                      ? 'bg-terminal-green/20 text-terminal-green'
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
                      ? 'bg-terminal-green/20 text-terminal-green'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  Transactions
                  <span className="ml-1 opacity-75">({address.transactionsCount})</span>
                </button>
                {daoSummary?.hasDaoActivity && (
                  <button
                    onClick={() => setActiveTab('dao')}
                    className={`rounded px-3 py-1 font-mono text-sm transition-colors ${
                      activeTab === 'dao'
                        ? 'bg-terminal-green/20 text-terminal-green'
                        : 'text-slate-400 hover:text-white'
                    }`}
                  >
                    DAO Activities
                    <span className="ml-1 opacity-75">
                      (
                      {(daoSummary.activeDepositsCount || 0) +
                        (daoSummary.pendingWithdrawalsCount || 0) +
                        (daoSummary.completedWithdrawalsCount || 0)}
                      )
                    </span>
                  </button>
                )}
              </div>
            }
          >
            {activeTab === 'cells' ? (
              <div className="flex items-center gap-2">
                <span>Cells</span>
                {selectedToken && (
                  <Badge variant="amber" className="ml-2">
                    Filter: {selectedToken.symbol || selectedToken.name}
                  </Badge>
                )}
                {selectedDao && (
                  <Badge variant="purple" className="ml-2">
                    Filter: Nervos DAO
                  </Badge>
                )}
              </div>
            ) : activeTab === 'dao' ? (
              'DAO Activities'
            ) : (
              'History'
            )}
          </TerminalPanelHeader>

          {selectedToken && activeTab === 'cells' && (
            <div className="flex items-center justify-between border-b border-slate-800 bg-slate-900/50 px-4 py-2">
              <span className="text-sm text-slate-400">
                Showing cells for{' '}
                <span className="text-amber">{selectedToken.symbol || selectedToken.name}</span>
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
                Showing cells for <span className="text-purple-400">Nervos DAO</span>
              </span>
              <button
                onClick={() => setSelectedDao(false)}
                className="text-xs text-red-400 hover:underline"
              >
                Clear Filter
              </button>
            </div>
          )}

          <TerminalPanelContent padding={activeTab === 'cells' ? 'md' : 'none'}>
            {activeTab === 'cells' && (
              <>
                {cells?.data && cells.data.length > 0 ? (
                  <>
                    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
                      {cells.data.map((cell) => {
                        const cellToken = cell.typeScriptHash
                          ? tokenMap.get(cell.typeScriptHash)
                          : undefined;
                        const cellIsDao = isDaoCell(cell);
                        const daoDepositBlock = cellIsDao ? parseDaoCellData(cell.data) : null;
                        const daoDepositInfo = cellIsDao
                          ? daoDeposits?.data?.find(
                              (d) => d.txHash === cell.txHash && d.outputIndex === cell.outputIndex
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
                                <div className="rounded border border-purple-900/30 bg-purple-900/10 px-2 py-1.5">
                                  <div className="flex items-center justify-between text-sm">
                                    <span className="font-mono text-purple-400">Nervos DAO</span>
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
                                      {cellToken.symbol || cellToken.name || 'Unknown Token'}
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
                                    <span className="text-xs text-slate-500">Unknown Token</span>
                                  </div>
                                </div>
                              )}

                              {!cellToken && !cellIsDao && !cell.udtAmount && cell.dataSize > 0 && (
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
                    {(cells.hasMore || cellsPagination.hasPrevious) && (
                      <TerminalPanelFooter className="mt-4 flex justify-center border-t-0">
                        <CursorPagination
                          hasMore={cells.hasMore}
                          hasPrevious={cellsPagination.hasPrevious}
                          onNext={() => cellsPagination.goToNext(cells.nextCursor)}
                          onPrevious={cellsPagination.goToPrevious}
                        />
                      </TerminalPanelFooter>
                    )}
                  </>
                ) : (
                  <div className="py-12 text-center text-slate-500">
                    {selectedToken
                      ? `No cells found for ${selectedToken.symbol || selectedToken.name}`
                      : 'No live cells'}
                  </div>
                )}
              </>
            )}

            {activeTab === 'transactions' && (
              <>
                {transactions?.data && transactions.data.length > 0 ? (
                  <>
                    <div className="min-w-full overflow-x-auto">
                      <div className="flex gap-4 border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                        <div className="w-36 shrink-0">Tx Hash</div>
                        <div className="w-28 shrink-0">Block</div>
                        <div className="w-28 shrink-0">Type</div>
                        <div className="w-40 shrink-0 text-right">CKB Change</div>
                        <div className="w-32 shrink-0">Assets</div>
                        <div className="w-24 shrink-0 text-right">Time</div>
                      </div>
                      {transactions.data.map((tx) => {
                        const isPositive = !tx.capacityChange.startsWith('-');
                        const txAssets = assetTransfersByTx.get(tx.txHash) || [];
                        const hasDaoActivity =
                          txAssets.some((a) => a.assetCategory === 'dao') ||
                          daoTxHashes.has(tx.txHash);
                        return (
                          <TerminalRow key={tx.txHash}>
                            <div className="flex w-full items-center gap-4">
                              <div className="w-36 shrink-0">
                                <Link href={`/tx/${tx.txHash}`}>
                                  <HexDisplay
                                    value={tx.txHash}
                                    truncate
                                    startChars={6}
                                    endChars={6}
                                    className="text-terminal-green"
                                  />
                                </Link>
                              </div>
                              <div className="w-28 shrink-0">
                                <Link
                                  href={`/blocks/${tx.blockNumber}`}
                                  className="font-mono text-sm text-slate-400 hover:text-white"
                                >
                                  #{tx.blockNumber.toLocaleString()}
                                </Link>
                              </div>
                              <div className="flex w-28 shrink-0 items-center gap-1">
                                {getTxTypeBadge(tx.txType)}
                                {hasDaoActivity && <Badge variant="purple">DAO</Badge>}
                              </div>
                              <div className="w-40 shrink-0 text-right">
                                <Capacity
                                  value={tx.capacityChange}
                                  className={isPositive ? 'text-green-400' : 'text-red-400'}
                                  showSign
                                />
                              </div>
                              <div className="flex w-32 shrink-0 flex-wrap gap-1">
                                {txAssets.length > 0 ? (
                                  txAssets.slice(0, 3).map((asset, idx) => (
                                    <span
                                      key={idx}
                                      className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-mono text-xs ${
                                        asset.direction === 'in'
                                          ? 'bg-green-900/30 text-green-400'
                                          : 'bg-red-900/30 text-red-400'
                                      }`}
                                    >
                                      <span>{asset.direction === 'in' ? '+' : '-'}</span>
                                      <span>{formatAssetAmount(asset)}</span>
                                      <Badge variant={getAssetBadgeVariant(asset.assetCategory)}>
                                        {getAssetLabel(asset)}
                                      </Badge>
                                    </span>
                                  ))
                                ) : (
                                  <span className="text-xs text-slate-600">-</span>
                                )}
                                {txAssets.length > 3 && (
                                  <span className="text-xs text-slate-500">
                                    +{txAssets.length - 3} more
                                  </span>
                                )}
                              </div>
                              <div className="w-24 shrink-0 text-right text-sm text-slate-500">
                                {formatTimeAgo(tx.timestamp)}
                              </div>
                            </div>
                          </TerminalRow>
                        );
                      })}
                    </div>
                    {(transactions.hasMore || txPagination.hasPrevious) && (
                      <TerminalPanelFooter className="flex justify-center">
                        <CursorPagination
                          total={transactions.total}
                          totalLabel="transactions"
                          hasMore={transactions.hasMore}
                          hasPrevious={txPagination.hasPrevious}
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

            {activeTab === 'dao' && (
              <>
                {daoDeposits?.data && daoDeposits.data.length > 0 ? (
                  <>
                    <div className="min-w-full overflow-x-auto">
                      <div className="flex gap-4 border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                        <div className="w-36 shrink-0">DAO Tx</div>
                        <div className="w-28 shrink-0">Block</div>
                        <div className="w-24 shrink-0">Status</div>
                        <div className="w-36 shrink-0 text-right">Capacity</div>
                        <div className="w-40 shrink-0 text-right">Compensation</div>
                        <div className="w-20 shrink-0">Duration</div>
                        <div className="w-24 shrink-0 text-right">Deposited</div>
                      </div>
                      {daoDeposits.data.map((deposit: DaoDeposit) => (
                        <TerminalRow key={`${deposit.txHash}-${deposit.outputIndex}`}>
                          <div className="flex w-full items-center gap-4">
                            <div className="w-36 shrink-0">
                              <Link href={`/tx/${deposit.txHash}`}>
                                <HexDisplay
                                  value={deposit.txHash}
                                  truncate
                                  startChars={6}
                                  endChars={6}
                                  className="text-terminal-green"
                                />
                              </Link>
                            </div>
                            <div className="w-28 shrink-0">
                              <Link
                                href={`/blocks/${deposit.depositBlockNumber}`}
                                className="font-mono text-sm text-slate-400 hover:text-white"
                              >
                                #{deposit.depositBlockNumber.toLocaleString()}
                              </Link>
                            </div>
                            <div className="w-24 shrink-0">{getDaoStatusBadge(deposit.status)}</div>
                            <div className="w-36 shrink-0 text-right">
                              <Capacity value={deposit.capacity} className="text-white" />
                            </div>
                            <div className="w-40 shrink-0 text-right">
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
                            <div className="w-20 shrink-0 font-mono text-sm text-slate-400">
                              {formatDaoDuration(
                                deposit.depositTimestamp,
                                deposit.withdrawTimestamp || deposit.withdrawRequestTimestamp
                              )}
                            </div>
                            <div className="w-24 shrink-0 text-right text-sm text-slate-500">
                              {formatTimeAgo(deposit.depositTimestamp)}
                            </div>
                          </div>
                        </TerminalRow>
                      ))}
                    </div>
                    {(daoDeposits.hasMore || daoPagination.hasPrevious) && (
                      <TerminalPanelFooter className="flex justify-center">
                        <CursorPagination
                          total={daoDeposits.total}
                          totalLabel="deposits"
                          hasMore={daoDeposits.hasMore}
                          hasPrevious={daoPagination.hasPrevious}
                          onNext={() => daoPagination.goToNext(daoDeposits.nextCursor)}
                          onPrevious={daoPagination.goToPrevious}
                        />
                      </TerminalPanelFooter>
                    )}
                  </>
                ) : (
                  <div className="py-12 text-center text-slate-500">No DAO deposits</div>
                )}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
