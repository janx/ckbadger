'use client';

import { useState, useEffect } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { useParams, useSearchParams } from 'next/navigation';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { Capacity } from '@/components/ui/capacity';
import { useCursorPagination } from '@/hooks/useCursorPagination';

type ScriptKind = 'lock' | 'type' | 'both';
type HashType = 'type' | 'data' | 'data1' | 'data2';

function isValidHashType(value: string | null): value is HashType {
  return value === 'type' || value === 'data' || value === 'data1' || value === 'data2';
}

function isValidScriptKind(value: string | null): value is ScriptKind {
  return value === 'lock' || value === 'type' || value === 'both';
}

export default function ScriptByCodeHashPage() {
  const params = useParams();
  const searchParams = useSearchParams();
  const codeHash = params.codeHash as string;

  const initialHashType = searchParams.get('hashType');
  const initialKind = searchParams.get('kind');

  const [hashType, setHashType] = useState<HashType>(
    isValidHashType(initialHashType) ? initialHashType : 'type'
  );
  const [scriptKind, setScriptKind] = useState<ScriptKind>(
    isValidScriptKind(initialKind) ? initialKind : 'both'
  );
  const cellsPagination = useCursorPagination();

  useEffect(() => {
    if (isValidHashType(initialHashType)) {
      setHashType(initialHashType);
    }
    if (isValidScriptKind(initialKind)) {
      setScriptKind(initialKind);
    }
  }, [initialHashType, initialKind]);

  const { data: lookupResult } = useQuery({
    queryKey: ['script-lookup', codeHash],
    queryFn: () => api.lookupScripts([codeHash]),
    staleTime: Infinity,
  });

  const knownScript = lookupResult?.[codeHash];

  const { data: codeCellData } = useQuery({
    queryKey: ['code-cell', codeHash, hashType],
    queryFn: () => api.getCodeCell(codeHash, hashType),
    enabled: !knownScript?.codeCellTxHash,
    staleTime: Infinity,
  });

  const codeCellTxHash = knownScript?.codeCellTxHash || codeCellData?.txHash;
  const codeCellOutputIndex = knownScript?.codeCellOutputIndex ?? codeCellData?.outputIndex ?? null;

  const { data: cellsData, isLoading: isCellsLoading } = useQuery({
    queryKey: ['script-cells-by-hash', codeHash, hashType, scriptKind, cellsPagination.cursor],
    queryFn: () =>
      api.getCellsByScriptRef({
        codeHash,
        hashType,
        scriptKind,
        limit: 20,
        cursor: cellsPagination.cursor,
      }),
    placeholderData: keepPreviousData,
  });

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Script"
          subtitle={
            knownScript ? (
              <Link
                href={`/scripts/${encodeURIComponent(knownScript.name)}`}
                className="text-terminal-green hover:underline"
              >
                {knownScript.name}
              </Link>
            ) : undefined
          }
        />

        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Deployment</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
              <div className="w-40">Code Cell</div>
              <div className="flex-1">Code Hash</div>
              <div className="w-20 text-center">Hash Type</div>
              <div className="w-20 text-center">Kind</div>
              <div className="w-24 text-right">Cells</div>
              <div className="w-32 text-right">Capacity</div>
            </div>

            <TerminalRow hoverable={false}>
              <div className="flex items-center">
                <div className="w-40">
                  {codeCellTxHash && codeCellOutputIndex !== null ? (
                    <Link
                      href={`/cell/${codeCellTxHash}-${codeCellOutputIndex}`}
                      className="hover:underline"
                    >
                      <HexDisplay
                        value={`${codeCellTxHash}:${codeCellOutputIndex}`}
                        color="green"
                        size="sm"
                        startChars={8}
                        endChars={8}
                      />
                    </Link>
                  ) : (
                    <span className="text-slate-500">-</span>
                  )}
                </div>
                <div className="flex-1">
                  <HexDisplay value={codeHash} truncate={false} color="white" size="sm" />
                </div>
                <div className="w-20 text-center">
                  <Badge variant="gray">{knownScript?.hashType || hashType}</Badge>
                </div>
                <div className="w-20 text-center">
                  {knownScript?.scriptKind || (scriptKind !== 'both' ? scriptKind : null) ? (
                    <Badge
                      variant={
                        (knownScript?.scriptKind || scriptKind) === 'lock' ? 'blue' : 'purple'
                      }
                    >
                      {knownScript?.scriptKind || scriptKind}
                    </Badge>
                  ) : (
                    <span className="text-slate-500">-</span>
                  )}
                </div>
                <div className="w-24 text-right font-mono text-slate-400">
                  {knownScript ? knownScript.liveCellsCount.toLocaleString() : '-'}
                </div>
                <div className="w-32 text-right text-slate-400">
                  {knownScript ? <Capacity value={knownScript.liveCapacitySum} /> : '-'}
                </div>
              </div>
            </TerminalRow>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel>
          <TerminalPanelHeader
            indicator="active"
            actions={
              <div className="flex flex-wrap gap-4 text-sm font-normal">
                <div className="flex items-center gap-2">
                  <span className="text-slate-500">Hash Type:</span>
                  <select
                    value={hashType}
                    onChange={(e) => {
                      setHashType(e.target.value as HashType);
                      cellsPagination.reset();
                    }}
                    className="rounded border border-slate-700 bg-slate-800 px-2 py-1 font-mono text-xs text-white"
                  >
                    <option value="type">type</option>
                    <option value="data">data</option>
                    <option value="data1">data1</option>
                    <option value="data2">data2</option>
                  </select>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-slate-500">Script Kind:</span>
                  <select
                    value={scriptKind}
                    onChange={(e) => {
                      setScriptKind(e.target.value as ScriptKind);
                      cellsPagination.reset();
                    }}
                    className="rounded border border-slate-700 bg-slate-800 px-2 py-1 font-mono text-xs text-white"
                  >
                    <option value="both">Both</option>
                    <option value="lock">Lock</option>
                    <option value="type">Type</option>
                  </select>
                </div>
              </div>
            }
          >
            Cells
          </TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {isCellsLoading ? (
              <div className="py-8 text-center text-slate-400">Loading cells...</div>
            ) : cellsData && cellsData.data.length > 0 ? (
              <>
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="flex-1">Cell</div>
                  <div className="w-32 text-right">Capacity</div>
                  <div className="w-28 text-right">Data Size</div>
                  <div className="w-28 text-right">Created At</div>
                </div>

                {cellsData.data.map((cell) => (
                  <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
                    <div className="flex items-center">
                      <div className="flex-1">
                        <Link
                          href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                          className="hover:underline"
                        >
                          <HexDisplay
                            value={`${cell.txHash}:${cell.outputIndex}`}
                            color="green"
                            size="sm"
                            startChars={8}
                            endChars={8}
                          />
                        </Link>
                      </div>
                      <div className="w-32 text-right text-sm text-slate-400">
                        <Capacity value={cell.capacity} className="text-sm" />
                      </div>
                      <div className="w-28 text-right font-mono text-sm text-slate-400">
                        {cell.dataSize.toLocaleString()} bytes
                      </div>
                      <div className="w-28 text-right">
                        <Link
                          href={`/blocks/${cell.createdAtBlock}`}
                          className="text-terminal-green font-mono text-sm hover:underline"
                        >
                          #{cell.createdAtBlock.toLocaleString()}
                        </Link>
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="py-8 text-center text-slate-500">
                No cells found for this script with hash_type=&quot;{hashType}&quot;
              </div>
            )}
          </TerminalPanelContent>

          {cellsData && cellsData.data.length > 0 && (
            <TerminalPanelFooter>
              <CursorPagination
                total={cellsData.total}
                totalLabel="cells"
                pageSize={20}
                page={cellsPagination.page}
                hasMore={cellsData.hasMore}
                hasPrevious={cellsPagination.hasPrevious}
                onNext={() => cellsPagination.goToNext(cellsData.nextCursor)}
                onPrevious={cellsPagination.goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
