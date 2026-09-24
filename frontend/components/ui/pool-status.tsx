'use client';

import { Badge } from '@/components/ui/page-header';
import type { PoolInterpretation, PoolStatus } from '@/lib/api';

/**
 * A transaction in the node's pool is a proposed transition that proof-of-work
 * has not confirmed. Everything in this file exists to keep that visible: it is
 * shown, and it is never shown as if it were settled.
 */
export function poolStatusLabel(status: PoolStatus): string {
  switch (status) {
    case 'pending':
      return 'Pending';
    case 'proposed':
      return 'Proposed';
    case 'committed_awaiting_index':
      // The node has it in a block; this explorer has not indexed it yet.
      return 'Confirming';
  }
}

export function PoolStatusBadge({ status }: { status: PoolStatus }) {
  return (
    <Badge variant={status === 'committed_awaiting_index' ? 'blue' : 'gold'}>
      {poolStatusLabel(status)}
    </Badge>
  );
}

/** How long the node has held this transaction, e.g. `5m`. */
export function formatPoolDuration(since: string): string {
  const seconds = Math.max(0, Math.floor((Date.now() - new Date(since).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

/**
 * Replaces the block time on an unconfirmed row. A pool transaction has no
 * block time; how long it has waited is the fact that exists.
 */
export function TimeInPool({ since, className }: { since: string; className?: string }) {
  return <span className={className}>in pool for {formatPoolDuration(since)}</span>;
}

function reasonText(reason: { code: string; detail?: string }): string {
  switch (reason.code) {
    case 'unresolved_input':
      return reason.detail
        ? `input not yet resolvable (${reason.detail})`
        : 'an input is not yet resolvable';
    case 'dao_compensation_unavailable':
      return 'DAO compensation not yet known';
    default:
      // An unrecognised reason is shown verbatim rather than dropped: the
      // point of the field is that nothing goes unexplained.
      return reason.detail ? `${reason.code} (${reason.detail})` : reason.code;
  }
}

/**
 * Says which layer of the interpretation could not be read, and why. Renders
 * nothing when the interpretation is complete.
 */
export function PoolInterpretationNotice({
  interpretation,
  className,
}: {
  interpretation?: PoolInterpretation;
  className?: string;
}) {
  if (!interpretation || interpretation.status !== 'partial') return null;
  const reasons = interpretation.reasons ?? [];
  if (reasons.length === 0) return null;

  return (
    <span className={className ?? 'text-warning font-mono text-[10px]'}>
      partial: {reasons.map(reasonText).join('; ')}
    </span>
  );
}

/**
 * Shown when the list's unconfirmed segment is capped: some of this address's
 * pool transactions are not among the rows shown.
 */
export function PoolTruncatedNotice() {
  return (
    <div className="border-base-border bg-base-surface/50 text-text-dim border-b px-4 py-2 font-mono text-xs">
      More unconfirmed transactions than shown.
    </div>
  );
}

/**
 * Shown when the mirror cannot reach the node. An empty pool segment would read
 * as "nothing unconfirmed", which is a claim we cannot make.
 */
export function PoolUnavailableNotice() {
  return (
    <div className="border-base-border bg-base-surface/50 text-text-dim border-b px-4 py-2 font-mono text-xs">
      Pool view unavailable — the node could not be reached, so unconfirmed transactions are not
      shown. Confirmed history below is unaffected.
    </div>
  );
}
