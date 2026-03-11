import Link from '@/components/ui/link';

import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { formatNumber } from '@/lib/utils';

interface IdentityActivityCardProps {
  txHash: string;
  blockNumber: number;
  txIndex?: number;
  timestamp?: string;
  actions: string[];
  /** Optional transform applied to each action label before display (e.g. burn->recycled). */
  normalizeAction?: (action: string) => string;
  /** When true, actions render as colored Badge components instead of plain text. */
  badgeActions?: boolean;
}

function actionBadgeVariant(action: string): 'green' | 'red' | 'blue' | 'neutral' {
  if (action === 'mint') return 'green';
  if (action === 'burn' || action === 'recycle') return 'red';
  if (action === 'renew') return 'blue';
  return 'neutral';
}

export function IdentityActivityCard({
  txHash,
  blockNumber,
  txIndex,
  timestamp,
  actions,
  normalizeAction,
  badgeActions = false,
}: IdentityActivityCardProps) {
  const displayActions = normalizeAction ? actions.map(normalizeAction) : actions;

  return (
    <div className="border-base-border bg-base-surface/40 space-y-2 rounded border p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-text-dim font-mono text-xs">
          Block{' '}
          <Link href={`/blocks/${blockNumber}`} className="text-gold hover:underline">
            #{formatNumber(blockNumber)}
          </Link>
          {txIndex !== undefined && (
            <>
              <span className="text-text-dim mx-1">&bull;</span>
              Tx Index {txIndex}
            </>
          )}
        </div>
        {badgeActions ? (
          <div className="flex flex-wrap gap-1.5">
            {actions.map((action) => (
              <Badge
                key={`${txHash}-${txIndex ?? 0}-${action}`}
                variant={actionBadgeVariant(action)}
              >
                {action}
              </Badge>
            ))}
          </div>
        ) : (
          <div className="text-text font-mono text-xs">{displayActions.join(', ')}</div>
        )}
      </div>
      <Link href={`/tx/${txHash}`} className="text-text block font-mono text-xs hover:underline">
        <HexDisplay value={txHash} size="sm" startChars={14} endChars={10} />
      </Link>
      {timestamp && <div className="text-text-dim font-mono text-xs">Timestamp: {timestamp}</div>}
    </div>
  );
}
