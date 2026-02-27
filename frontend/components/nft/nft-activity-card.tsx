import Link from 'next/link';

import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { formatNumber } from '@/lib/utils';

interface NftActivityCardProps {
  txHash: string;
  blockNumber: number;
  txIndex?: number;
  timestamp?: string;
  actions: string[];
  /** Optional transform applied to each action label before display (e.g. burn→recycled). */
  normalizeAction?: (action: string) => string;
  /** When true, actions render as colored Badge components instead of plain text. */
  badgeActions?: boolean;
}

function actionBadgeVariant(action: string): 'green' | 'red' | 'neutral' {
  if (action === 'mint') return 'green';
  if (action === 'burn') return 'red';
  return 'neutral';
}

export function NftActivityCard({
  txHash,
  blockNumber,
  txIndex,
  timestamp,
  actions,
  normalizeAction,
  badgeActions = false,
}: NftActivityCardProps) {
  const displayActions = normalizeAction ? actions.map(normalizeAction) : actions;

  return (
    <div className="space-y-2 rounded border border-slate-800 bg-slate-900/40 p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="font-mono text-xs text-slate-400">
          Block{' '}
          <Link href={`/blocks/${blockNumber}`} className="text-terminal-green hover:underline">
            #{formatNumber(blockNumber)}
          </Link>
          {txIndex !== undefined && (
            <>
              <span className="mx-1 text-slate-500">•</span>
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
          <div className="font-mono text-xs text-slate-300">{displayActions.join(', ')}</div>
        )}
      </div>
      <Link
        href={`/tx/${txHash}`}
        className="block font-mono text-xs text-slate-300 hover:underline"
      >
        <HexDisplay value={txHash} color="accent" size="sm" startChars={14} endChars={10} />
      </Link>
      {timestamp && <div className="font-mono text-xs text-slate-500">Timestamp: {timestamp}</div>}
    </div>
  );
}
