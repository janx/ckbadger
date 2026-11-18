'use client';

import { Badge } from '@/components/ui/page-header';
import { useClockText } from '@/components/ui/utc8-date';
import type { PoolInterpretation, PoolStatus } from '@/lib/api';
import { ageInOneUnit, CLOCK_BEHIND } from '@/lib/format-age';

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

/**
 * Replaces the block time on an unconfirmed row. A pool transaction has no
 * block time; how long it has waited is the fact that exists, and it grows
 * while the row is shown: the wait reads the page's one clock (lane E4).
 */
export function TimeInPool({ since, className }: { since: string; className?: string }) {
  // the one age form (lib/format-age.ts); a time further ahead than consensus allows says so
  const wait = useClockText((now) => {
    const age = ageInOneUnit(since, now);
    return age === null ? CLOCK_BEHIND : `in pool for ${age}`;
  });
  return <span className={className}>{wait}</span>;
}

function reasonText(reason: { code: string; detail?: string }): string {
  switch (reason.code) {
    case 'unresolved_input':
      return reason.detail
        ? `input not yet resolvable (${reason.detail})`
        : 'an input is not yet resolvable';
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
    <span className={className ?? 'text-warning text-meta font-mono'}>
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
    // a sentence that explains why the list is short: the reading ink (R3)
    <div className="border-base-border bg-base-surface/50 text-text text-small border-b px-4 py-2 font-mono">
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
    // a sentence that explains why the list is short: the reading ink (R3)
    <div className="border-base-border bg-base-surface/50 text-text text-small border-b px-4 py-2 font-mono">
      Pool view unavailable — the node could not be reached, so unconfirmed transactions are not
      shown. Confirmed history below is unaffected.
    </div>
  );
}
