'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams } from 'next/navigation';
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
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { api, TokenHolder, TokenTransfer } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';

export default function TokenDetailPage() {
  const params = useParams();
  const typeHash = params.typeHash as string;

  const [holdersCursor, setHoldersCursor] = useState<string | undefined>(undefined);
  const [holdersCursorHistory, setHoldersCursorHistory] = useState<string[]>([]);

  const [transfersCursor, setTransfersCursor] = useState<string | undefined>(undefined);
  const [transfersCursorHistory, setTransfersCursorHistory] = useState<string[]>([]);

  const {
    data: token,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['token', typeHash],
    queryFn: () => api.getToken(typeHash),
  });

  const { data: holders } = useQuery({
    queryKey: ['token-holders', typeHash, holdersCursor],
    queryFn: () => api.getTokenHolders(typeHash, { limit: 20, cursor: holdersCursor }),
    enabled: !!token,
  });

  const { data: transfers } = useQuery({
    queryKey: ['token-transfers', typeHash, transfersCursor],
    queryFn: () => api.getTokenTransfers(typeHash, { limit: 20, cursor: transfersCursor }),
    enabled: !!token,
  });

  const handleHoldersNextPage = () => {
    if (holders?.nextCursor) {
      setHoldersCursorHistory((prev) => [...prev, holdersCursor || '']);
      setHoldersCursor(holders.nextCursor);
    }
  };

  const handleHoldersPrevPage = () => {
    if (holdersCursorHistory.length > 0) {
      const prev = holdersCursorHistory[holdersCursorHistory.length - 1];
      setHoldersCursorHistory((h) => h.slice(0, -1));
      setHoldersCursor(prev || undefined);
    }
  };

  const handleTransfersNextPage = () => {
    if (transfers?.nextCursor) {
      setTransfersCursorHistory((prev) => [...prev, transfersCursor || '']);
      setTransfersCursor(transfers.nextCursor);
    }
  };

  const handleTransfersPrevPage = () => {
    if (transfersCursorHistory.length > 0) {
      const prev = transfersCursorHistory[transfersCursorHistory.length - 1];
      setTransfersCursorHistory((h) => h.slice(0, -1));
      setTransfersCursor(prev || undefined);
    }
  };

  const formatNumber = (num: number | string) => {
    return new Intl.NumberFormat().format(Number(num));
  };

  const formatAmount = (amount: string, decimals: number) => {
    const num = BigInt(amount);
    const divisor = BigInt(10 ** decimals);
    const whole = num / divisor;
    const remainder = num % divisor;
    if (remainder === BigInt(0)) {
      return formatNumber(whole.toString());
    }
    const decimal = remainder.toString().padStart(decimals, '0').slice(0, 4);
    return `${formatNumber(whole.toString())}.${decimal}`;
  };

  if (isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="h-20 w-full rounded bg-slate-900" />
            <div className="grid gap-6 lg:grid-cols-2">
              <div className="h-48 rounded bg-slate-900" />
              <div className="h-48 rounded bg-slate-900" />
            </div>
            <div className="h-96 rounded bg-slate-900" />
          </div>
        </main>
      </div>
    );
  }

  if (error || !token) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Token not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title={token.symbol || token.name || 'Unknown Token'}
          subtitle={
            <div className="flex items-center gap-2">
              <HexDisplay value={token.typeScriptHash} truncate color="white" size="sm" />
            </div>
          }
          badge={
            <div className="flex items-center gap-2">
              <Badge variant={token.standard === 'xudt' ? 'purple' : 'blue'}>
                {token.standard.toUpperCase()}
              </Badge>
              {token.published && (
                <span className="text-terminal-green" title="Verified">
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
                <span className="text-amber" title="Famous">
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

        <div className="mb-6 grid gap-6 lg:grid-cols-2">
          <TerminalPanel glow>
            <TerminalPanelHeader indicator="active">Overview</TerminalPanelHeader>
            <TerminalPanelContent>
              <StatGrid columns={2}>
                <StatBlock label="Holders" value={token.holdersCount} color="green" />
                <StatBlock label="Transfers" value={token.transfersCount} color="amber" />
                <StatBlock label="Decimals" value={token.decimals} color="white" />
                <StatBlock
                  label="Total Supply"
                  value={formatAmount(token.totalSupply, token.decimals)}
                  color="green"
                />
              </StatGrid>
              {token.description && (
                <div className="mt-4 border-t border-slate-800 pt-4">
                  <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                    Description
                  </div>
                  <div className="mt-1 text-sm text-slate-300">{token.description}</div>
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
                        className="text-terminal-green hover:underline"
                      >
                        {token.operatorWebsite}
                      </a>
                    </DataField>
                  )}
                  {token.manager && (
                    <DataField label="Manager">
                      <Link
                        href={`/address/${token.manager}`}
                        className="text-terminal-green font-mono text-sm hover:underline"
                      >
                        {token.manager.length > 40
                          ? `${token.manager.slice(0, 20)}...${token.manager.slice(-20)}`
                          : token.manager}
                      </Link>
                    </DataField>
                  )}
                  {token.email && (
                    <DataField label="Contact">
                      <a
                        href={`mailto:${token.email}`}
                        className="text-terminal-green hover:underline"
                      >
                        {token.email}
                      </a>
                    </DataField>
                  )}
                  {token.udtType && (
                    <DataField label="UDT Type">
                      <span className="text-white">{token.udtType}</span>
                    </DataField>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          )}
        </div>

        <TerminalPanel>
          <Tabs defaultValue="holders">
            <TerminalPanelHeader
              actions={
                <TabsList>
                  <TabsTrigger value="holders">
                    Holders ({formatNumber(token.holdersCount)})
                  </TabsTrigger>
                  <TabsTrigger value="transfers">
                    Transfers ({formatNumber(token.transfersCount)})
                  </TabsTrigger>
                </TabsList>
              }
            >
              Data
            </TerminalPanelHeader>

            <TabsContent value="holders">
              <TerminalPanelContent padding="none">
                {holders?.data?.length ? (
                  <>
                    <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
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
                                  color="green"
                                  className="hover:underline"
                                />
                              </Link>
                            )}
                          </div>
                          <div className="w-48 text-right font-mono text-white">
                            {formatAmount(holder.balance, token.decimals)}
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                  </>
                ) : (
                  <div className="py-8 text-center text-slate-500">No holders</div>
                )}
              </TerminalPanelContent>
              {holders?.data?.length ? (
                <TerminalPanelFooter className="flex items-center justify-between">
                  <span className="text-sm text-slate-500">
                    Total: {formatNumber(holders.total)} holders
                  </span>
                  <div className="flex gap-2">
                    <button
                      onClick={handleHoldersPrevPage}
                      disabled={holdersCursorHistory.length === 0}
                      className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Previous
                    </button>
                    <button
                      onClick={handleHoldersNextPage}
                      disabled={!holders.hasMore}
                      className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Next
                    </button>
                  </div>
                </TerminalPanelFooter>
              ) : null}
            </TabsContent>

            <TabsContent value="transfers">
              <TerminalPanelContent padding="none">
                {transfers?.data?.length ? (
                  <>
                    <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                      <div className="w-32">Tx Hash</div>
                      <div className="flex-1">From</div>
                      <div className="flex-1">To</div>
                      <div className="w-32 text-right">Amount</div>
                      <div className="w-24 text-right">Time</div>
                    </div>
                    {transfers.data.map((transfer: TokenTransfer, idx: number) => (
                      <TerminalRow key={`${transfer.txHash}-${idx}`}>
                        <div className="flex items-center">
                          <div className="w-32">
                            <Link href={`/tx/${transfer.txHash}`}>
                              <HexDisplay
                                value={transfer.txHash}
                                color="amber"
                                startChars={8}
                                endChars={6}
                                className="hover:underline"
                              />
                            </Link>
                          </div>
                          <div className="flex-1">
                            {transfer.isMint ? (
                              <Badge variant="green">Mint</Badge>
                            ) : transfer.fromAddress ? (
                              <Address address={transfer.fromAddress} className="text-slate-400" />
                            ) : transfer.fromLockHash ? (
                              <Link href={`/address/${transfer.fromLockHash}`}>
                                <HexDisplay
                                  value={transfer.fromLockHash}
                                  color="white"
                                  startChars={8}
                                  endChars={6}
                                  className="hover:text-terminal-green"
                                />
                              </Link>
                            ) : (
                              <span className="text-slate-600">-</span>
                            )}
                          </div>
                          <div className="flex-1">
                            {transfer.isBurn ? (
                              <Badge variant="red">Burn</Badge>
                            ) : transfer.toAddress ? (
                              <Address address={transfer.toAddress} className="text-slate-400" />
                            ) : (
                              <Link href={`/address/${transfer.toLockHash}`}>
                                <HexDisplay
                                  value={transfer.toLockHash}
                                  color="white"
                                  startChars={8}
                                  endChars={6}
                                  className="hover:text-terminal-green"
                                />
                              </Link>
                            )}
                          </div>
                          <div className="w-32 text-right font-mono text-white">
                            {formatAmount(transfer.amount, token.decimals)}
                          </div>
                          <div className="w-24 text-right text-slate-500">
                            {formatTimeAgo(transfer.timestamp)}
                          </div>
                        </div>
                      </TerminalRow>
                    ))}
                  </>
                ) : (
                  <div className="py-8 text-center text-slate-500">No transfers</div>
                )}
              </TerminalPanelContent>
              {transfers?.data?.length ? (
                <TerminalPanelFooter className="flex items-center justify-between">
                  <span className="text-sm text-slate-500">
                    Total: {formatNumber(transfers.total)} transfers
                  </span>
                  <div className="flex gap-2">
                    <button
                      onClick={handleTransfersPrevPage}
                      disabled={transfersCursorHistory.length === 0}
                      className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Previous
                    </button>
                    <button
                      onClick={handleTransfersNextPage}
                      disabled={!transfers.hasMore}
                      className="hover:border-terminal-dark hover:text-terminal-green rounded border border-slate-700 bg-slate-800 px-4 py-1.5 font-mono text-sm text-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Next
                    </button>
                  </div>
                </TerminalPanelFooter>
              ) : null}
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
