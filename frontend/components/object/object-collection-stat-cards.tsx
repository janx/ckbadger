import Link from '@/components/ui/link';

import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { formatNumber } from '@/lib/utils';
import { formatCompositionTier } from '@/lib/asset-utils';
import { formatDecimal } from '@/lib/exact-integer';
import { CompositionTierTooltip, compositionTierCardStyle } from '@/components/object/storage-tier';

interface ObjectCollectionStatCardsProps {
  totalCount: number;
  totalLabel?: string;
  liveCount?: number;
  createdAtBlock?: number;
  compositionTier?: string;
  storageOnchainRatio?: string;
}

export function ObjectCollectionStatCards({
  totalCount,
  totalLabel = 'Total Objects',
  liveCount,
  createdAtBlock,
  compositionTier,
  storageOnchainRatio,
}: ObjectCollectionStatCardsProps) {
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

      {compositionTier && (
        <TerminalPanel variant="inset">
          <TerminalPanelContent className="space-y-2">
            <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
              Storage Integrity
            </div>
            <div className="flex items-center gap-1 text-base font-semibold">
              <span className={compositionTierCardStyle(compositionTier).text}>
                {formatCompositionTier(compositionTier)}
              </span>
              <CompositionTierTooltip tier={compositionTier} />
            </div>
            {storageOnchainRatio && (
              <div className="text-text-dim font-mono text-xs">
                On-chain ratio: {formatDecimal(storageOnchainRatio, 2, 100)}%
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      )}

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
