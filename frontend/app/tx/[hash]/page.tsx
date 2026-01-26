'use client';

import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { UsageBar } from '@/components/ui/progress-bar';
import { HexDisplay } from '@/components/ui/hex-display';
import { Capacity } from '@/components/ui/capacity';
import { Address } from '@/components/ui/address';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { CellGraph } from '@/components/cell-graph';
import {
  api,
  type CellDep,
  type GraphNode,
  type ScriptLookupResponse,
  type AssetTransfer,
} from '@/lib/api';
import { formatTimeAgo, formatCkbAmount } from '@/lib/utils';

function useCyclesCalculation(hash: string, txCycles: number | undefined, isCellbase: boolean) {
  const queryClient = useQueryClient();
  const [isCalculating, setIsCalculating] = useState(false);
  const [hasFailed, setHasFailed] = useState(false);
  const [triggeredForHash, setTriggeredForHash] = useState<string | null>(null);

  const parsedCycles = txCycles ?? null;
  const hasCycles = parsedCycles !== null && parsedCycles > 0;
  const needsCalculation = !isCellbase && !hasCycles && !hasFailed;

  useEffect(() => {
    setIsCalculating(false);
    setHasFailed(false);
    setTriggeredForHash(null);
  }, [hash]);

  useEffect(() => {
    if (!needsCalculation || triggeredForHash === hash) return;

    setTriggeredForHash(hash);

    const trigger = async () => {
      try {
        const response = await api.triggerCyclesCalculation(hash);

        if (response.status === 'done') {
          queryClient.invalidateQueries({ queryKey: ['transaction', hash] });
        } else if (response.status === 'failed' || response.status === 'notFound') {
          setHasFailed(true);
        } else {
          setIsCalculating(true);
        }
      } catch {
        setHasFailed(true);
      }
    };

    trigger();
  }, [needsCalculation, triggeredForHash, hash, queryClient]);

  useEffect(() => {
    if (!isCalculating) return;

    const pollInterval = setInterval(async () => {
      try {
        const response = await api.getCyclesStatus(hash);

        if (response.status === 'done') {
          setIsCalculating(false);
          queryClient.invalidateQueries({ queryKey: ['transaction', hash] });
        } else if (response.status === 'failed' || response.status === 'notFound') {
          setIsCalculating(false);
          setHasFailed(true);
        }
      } catch {
        setIsCalculating(false);
        setHasFailed(true);
      }
    }, 2000);

    return () => clearInterval(pollInterval);
  }, [isCalculating, hash, queryClient]);

  return {
    cycles: parsedCycles,
    hasCycles,
    isCalculating,
    hasFailed,
  };
}

function formatAssetAmount(transfer: AssetTransfer): string {
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
}

function getAssetLabel(transfer: AssetTransfer): string {
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
}

function getAssetBadgeVariant(category: string): 'green' | 'amber' | 'red' | 'gray' | 'purple' {
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
}

export default function TransactionDetailPage() {
  const params = useParams();
  const router = useRouter();
  const hash = params.hash as string;

  const {
    data: tx,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['transaction', hash],
    queryFn: () => api.getTransactionDetail(hash),
  });

  const { cycles, hasCycles, isCalculating, hasFailed } = useCyclesCalculation(
    hash,
    tx?.cycles,
    tx?.isCellbase ?? false
  );

  const { data: graphData } = useQuery({
    queryKey: ['txGraph', hash],
    queryFn: () => api.getTransactionGraph(hash),
    enabled: !!hash,
  });

  const { data: cellDeps, isLoading: cellDepsLoading } = useQuery({
    queryKey: ['txCellDeps', hash],
    queryFn: () => api.getTransactionCellDeps(hash),
    enabled: !!hash,
  });

  const { data: lifecycle } = useQuery({
    queryKey: ['txLifecycle', hash],
    queryFn: () => api.getTransactionLifecycle(hash),
    enabled: !!hash && !!tx && !tx.isCellbase,
  });

  const codeHashes = useMemo(() => {
    if (!tx) return [];
    const hashes = new Set<string>();
    tx.inputs?.forEach((input) => {
      if (input.lock?.codeHash) hashes.add(input.lock.codeHash);
    });
    tx.outputs?.forEach((output) => {
      if (output.lock?.codeHash) hashes.add(output.lock.codeHash);
      if (output.type?.codeHash) hashes.add(output.type.codeHash);
    });
    return Array.from(hashes);
  }, [tx]);

  const { data: scriptLookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes],
    queryFn: () => api.lookupScripts(codeHashes),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });

  const { data: assetTransfers } = useQuery({
    queryKey: ['txAssetTransfers', hash],
    queryFn: () => api.getTransactionAssetTransfers(hash),
    enabled: !!hash,
  });

  const handleGraphNodeClick = (node: GraphNode) => {
    if (node.nodeType === 'transaction' && node.data?.hash) {
      router.push(`/tx/${node.data.hash}`);
    } else if (
      node.nodeType === 'cell' &&
      node.data?.txHash !== undefined &&
      node.data?.outputIndex !== undefined
    ) {
      router.push(`/cell/${node.data.txHash}-${node.data.outputIndex}`);
    }
  };

  if (isLoading) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="animate-pulse space-y-8">
            <div className="h-20 w-full rounded bg-slate-900" />
            <div className="h-64 w-full rounded bg-slate-900" />
            <div className="h-96 w-full rounded bg-slate-900" />
          </div>
        </main>
      </div>
    );
  }

  if (error || !tx) {
    return (
      <div className="min-h-screen bg-slate-950">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-xl text-slate-400">Transaction not found</h2>
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
          title="Transaction"
          hash={tx.hash}
          badge={<Badge variant="green">{tx.confirmations.toLocaleString()} Confirmations</Badge>}
        />

        <TerminalPanel className="mb-8" glow>
          <TerminalPanelHeader indicator="active">Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            <DataGrid>
              <DataField label="Block">
                {tx.isCellbase ? (
                  <Link
                    href={`/blocks/${tx.blockNumber}`}
                    className="text-terminal-green hover:underline"
                  >
                    #{tx.blockNumber.toLocaleString()}
                  </Link>
                ) : lifecycle?.proposedIn ? (
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-slate-500">Proposed</span>
                      <Link
                        href={`/blocks/${lifecycle.proposedIn.blockNumber}`}
                        className="text-slate-400 hover:text-slate-300 hover:underline"
                      >
                        #{lifecycle.proposedIn.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                    <span className="text-slate-600">→</span>
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-slate-500">Committed</span>
                      <Link
                        href={`/blocks/${tx.blockNumber}`}
                        className="text-terminal-green font-medium hover:underline"
                      >
                        #{tx.blockNumber.toLocaleString()}
                      </Link>
                    </div>
                    {lifecycle.commitmentDistance !== null && (
                      <Badge
                        variant={
                          lifecycle.commitmentDistance <= 4
                            ? 'green'
                            : lifecycle.commitmentDistance <= 7
                              ? 'amber'
                              : 'red'
                        }
                      >
                        +{lifecycle.commitmentDistance}
                      </Badge>
                    )}
                  </div>
                ) : (
                  <Link
                    href={`/blocks/${tx.blockNumber}`}
                    className="text-terminal-green hover:underline"
                  >
                    #{tx.blockNumber.toLocaleString()}
                  </Link>
                )}
              </DataField>

              <DataField label="Timestamp" copyValue={new Date(tx.timestamp).toISOString()}>
                {new Date(tx.timestamp).toLocaleString()} ({formatTimeAgo(tx.timestamp)})
              </DataField>

              <DataField label="Type">
                {tx.isCellbase ? (
                  <Badge variant="amber">Cellbase (Mining Reward)</Badge>
                ) : (
                  <Badge variant="blue">Normal Transaction</Badge>
                )}
              </DataField>

              <DataField label="Fee">
                <div className="flex flex-col items-end gap-1">
                  <span className="text-white">
                    <Capacity value={tx.fee} className="text-white" />
                    <span className="ml-2 font-mono text-sm tabular-nums text-slate-400">
                      ({BigInt(tx.fee).toLocaleString()} shannon)
                    </span>
                  </span>
                  {tx.feeRate && (
                    <span className="font-mono text-xs tabular-nums text-slate-400">
                      {Number(tx.feeRate).toLocaleString()} shannon/KB
                    </span>
                  )}
                </div>
              </DataField>

              {tx.txSize && (
                <DataField label="Size">
                  <UsageBar value={tx.txSize} max={512000} unit="Bytes" />
                </DataField>
              )}

              {!tx.isCellbase && (
                <DataField label="Cycles">
                  {hasCycles && cycles !== null ? (
                    <UsageBar value={cycles} max={3_500_000_000} />
                  ) : isCalculating ? (
                    <span className="inline-flex items-center gap-2 italic text-slate-400">
                      <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-slate-400 border-t-transparent" />
                      Calculating...
                    </span>
                  ) : hasFailed ? (
                    <span className="italic text-red-400">Calculation failed</span>
                  ) : (
                    <span className="italic text-slate-500">Not available</span>
                  )}
                </DataField>
              )}

              <DataField label="Carried Capacity">
                <Capacity
                  value={(BigInt(tx.outputsCapacity || '0') + BigInt(tx.fee)).toString()}
                  className="text-white"
                />
              </DataField>

              <DataField label="Occupied Capacity Change">
                {(() => {
                  const inputOccupied = BigInt(tx.inputsOccupiedCapacity || '0');
                  const outputOccupied = BigInt(tx.outputsOccupiedCapacity || '0');
                  const change = outputOccupied - inputOccupied;
                  const zero = BigInt(0);
                  const isIncrease = change > zero;
                  const isDecrease = change < zero;
                  const absChange = change < zero ? -change : change;
                  const f = formatCkbAmount(absChange);
                  return (
                    <div className="flex items-center justify-end gap-2">
                      {isIncrease && (
                        <span className="inline-flex items-center rounded border border-green-900/50 bg-green-900/50 px-2 py-1 font-mono text-sm tabular-nums text-green-400">
                          +{f.integer}
                          <span className="text-[0.85em] text-green-400/60">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {isDecrease && (
                        <span className="inline-flex items-center rounded border border-red-900/50 bg-red-900/50 px-2 py-1 font-mono text-sm tabular-nums text-red-400">
                          -{f.integer}
                          <span className="text-[0.85em] text-red-400/60">.{f.decimal}</span>
                          <span className="ml-1 text-[0.85em]">CKB</span>
                        </span>
                      )}
                      {!isIncrease && !isDecrease && (
                        <span className="inline-flex items-center rounded border border-slate-700 bg-slate-800 px-2 py-1 text-sm text-slate-400">
                          No change
                        </span>
                      )}
                    </div>
                  );
                })()}
              </DataField>
            </DataGrid>
          </TerminalPanelContent>
        </TerminalPanel>

        {assetTransfers && assetTransfers.length > 0 && (
          <TerminalPanel className="mb-8">
            <TerminalPanelHeader>Asset Transfers ({assetTransfers.length})</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="min-w-full">
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="w-24">Direction</div>
                  <div className="flex-1">Asset</div>
                  <div className="flex-1 text-right">Amount</div>
                </div>
                {assetTransfers.map((transfer, idx) => (
                  <TerminalRow key={idx} className="flex items-center">
                    <div className="w-24">
                      <Badge variant={transfer.direction === 'in' ? 'green' : 'red'}>
                        {transfer.direction === 'in' ? 'Incoming' : 'Outgoing'}
                      </Badge>
                    </div>
                    <div className="flex flex-1 items-center gap-3">
                      <Badge variant={getAssetBadgeVariant(transfer.assetCategory)}>
                        {getAssetLabel(transfer)}
                      </Badge>
                      {transfer.assetId && (
                        <span className="font-mono text-xs text-slate-500">
                          ID:{' '}
                          <HexDisplay
                            value={transfer.assetId}
                            truncate
                            startChars={6}
                            endChars={4}
                          />
                        </span>
                      )}
                    </div>
                    <div className="flex-1 text-right font-mono text-white">
                      {transfer.direction === 'in' ? '+' : '-'}
                      {formatAssetAmount(transfer)}
                    </div>
                  </TerminalRow>
                ))}
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        <TerminalPanel>
          <Tabs defaultValue="io">
            <TerminalPanelHeader>
              <TabsList>
                <TabsTrigger value="io">
                  Inputs/Outputs ({tx.inputsCount} → {tx.outputsCount})
                </TabsTrigger>
                <TabsTrigger value="scripts">Scripts</TabsTrigger>
                <TabsTrigger value="celldeps">Cell Deps</TabsTrigger>
                <TabsTrigger value="graph">Graph</TabsTrigger>
              </TabsList>
            </TerminalPanelHeader>

            <TabsContent value="io" className="p-0">
              <InputsOutputsTab tx={tx} scriptLookup={scriptLookup} />
            </TabsContent>

            <TabsContent value="scripts" className="p-0">
              <ScriptsSummaryTab tx={tx} scriptLookup={scriptLookup} />
            </TabsContent>

            <TabsContent value="celldeps" className="p-0">
              <CellDepsTab cellDeps={cellDeps} isLoading={cellDepsLoading} />
            </TabsContent>

            <TabsContent value="graph" className="p-4">
              {graphData && graphData.nodes.length > 0 ? (
                <CellGraph
                  nodes={graphData.nodes}
                  links={graphData.links}
                  onNodeClick={handleGraphNodeClick}
                  width={undefined}
                  height={400}
                />
              ) : (
                <p className="py-8 text-center text-slate-500">Loading graph...</p>
              )}
            </TabsContent>
          </Tabs>
        </TerminalPanel>
      </main>
    </div>
  );
}

interface TabProps {
  tx: NonNullable<Awaited<ReturnType<typeof api.getTransactionDetail>>>;
  scriptLookup?: ScriptLookupResponse;
}

function ScriptLabel({
  script,
  scriptLookup,
  type,
}: {
  script: { codeHash: string } | undefined;
  scriptLookup?: ScriptLookupResponse;
  type: 'lock' | 'type';
}) {
  if (!script) return null;
  const info = scriptLookup?.[script.codeHash];
  if (!info) return null;

  return (
    <Link href={`/scripts/${encodeURIComponent(info.name)}`}>
      <Badge variant={type === 'lock' ? 'blue' : 'purple'} className="hover:opacity-80">
        {info.name}
      </Badge>
    </Link>
  );
}

function InputsOutputsTab({ tx, scriptLookup }: TabProps) {
  return (
    <div className="grid gap-6 p-4 lg:grid-cols-2">
      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Inputs ({tx.inputsCount})
        </h4>
        {tx.inputs && tx.inputs.length > 0 ? (
          <div className="rounded-lg border border-slate-800 bg-slate-900/50">
            {tx.inputs.map((input, index) => (
              <TerminalRow key={index} className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs text-slate-500">#{index}</span>
                    <ScriptLabel script={input.lock} scriptLookup={scriptLookup} type="lock" />
                  </div>
                  {input.previousOutput && (
                    <Link
                      href={`/cell/${input.previousOutput.txHash}-${input.previousOutput.index}`}
                      className="hover:text-terminal-green group flex items-center gap-1 font-mono text-xs text-slate-400"
                    >
                      <HexDisplay
                        value={input.previousOutput.txHash}
                        startChars={8}
                        endChars={6}
                        color="amber"
                        size="sm"
                        copyable={false}
                      />
                      <span>:{input.previousOutput.index}</span>
                    </Link>
                  )}
                </div>
                {(input.address || input.capacity) && (
                  <div className="flex items-center justify-between">
                    {input.address ? (
                      <Address address={input.address} />
                    ) : (
                      <span className="text-sm text-red-400">Address error</span>
                    )}
                    {input.capacity && (
                      <Capacity value={input.capacity} className="text-sm text-slate-300" />
                    )}
                  </div>
                )}
              </TerminalRow>
            ))}
          </div>
        ) : tx.isCellbase ? (
          <p className="text-sm text-slate-500">Cellbase has no inputs</p>
        ) : (
          <p className="text-sm text-slate-500">Loading inputs...</p>
        )}
      </div>

      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Outputs ({tx.outputsCount})
        </h4>
        {tx.outputs && tx.outputs.length > 0 ? (
          <div className="rounded-lg border border-slate-800 bg-slate-900/50">
            {tx.outputs.map((output, index) => (
              <TerminalRow key={index} className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs text-slate-500">#{index}</span>
                    <ScriptLabel script={output.lock} scriptLookup={scriptLookup} type="lock" />
                    <ScriptLabel script={output.type} scriptLookup={scriptLookup} type="type" />
                  </div>
                  <Link
                    href={`/cell/${tx.hash}-${index}`}
                    className="text-terminal-green font-mono text-xs hover:underline"
                  >
                    View Cell
                  </Link>
                </div>
                <div className="flex items-center justify-between">
                  {output.address ? (
                    <Address address={output.address} />
                  ) : (
                    <span className="text-sm text-red-400">Address error</span>
                  )}
                  <Capacity value={output.capacity} className="text-slate-300" />
                </div>
              </TerminalRow>
            ))}
          </div>
        ) : (
          <p className="text-sm text-slate-500">Loading outputs...</p>
        )}
      </div>
    </div>
  );
}

interface ScriptInfo {
  codeHash: string;
  hashType: string;
  name: string | null;
  count: number;
}

function ScriptsSummaryTab({ tx, scriptLookup }: TabProps) {
  const scriptSummary = useMemo(() => {
    const lockScripts = new Map<string, ScriptInfo>();
    const typeScripts = new Map<string, ScriptInfo>();

    const addScript = (
      map: Map<string, ScriptInfo>,
      codeHash: string,
      hashType: string,
      name: string | null
    ) => {
      const key = `${codeHash}:${hashType}`;
      const existing = map.get(key);
      map.set(key, {
        codeHash,
        hashType,
        name,
        count: (existing?.count ?? 0) + 1,
      });
    };

    tx.inputs?.forEach((input) => {
      if (input.lock?.codeHash && input.lock?.hashType) {
        const name = scriptLookup?.[input.lock.codeHash]?.name ?? null;
        addScript(lockScripts, input.lock.codeHash, input.lock.hashType, name);
      }
    });

    tx.outputs?.forEach((output) => {
      if (output.lock?.codeHash && output.lock?.hashType) {
        const name = scriptLookup?.[output.lock.codeHash]?.name ?? null;
        addScript(lockScripts, output.lock.codeHash, output.lock.hashType, name);
      }
      if (output.type?.codeHash && output.type?.hashType) {
        const name = scriptLookup?.[output.type.codeHash]?.name ?? null;
        addScript(typeScripts, output.type.codeHash, output.type.hashType, name);
      }
    });

    return { lockScripts, typeScripts };
  }, [tx, scriptLookup]);

  return (
    <div className="grid gap-6 p-4 lg:grid-cols-2">
      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Lock Scripts
        </h4>
        <div className="rounded-lg border border-slate-800 bg-slate-900/50">
          {Array.from(scriptSummary.lockScripts.values()).map((script) => (
            <TerminalRow
              key={`${script.codeHash}:${script.hashType}`}
              className="flex items-center justify-between"
            >
              <div className="min-w-0 flex-1">
                {script.name ? (
                  <Link
                    href={`/scripts/${encodeURIComponent(script.name)}`}
                    className="text-terminal-green hover:underline"
                  >
                    {script.name}
                  </Link>
                ) : (
                  <Link
                    href={`/script/${script.codeHash}?hashType=${script.hashType}&kind=lock`}
                    className="group"
                  >
                    <HexDisplay
                      value={script.codeHash}
                      truncate
                      className="text-terminal-green group-hover:underline"
                    />
                  </Link>
                )}
              </div>
              <Badge variant="gray">
                {script.count} cell{script.count > 1 ? 's' : ''}
              </Badge>
            </TerminalRow>
          ))}
          {scriptSummary.lockScripts.size === 0 && (
            <div className="p-4 text-center text-sm text-slate-500">No lock scripts</div>
          )}
        </div>
      </div>

      <div>
        <h4 className="mb-3 border-b border-slate-800 pb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Type Scripts
        </h4>
        <div className="rounded-lg border border-slate-800 bg-slate-900/50">
          {Array.from(scriptSummary.typeScripts.values()).map((script) => (
            <TerminalRow
              key={`${script.codeHash}:${script.hashType}`}
              className="flex items-center justify-between"
            >
              <div className="min-w-0 flex-1">
                {script.name ? (
                  <Link
                    href={`/scripts/${encodeURIComponent(script.name)}`}
                    className="text-terminal-green hover:underline"
                  >
                    {script.name}
                  </Link>
                ) : (
                  <Link
                    href={`/script/${script.codeHash}?hashType=${script.hashType}&kind=type`}
                    className="group"
                  >
                    <HexDisplay
                      value={script.codeHash}
                      truncate
                      className="text-terminal-green group-hover:underline"
                    />
                  </Link>
                )}
              </div>
              <Badge variant="gray">
                {script.count} cell{script.count > 1 ? 's' : ''}
              </Badge>
            </TerminalRow>
          ))}
          {scriptSummary.typeScripts.size === 0 && (
            <div className="p-4 text-center text-sm text-slate-500">No type scripts</div>
          )}
        </div>
      </div>
    </div>
  );
}

interface CellDepsTabProps {
  cellDeps?: CellDep[];
  isLoading: boolean;
}

function CellDepsTab({ cellDeps, isLoading }: CellDepsTabProps) {
  if (isLoading) {
    return <p className="py-8 text-center text-slate-500">Loading cell deps...</p>;
  }

  if (!cellDeps || cellDeps.length === 0) {
    return <p className="py-8 text-center text-slate-500">No cell dependencies</p>;
  }

  return (
    <div className="m-4 rounded-lg border border-slate-800 bg-slate-900/50">
      {cellDeps.map((cellDep, index) => (
        <TerminalRow
          key={`${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
          className="flex items-center justify-between"
        >
          <div className="flex items-center gap-3">
            <span className="font-mono text-xs text-slate-500">#{index}</span>
            <Link
              href={`/cell/${cellDep.outPointTxHash}-${cellDep.outPointIndex}`}
              className="hover:text-terminal-green group flex items-center gap-1"
            >
              <HexDisplay
                value={cellDep.outPointTxHash}
                startChars={10}
                endChars={8}
                color="amber"
                copyable={false}
              />
              <span className="group-hover:text-terminal-green text-slate-500">:</span>
              <span className="group-hover:text-terminal-green font-mono text-sm text-slate-300">
                {cellDep.outPointIndex}
              </span>
            </Link>
          </div>
          <Badge variant={cellDep.depType === 'dep_group' ? 'purple' : 'blue'}>
            {cellDep.depType}
          </Badge>
        </TerminalRow>
      ))}
    </div>
  );
}
