import Link from 'next/link';

import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { formatCkbCompact, formatNumber } from '@/lib/utils';
import { formatStorageTier } from '@/lib/nft-utils';

interface NftCollectionStatCardsProps {
  totalCount: number;
  totalLabel?: string;
  liveCount?: number;
  liveCapacity: string | null | undefined;
  liveOccupiedCapacity: string | null | undefined;
  createdAtBlock?: number;
  storageTier?: string;
  storageOnchainRatio?: string;
}

function parseShannons(value: string | null | undefined): bigint | null {
  if (!value) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

export function NftCollectionStatCards({
  totalCount,
  totalLabel = 'Total NFTs',
  liveCount,
  liveCapacity,
  liveOccupiedCapacity,
  createdAtBlock,
  storageTier,
  storageOnchainRatio,
}: NftCollectionStatCardsProps) {
  const capacity = parseShannons(liveCapacity);
  const occupied = parseShannons(liveOccupiedCapacity);
  const occupationPercent =
    capacity && occupied && capacity > BigInt(0)
      ? (Number((occupied * BigInt(10000)) / capacity) / 100).toFixed(2)
      : null;
  const compactCapacity = capacity ? `${formatCkbCompact(capacity).value} CKB` : '--';
  const compactOccupied = occupied ? `${formatCkbCompact(occupied).value} CKB` : '--';
  const showLiveCount = liveCount !== undefined && liveCount !== totalCount;

  return (
    <div className="mb-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
            {totalLabel}
          </div>
          <div className="text-amber text-2xl font-semibold tabular-nums">
            {formatNumber(totalCount)}
          </div>
          <div className="font-mono text-xs text-slate-500">Full collection supply</div>
        </TerminalPanelContent>
      </TerminalPanel>

      {showLiveCount && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
              Live Items
            </div>
            <div className="text-terminal-green text-2xl font-semibold tabular-nums">
              {formatNumber(liveCount)}
            </div>
            <div className="font-mono text-xs text-slate-500">Currently on-chain</div>
          </TerminalPanelContent>
        </TerminalPanel>
      )}

      {storageTier && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
              Storage Integrity
            </div>
            <div className="text-terminal-green text-base font-semibold">
              {formatStorageTier(storageTier)}
            </div>
            {storageOnchainRatio && (
              <div className="font-mono text-xs text-slate-400">
                On-chain ratio: {(Number(storageOnchainRatio) * 100).toFixed(2)}%
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      )}

      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
            Live Capacity
          </div>
          <div className="font-mono text-lg text-white">{compactCapacity}</div>
          <div className="font-mono text-xs text-slate-500">Total live CKB in this collection</div>
        </TerminalPanelContent>
      </TerminalPanel>

      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
            Occupied Capacity
          </div>
          <div className="font-mono text-lg text-white">{compactOccupied}</div>
          <div className="font-mono text-xs text-slate-500">
            Occupied Ratio: {occupationPercent ? `${occupationPercent}%` : '--'}
          </div>
        </TerminalPanelContent>
      </TerminalPanel>

      {createdAtBlock !== undefined && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
              Created At
            </div>
            <Link
              href={`/blocks/${createdAtBlock}`}
              className="text-terminal-green font-mono text-lg hover:underline"
            >
              #{formatNumber(createdAtBlock)}
            </Link>
            <div className="font-mono text-xs text-slate-500">Genesis block of this collection</div>
          </TerminalPanelContent>
        </TerminalPanel>
      )}
    </div>
  );
}
