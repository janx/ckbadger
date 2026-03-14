'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { HexDisplay } from '@/components/ui/hex-display';
import { Capacity } from '@/components/ui/capacity';
import { Badge } from '@/components/ui/page-header';
import { TerminalRow } from '@/components/ui/terminal-panel';
import type { ScriptRefHashType } from '@/lib/script-ref';

interface CodeCellsListProps {
  codeHash: string;
  hashType: ScriptRefHashType;
}

export function CodeCellsList({ codeHash, hashType }: CodeCellsListProps) {
  const { data, isLoading } = useQuery({
    queryKey: ['code-cells', codeHash, hashType],
    queryFn: () => api.getCodeCells(codeHash, hashType),
    staleTime: Infinity,
  });

  if (isLoading) {
    return <div className="text-text-dim px-4 py-3 text-xs">Loading code cells...</div>;
  }

  if (!data || data.codeCells.length === 0) {
    return <div className="text-text-dim px-4 py-3 text-xs">No code cells found</div>;
  }

  return (
    <div>
      <div className="border-base-border bg-base-surface/50 text-text-dim flex items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
        <div className="w-48">Outpoint</div>
        <div className="w-20">Status</div>
        <div className="w-28 text-right">Created At</div>
        <div className="flex-1 text-right">Capacity</div>
      </div>
      {data.codeCells.map((cell) => (
        <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
          <div className="flex items-center gap-x-4">
            <div className="w-48">
              <Link href={`/cell/${cell.txHash}-${cell.outputIndex}`} className="hover:underline">
                <HexDisplay
                  value={`${cell.txHash}:${cell.outputIndex}`}
                  size="sm"
                  startChars={8}
                  endChars={8}
                />
              </Link>
            </div>
            <div className="w-20">
              <Badge variant={cell.status === 'live' ? 'green' : 'gray'}>
                {cell.status === 'live' ? 'Live' : 'Consumed'}
              </Badge>
            </div>
            <div className="w-28 text-right">
              <Link
                href={`/blocks/${cell.createdAtBlock}`}
                className="text-emphasis font-mono text-xs hover:underline"
              >
                #{cell.createdAtBlock.toLocaleString()}
              </Link>
            </div>
            <div className="flex-1 text-right">
              <Capacity value={cell.capacity} className="text-sm" />
            </div>
          </div>
        </TerminalRow>
      ))}
    </div>
  );
}

export function CodeCellsSummary({
  liveCount,
  totalCount,
}: {
  liveCount: number;
  totalCount: number;
}) {
  const consumedCount = totalCount - liveCount;
  if (totalCount === 0) return <span className="text-text-dim">-</span>;
  return (
    <span className="text-text-dim text-xs">
      {liveCount > 0 && <span className="text-positive">{liveCount} live</span>}
      {liveCount > 0 && consumedCount > 0 && ', '}
      {consumedCount > 0 && <span>{consumedCount} consumed</span>}
    </span>
  );
}
