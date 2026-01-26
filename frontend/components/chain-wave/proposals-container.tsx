'use client';

interface ProposalsContainerProps {
  shortIds: string[];
  totalCount: number;
}

function toShortId(txHash: string): string {
  const hash = txHash.startsWith('0x') ? txHash.slice(2) : txHash;
  if (hash.length <= 12) return `0x${hash}`;
  return `0x${hash.slice(0, 6)}...${hash.slice(-6)}`;
}

export function ProposalsContainer({ shortIds, totalCount }: ProposalsContainerProps) {
  return (
    <div className="flex flex-1 flex-col rounded-xl border-2 border-amber-600/50 bg-gradient-to-br from-amber-950/30 to-slate-900/50 p-3 sm:p-4">
      <div className="mb-2 flex items-center justify-between sm:mb-3">
        <div>
          <h3 className="text-sm font-bold text-amber-400 sm:text-base">Proposed</h3>
          <div className="text-[10px] text-slate-500 sm:text-xs">Awaiting commit</div>
        </div>
        <div className="text-right text-xs text-amber-500/80 sm:text-sm">
          <span className="font-bold text-white">{totalCount.toLocaleString()}</span>
          <span className="ml-1">txs</span>
        </div>
      </div>

      <div className="flex-1 overflow-hidden rounded-lg bg-black/20 p-2">
        {shortIds.length === 0 ? (
          <div className="flex h-full min-h-[140px] w-full items-center justify-center text-xs text-slate-600 sm:min-h-[180px]">
            No proposed txs
          </div>
        ) : (
          <div className="flex h-full min-h-[140px] flex-wrap content-start gap-1 overflow-y-auto sm:min-h-[180px]">
            {shortIds.map((txHash) => (
              <span
                key={txHash}
                className="inline-block rounded bg-amber-600/30 px-1.5 py-0.5 font-mono text-[9px] text-amber-300 sm:text-[10px]"
              >
                {toShortId(txHash)}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
