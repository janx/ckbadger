import Link from '@/components/ui/link';

import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { formatCkbCompact, formatNumber } from '@/lib/utils';
import { formatStorageTier } from '@/lib/asset-utils';

interface ObjectCollectionStatCardsProps {
  totalCount: number;
  totalLabel?: string;
  liveCount?: number;
  ownedCapacity: string | null | undefined;
  ownedKnowledge: string | null | undefined;
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

export function ObjectCollectionStatCards({
  totalCount,
  totalLabel = 'Total Objects',
  liveCount,
  ownedCapacity,
  ownedKnowledge,
  createdAtBlock,
  storageTier,
  storageOnchainRatio,
}: ObjectCollectionStatCardsProps) {
  const capacity = parseShannons(ownedCapacity);
  const used = parseShannons(ownedKnowledge);
  const usedPercent =
    capacity && used && capacity > BigInt(0)
      ? (Number((used * BigInt(10000)) / capacity) / 100).toFixed(2)
      : null;
  const compactCapacity = capacity ? `${formatCkbCompact(capacity).value} CKB` : '--';
  const compactUsed = used ? `${formatCkbCompact(used).value} CKB` : '--';
  const showLiveCount = liveCount !== undefined && liveCount !== totalCount;

  return (
    <div className="mb-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
            {totalLabel}
          </div>
          <div className="text-warning text-2xl font-semibold tabular-nums">
            {formatNumber(totalCount)}
          </div>
          <div className="text-text-dim font-mono text-xs">Full collection supply</div>
        </TerminalPanelContent>
      </TerminalPanel>

      {showLiveCount && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
              Live Items
            </div>
            <div className="text-gold text-2xl font-semibold tabular-nums">
              {formatNumber(liveCount)}
            </div>
            <div className="text-text-dim font-mono text-xs">Currently on-chain</div>
          </TerminalPanelContent>
        </TerminalPanel>
      )}

      {storageTier && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
              Storage Integrity
            </div>
            <div className="text-gold text-base font-semibold">
              {formatStorageTier(storageTier)}
            </div>
            {storageOnchainRatio && (
              <div className="text-text-dim font-mono text-xs">
                On-chain ratio: {(Number(storageOnchainRatio) * 100).toFixed(2)}%
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      )}

      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
            Owned Capacity
          </div>
          <div className="text-text-bright font-mono text-lg">{compactCapacity}</div>
          <div className="text-text-dim font-mono text-xs">Total owned CKB in this collection</div>
        </TerminalPanelContent>
      </TerminalPanel>

      <TerminalPanel variant="inset">
        <TerminalPanelContent className="space-y-2">
          <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
            Common Knowledge Size
          </div>
          <div className="text-text-bright font-mono text-lg">{compactUsed}</div>
          <div className="text-text-dim font-mono text-xs">
            Common Knowledge Share: {usedPercent ? `${usedPercent}%` : '--'}
          </div>
        </TerminalPanelContent>
      </TerminalPanel>

      {createdAtBlock !== undefined && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
              Created At
            </div>
            <Link
              href={`/blocks/${createdAtBlock}`}
              className="text-gold font-mono text-lg hover:underline"
            >
              #{formatNumber(createdAtBlock)}
            </Link>
            <div className="text-text-dim font-mono text-xs">Genesis block of this collection</div>
          </TerminalPanelContent>
        </TerminalPanel>
      )}
    </div>
  );
}
