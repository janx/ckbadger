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
    <div className="h-full overflow-hidden rounded-lg border border-slate-800 bg-slate-900 p-4">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-baseline gap-3">
          <span className="font-mono text-xs uppercase tracking-wider text-slate-500">Epoch</span>
          <span className="text-terminal-green font-mono text-2xl font-bold tabular-nums">
            {epochNumber.toLocaleString()}
          </span>
          <span className="rounded bg-slate-800 px-2 py-0.5 font-mono text-xs tabular-nums text-slate-300">
            {progressClamped.toFixed(1)}%
          </span>
        </div>
        {estimatedTimeRemaining && (
          <div className="flex items-baseline gap-2">
            <span className="font-mono text-xs uppercase tracking-wider text-slate-500">
              Est. Time
            </span>
            <span className="text-amber font-mono tabular-nums">{estimatedTimeRemaining}</span>
          </div>
        )}
      </div>

      <div className="relative h-3 overflow-hidden rounded-full bg-slate-800 sm:h-4">
        <div
          className={cn(
            'absolute inset-y-0 left-0 rounded-full transition-all duration-1000',
            progress < 25
              ? 'bg-gradient-to-r from-emerald-600 to-emerald-500'
              : progress < 50
                ? 'bg-gradient-to-r from-emerald-500 to-green-500'
                : progress < 75
                  ? 'bg-gradient-to-r from-green-500 to-amber-500'
                  : 'bg-gradient-to-r from-amber-500 to-purple-500'
          )}
          style={{ width: `${progressClamped}%` }}
        />
        <div
          className="absolute top-1/2 h-2.5 w-0.5 -translate-y-1/2 rounded-full bg-white/80 shadow-sm sm:h-3"
          style={{ left: `${progressClamped}%`, transform: `translateX(-50%) translateY(-50%)` }}
        />
      </div>

      <div className="mt-2 flex items-center justify-between text-[10px] text-slate-500 sm:text-xs">
        <span className="font-mono tabular-nums">#{epochStartBlock.toLocaleString()}</span>
        <span className="font-mono tabular-nums">
          {epochIndex.toLocaleString()} / {epochLength.toLocaleString()}
        </span>
        <span className="font-mono tabular-nums">#{epochEndBlock.toLocaleString()}</span>
      </div>
    </div>
  );
}
