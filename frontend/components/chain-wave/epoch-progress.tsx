'use client';

import { cn } from '@/lib/utils';

interface EpochProgressProps {
  epochNumber: number;
  epochIndex: number;
  epochLength: number;
  latestBlock: number;
  estimatedTimeRemaining?: string;
}

export function EpochProgress({
  epochNumber,
  epochIndex,
  epochLength,
  latestBlock,
  estimatedTimeRemaining,
}: EpochProgressProps) {
  const progress = epochLength > 0 ? (epochIndex / epochLength) * 100 : 0;
  const progressClamped = Math.min(100, Math.max(0, progress));

  const epochStartBlock = latestBlock - epochIndex;
  const epochEndBlock = epochStartBlock + epochLength - 1;

  return (
    <div className="border-base-border bg-base-surface h-full overflow-hidden rounded-lg border p-4">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-baseline gap-3">
          <span className="text-text-muted font-mono text-xs uppercase tracking-wider">Epoch</span>
          <span className="text-emphasis font-mono text-2xl font-bold tabular-nums">
            {epochNumber.toLocaleString()}
          </span>
          <span className="bg-base-elevated text-text-secondary rounded px-2 py-0.5 font-mono text-xs tabular-nums">
            {progressClamped.toFixed(1)}%
          </span>
        </div>
        {estimatedTimeRemaining && (
          <div className="flex items-baseline gap-2">
            <span className="text-text-muted font-mono text-xs uppercase tracking-wider">
              Est. Time
            </span>
            <span className="text-warning font-mono tabular-nums">{estimatedTimeRemaining}</span>
          </div>
        )}
      </div>

      <div className="bg-base-elevated relative h-3 overflow-hidden rounded-full sm:h-4">
        <div
          className={cn(
            'absolute inset-y-0 left-0 rounded-full transition-all duration-1000',
            progress < 25
              ? 'from-emphasis-dim to-emphasis-dim bg-gradient-to-r'
              : progress < 50
                ? 'from-emphasis-dim to-emphasis bg-gradient-to-r'
                : progress < 75
                  ? 'from-emphasis to-warning-dim bg-gradient-to-r'
                  : 'from-warning-dim to-warning bg-gradient-to-r'
          )}
          style={{ width: `${progressClamped}%` }}
        />
        <div
          className="absolute top-1/2 h-2.5 w-0.5 -translate-y-1/2 rounded-full bg-white/80 shadow-sm sm:h-3"
          style={{ left: `${progressClamped}%`, transform: `translateX(-50%) translateY(-50%)` }}
        />
      </div>

      <div className="text-text-muted mt-2 flex items-center justify-between text-[10px] sm:text-xs">
        <span className="font-mono tabular-nums">#{epochStartBlock.toLocaleString()}</span>
        <span className="font-mono tabular-nums">
          {epochIndex.toLocaleString()} / {epochLength.toLocaleString()}
        </span>
        <span className="font-mono tabular-nums">#{epochEndBlock.toLocaleString()}</span>
      </div>
    </div>
  );
}
