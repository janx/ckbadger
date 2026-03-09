'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, Block } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import { cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { HexDisplay } from '@/components/ui/hex-display';

interface LatestBlocksProps {
  isRealtime?: boolean;
  initialBlocks?: Block[];
}

export function LatestBlocks({ isRealtime = false, initialBlocks }: LatestBlocksProps) {
  const [newBlockNumber, setNewBlockNumber] = useState<number | null>(null);
  const prevBlocksRef = useRef<number[]>([]);

  const { data: blocks, isLoading } = useQuery({
    queryKey: ['latest-blocks'],
    queryFn: () => api.getBlocks({ limit: 10 }),
    initialData: initialBlocks?.length
      ? {
          data: initialBlocks,
          total: initialBlocks.length,
          limit: 10,
          hasMore: false,
          nextCursor: null,
        }
      : undefined,
    initialDataUpdatedAt: 0,
    refetchInterval: 10000,
  });

  const itemCount = blocks?.data?.length ?? 0;
  const showSkeleton = isLoading || itemCount === 0;

  useEffect(() => {
    if (blocks?.data) {
      const currentNumbers = blocks.data.map((b) => b.number);
      const prevNumbers = prevBlocksRef.current;

      if (prevNumbers.length > 0) {
        const newBlock = currentNumbers.find((n) => !prevNumbers.includes(n));
        if (newBlock) {
          setNewBlockNumber(newBlock);
          setTimeout(() => setNewBlockNumber(null), 2000);
        }
      }

      prevBlocksRef.current = currentNumbers;
    }
  }, [blocks?.data]);

  const headerActions = (
    <Link
      href="/blocks"
      className="text-text-muted hover:text-interactive font-mono text-xs transition-colors"
    >
      VIEW ALL →
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        Latest Blocks
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none">
        {showSkeleton
          ? Array.from({ length: 8 }).map((_, i) => (
              <TerminalRow key={i} hoverable={false}>
                <div className="flex animate-pulse items-center justify-between">
                  <div className="space-y-2">
                    <div className="bg-base-elevated h-4 w-20 rounded" />
                    <div className="bg-base-elevated h-3 w-16 rounded" />
                  </div>
                  <div className="space-y-2 text-right">
                    <div className="bg-base-elevated h-3 w-24 rounded" />
                    <div className="bg-base-elevated h-3 w-12 rounded" />
                  </div>
                </div>
              </TerminalRow>
            ))
          : blocks?.data?.slice(0, 8).map((block) => (
              <TerminalRow
                key={block.number}
                className={cn(
                  'transition-all duration-500',
                  newBlockNumber === block.number && 'bg-emphasis/10 shadow-glow'
                )}
              >
                <div className="flex items-center justify-between gap-4">
                  <div className="min-w-0">
                    <Link
                      href={`/blocks/${block.number}`}
                      className="group flex items-center gap-1 transition-opacity hover:opacity-80"
                    >
                      <span className="text-text-muted text-xs">#</span>
                      <span className="text-interactive font-mono font-bold tabular-nums">
                        {block.number.toLocaleString()}
                      </span>
                    </Link>
                    <div className="mt-1.5 flex items-center gap-3 text-xs">
                      <span className="text-text-muted">
                        <span className="text-emphasis-dim">{block.transactionsCount}</span> txs
                      </span>
                      {block.hardforkActivation && (
                        <span
                          className="border-warning-dim/60 bg-warning/10 text-warning rounded border px-1.5 py-0.5 font-mono text-[10px]"
                          data-testid={`latest-block-hardfork-${block.number}`}
                        >
                          HF · {block.hardforkActivation.shortName.toUpperCase()}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="min-w-0 text-right">
                    <Link
                      href={`/blocks/${block.number}`}
                      className="inline-block transition-opacity hover:opacity-80"
                    >
                      <HexDisplay
                        value={block.hash}
                        truncate
                        startChars={8}
                        endChars={6}
                        color="green"
                        size="sm"
                        showGroupHighlight={false}
                        copyable={false}
                      />
                    </Link>
                    <div className="text-text-muted mt-1.5 text-xs">
                      {formatTimeAgo(block.timestamp)}
                    </div>
                  </div>
                </div>
              </TerminalRow>
            ))}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
