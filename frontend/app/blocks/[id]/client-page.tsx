'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useParams, useRouter } from '@/src/navigation';
import { useEffect } from 'react';
import { Header } from '@/components/layout/header';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { UsageBar, ProgressBar } from '@/components/ui/progress-bar';
import { api } from '@/lib/api';
import { isEditableElement } from '@/lib/search-focus';
import { formatCkbAmount } from '@/lib/utils';

const BLOCK_MAX_SIZE = 597_000;
const BLOCK_MAX_CYCLES = 3_500_000_000;

function getOrdinalSuffix(n: number): string {
  const s = ['th', 'st', 'nd', 'rd'];
  const v = n % 100;
  return s[(v - 20) % 10] || s[v] || s[0];
}

function FormatReward({ reward }: { reward: string | null }) {
  if (!reward) return null;
  const { integer, decimal } = formatCkbAmount(reward);
  return (
    <span className="font-mono tabular-nums">
      {integer}
      <span className="text-text-muted text-[0.85em]">.{decimal}</span>
      <span className="text-text-secondary ml-1 text-[0.85em]">CKB</span>
    </span>
  );
}

function formatTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  return date.toLocaleString();
}

export default function BlockDetailPage() {
  const params = useParams();
  const router = useRouter();
  const id = params.id as string;

  const {
    data: block,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['block', id],
    queryFn: () => api.getBlock(id),
  });

  const { data: txs } = useQuery({
    queryKey: ['block-transactions', id, block?.number],
    queryFn: () => api.getTransactions({ blockNumber: block!.number, limit: 100 }),
    enabled: !!block,
  });

  const { data: feeStats } = useQuery({
    queryKey: ['block-fee-stats', id],
    queryFn: () => api.getBlockFeeStats(id),
    enabled: !!block,
  });

  const { data: proposals } = useQuery({
    queryKey: ['block-proposals', id],
    queryFn: () => api.getBlockProposals(id),
    enabled: !!block && block.proposalsCount > 0,
  });

  const { data: hardforkTimeline } = useQuery({
    queryKey: ['hardforks-for-block', block?.hardforkActivation?.id],
    queryFn: () => api.getHardforks(),
    enabled:
      !!block?.hardforkActivation &&
      (!block.hardforkActivation.resources || block.hardforkActivation.resources.length === 0),
    staleTime: 60_000,
  });

  useEffect(() => {
    if (!block) return;

    const handleWindowKeyDown = (event: KeyboardEvent) => {
      if (
        isEditableElement(event.target) ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey
      ) {
        return;
      }

      const key = event.key.toLowerCase();
      if (key === 'arrowleft' || key === 'h') {
        if (block.number > 0) {
          event.preventDefault();
          router.push(`/blocks/${block.number - 1}`);
        }
        return;
      }

      if (key === 'arrowright' || key === 'l') {
        event.preventDefault();
        router.push(`/blocks/${block.number + 1}`);
      }
    };

    window.addEventListener('keydown', handleWindowKeyDown);
    return () => window.removeEventListener('keydown', handleWindowKeyDown);
  }, [block, router]);

  if (isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <div className="animate-pulse space-y-4">
            <div className="bg-base-elevated h-8 w-48 rounded" />
            <div className="bg-base-elevated h-64 rounded" />
          </div>
        </main>
      </div>
    );
  }

  if (error || !block) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-4">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-muted text-xl">Block not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const epochStartNumber = block.number - block.epochIndex;
  const ordinalSuffix = getOrdinalSuffix(block.epochIndex + 1);
  const activationHardfork = block.hardforkActivation;
  const activationResources =
    activationHardfork?.resources && activationHardfork.resources.length > 0
      ? activationHardfork.resources
      : (hardforkTimeline?.events.find((event) => event.id === activationHardfork?.id)?.resources ??
        []);

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-5xl px-4 py-4">
        <PageHeader
          title={`Block #${block.number.toLocaleString()}`}
          hash={block.hash}
          badge={
            activationHardfork ? (
              <Badge variant="amber">
                HARDFORK ACTIVATION · {activationHardfork.shortName.toUpperCase()}
              </Badge>
            ) : undefined
          }
          navigation={{
            prev: { href: `/blocks/${block.number - 1}`, label: 'Previous Block' },
            next: { href: `/blocks/${block.number + 1}`, label: 'Next Block' },
          }}
        />

        {activationHardfork && activationResources.length > 0 && (
          <TerminalPanel className="border-warning/30 mb-4">
            <TerminalPanelHeader indicator="active">
              HARDFORK RESOURCES · {activationHardfork.shortName.toUpperCase()}
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="flex flex-wrap gap-3">
                {activationResources.map((resource) => (
                  <a
                    key={`${activationHardfork.id}-${resource.label}`}
                    href={resource.url}
                    target="_blank"
                    rel="noreferrer"
                    className="text-interactive font-mono text-xs hover:underline"
                    data-testid="block-hardfork-resource-link"
                  >
                    {resource.label}
                  </a>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        <TerminalPanel className="mb-4">
          <TerminalPanelHeader indicator="active">Epoch Progress</TerminalPanelHeader>
          <TerminalPanelContent>
            <div className="mb-3 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-text-muted text-sm">Epoch</span>
                <Link
                  href={`/blocks/${epochStartNumber}`}
                  className="text-interactive font-mono hover:underline"
                >
                  #{block.epochNumber.toLocaleString()}
                </Link>
              </div>
              <div className="flex items-center gap-2 text-sm">
                <span className="text-text-primary font-mono">
                  {block.epochIndex + 1}
                  <sup className="text-xs">{ordinalSuffix}</sup>
                </span>
                <span className="text-text-muted">of</span>
                <span className="text-text-primary font-mono">{block.epochLength}</span>
              </div>
            </div>
            <ProgressBar
              value={block.epochIndex + 1}
              max={block.epochLength}
              color="green"
              labelFormat="percent"
            />
            <div className="text-text-muted mt-2 flex justify-between font-mono text-xs">
              <Link href={`/blocks/${epochStartNumber}`} className="hover:text-text-secondary">
                #{epochStartNumber.toLocaleString()}
              </Link>
              <Link
                href={`/blocks/${epochStartNumber + block.epochLength - 1}`}
                className="hover:text-text-secondary"
              >
                #{(epochStartNumber + block.epochLength - 1).toLocaleString()}
              </Link>
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel className="mb-4">
          <TerminalPanelHeader indicator="active">Block Details</TerminalPanelHeader>
          <TerminalPanelContent>
            <DataGrid columns={2}>
              <div>
                <DataField label="Timestamp">{formatTimestamp(block.timestamp)}</DataField>
                <DataField label="Difficulty">{block.difficulty}</DataField>
                <DataField label="Nonce" copyValue={block.nonce}>
                  <span className="max-w-[200px] truncate text-xs">{block.nonce}</span>
                </DataField>
                {feeStats && (
                  <DataField label="Size">
                    <UsageBar value={feeStats.totalSize} max={BLOCK_MAX_SIZE} unit="Bytes" />
                  </DataField>
                )}
                <DataField label="Transactions Root" copyValue={block.transactionsRoot}>
                  <span className="max-w-[180px] truncate text-xs">{block.transactionsRoot}</span>
                </DataField>
              </div>

              <div>
                <DataField label="Mining Reward">
                  {block.miningReward ? (
                    block.miningRewardTxHash ? (
                      <Link
                        href={`/tx/${block.miningRewardTxHash}`}
                        className="text-interactive hover:underline"
                      >
                        <FormatReward reward={block.miningReward} />
                      </Link>
                    ) : (
                      <FormatReward reward={block.miningReward} />
                    )
                  ) : (
                    <Badge variant="amber">Pending</Badge>
                  )}
                </DataField>
                <DataField label="Miner">
                  {block.minerAddress ? (
                    <Address address={block.minerAddress} className="text-emphasis" />
                  ) : (
                    <span className="text-text-muted">-</span>
                  )}
                </DataField>
                <DataField label="Miner Message" copyValue={block.minerMessage || undefined}>
                  {block.minerMessage ? (
                    <span className="max-w-[200px] truncate">{block.minerMessage}</span>
                  ) : (
                    <span className="text-text-muted">-</span>
                  )}
                </DataField>
                <DataField label="Cycles">
                  {feeStats && (feeStats.totalCycles > 0 || block.number === 0) ? (
                    <UsageBar value={feeStats.totalCycles} max={BLOCK_MAX_CYCLES} />
                  ) : (
                    <span className="text-text-muted italic">Calculating...</span>
                  )}
                </DataField>
                <DataField label="Uncle Count">{block.unclesCount}</DataField>
              </div>
            </DataGrid>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel>
          <Tabs defaultValue="transactions">
            <TerminalPanelHeader indicator="none">
              <TabsList className="gap-6 bg-transparent p-0">
                <TabsTrigger
                  value="transactions"
                  className="data-[state=active]:border-interactive data-[state=active]:text-interactive text-text-muted rounded-none border-b-2 border-transparent px-0 pb-2 data-[state=active]:bg-transparent data-[state=active]:shadow-none"
                >
                  Transactions ({txs?.data?.length ?? block.transactionsCount})
                </TabsTrigger>
                <TabsTrigger
                  value="proposals"
                  className="data-[state=active]:border-interactive data-[state=active]:text-interactive text-text-muted rounded-none border-b-2 border-transparent px-0 pb-2 data-[state=active]:bg-transparent data-[state=active]:shadow-none"
                >
                  Proposals ({block.proposalsCount})
                </TabsTrigger>
              </TabsList>
            </TerminalPanelHeader>

            <TabsContent value="transactions" className="m-0">
              {txs?.data?.length ? (
                txs.data.map((tx, index) => (
                  <TerminalRow key={tx.hash}>
                    <div className="flex items-center justify-between">
                      <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-3">
                          <Link
                            href={`/tx/${tx.hash}`}
                            className="text-interactive font-mono text-sm hover:underline"
                          >
                            <Hash hash={tx.hash} copyable={false} />
                          </Link>
                          {tx.isCellbase && <Badge variant="amber">Cellbase</Badge>}
                        </div>
                        <span className="text-text-muted text-xs">Index: {index}</span>
                      </div>
                      <div className="text-text-secondary text-right font-mono text-sm">
                        <span className="text-emphasis-dim">{tx.inputsCount}</span>
                        <span className="text-text-muted mx-1">→</span>
                        <span className="text-warning-dim">{tx.outputsCount}</span>
                      </div>
                    </div>
                  </TerminalRow>
                ))
              ) : (
                <TerminalPanelContent className="text-text-muted py-8 text-center">
                  No transactions
                </TerminalPanelContent>
              )}
            </TabsContent>

            <TabsContent value="proposals" className="m-0">
              {block.proposalsCount > 0 ? (
                proposals?.length ? (
                  proposals.map((proposal) => (
                    <TerminalRow key={proposal.proposalId}>
                      <div className="flex items-center justify-between">
                        <div className="flex flex-col gap-1">
                          <div className="flex items-center gap-3">
                            <span className="text-text-muted text-xs">
                              #{proposal.proposalIndex}
                            </span>
                            <span className="text-text-secondary font-mono text-sm">
                              {proposal.proposalId}
                            </span>
                          </div>
                          {proposal.committedTxHash && (
                            <div className="flex items-center gap-2 text-xs">
                              <span className="text-text-muted">Committed:</span>
                              <Link
                                href={`/tx/${proposal.committedTxHash}`}
                                className="text-interactive font-mono hover:underline"
                              >
                                <Hash hash={proposal.committedTxHash} copyable={false} />
                              </Link>
                              <span className="text-base-border">
                                (Block #{proposal.committedBlockNumber?.toLocaleString()})
                              </span>
                            </div>
                          )}
                        </div>
                        {proposal.committedTxHash ? (
                          <Badge variant="green">Committed</Badge>
                        ) : (
                          <Badge variant="amber">Pending</Badge>
                        )}
                      </div>
                    </TerminalRow>
                  ))
                ) : (
                  <TerminalPanelContent className="text-text-muted py-8 text-center">
                    Loading proposals...
                  </TerminalPanelContent>
                )
              ) : (
                <TerminalPanelContent className="text-text-muted py-8 text-center">
                  No proposals in this block
                </TerminalPanelContent>
              )}
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}
