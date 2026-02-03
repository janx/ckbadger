'use client';

import { PendingProposal } from '@/lib/api';

interface ProposalsContainerProps {
  proposals: PendingProposal[];
  totalCount: number;
}

function formatShortId(proposalId: string): string {
  const id = proposalId.startsWith('0x') ? proposalId.slice(2) : proposalId;
  if (id.length <= 12) return `0x${id}`;
  return `0x${id.slice(0, 6)}...${id.slice(-6)}`;
}

function formatFee(fee: number): string {
  if (fee >= 1e8) return `${(fee / 1e8).toFixed(2)} CKB`;
  if (fee >= 1e4) return `${(fee / 1e4).toFixed(0)}k shannon`;
  return `${fee} shannon`;
}

function formatSize(size: number): string {
  if (size >= 1000) return `${(size / 1000).toFixed(1)}KB`;
  return `${size}B`;
}

export function ProposalsContainer({ proposals, totalCount }: ProposalsContainerProps) {
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
        {proposals.length === 0 ? (
          <div className="flex h-full min-h-[140px] w-full items-center justify-center text-xs text-slate-600 sm:min-h-[180px]">
            No proposed txs
          </div>
        ) : (
          <div className="flex h-full min-h-[140px] flex-wrap content-start gap-1 overflow-y-auto sm:min-h-[180px]">
            {proposals.map((proposal) => (
              <span
                key={proposal.proposalId}
                className="group relative inline-block rounded bg-amber-600/30 px-1.5 py-0.5 font-mono text-[9px] text-amber-300 sm:text-[10px]"
                title={
                  proposal.fee != null && proposal.size != null
                    ? `Fee: ${formatFee(proposal.fee)} | Size: ${formatSize(proposal.size)} | Expires in ${proposal.blocksUntilExpiry} blocks`
                    : `Expires in ${proposal.blocksUntilExpiry} blocks`
                }
              >
                {formatShortId(proposal.proposalId)}
                {proposal.feeRate != null && (
                  <span className="ml-1 text-amber-400/60">{proposal.feeRate.toFixed(1)}</span>
                )}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
