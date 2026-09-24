'use client';

import { Address } from '@/components/ui/address';
import Link from '@/components/ui/link';
import type { ParticipantRef } from '@/lib/api';
import { cn, truncateHash } from '@/lib/utils';

function compactLabel(value: string): string {
  if (value.startsWith('ckb1') || value.startsWith('ckt1')) {
    return `${value.slice(0, 8)}...${value.slice(-6)}`;
  }
  return truncateHash(value, 8, 6);
}

/**
 * One transaction party — the ONE place a party is rendered.
 *
 * It shows the most specific identity the API gave:
 * - an address, linked like any other;
 * - else a full lock hash (a lock whose script this store does not know, so it
 *   encodes to no address), linked to the address page by hash;
 * - else the 20-byte lock-hash prefix a protocol named, marked unresolved and
 *   NOT linked — no page exists for a party we cannot name, and inventing one
 *   would be a lie.
 * A party with none of the three is an API contract violation and throws.
 *
 * `compact` is the per-participant line inside an activity row; the default is
 * the standalone form.
 */
export function ParticipantRefView({
  participant,
  compact = false,
  className,
}: {
  participant: ParticipantRef;
  compact?: boolean;
  className?: string;
}) {
  const roles = participant.roles.length > 0 ? participant.roles.join(', ') : null;
  const address = participant.address;

  if (address !== null) {
    const isCkbAddr = address.startsWith('ckb1') || address.startsWith('ckt1');
    return (
      <span className={cn('inline-flex shrink-0 items-center gap-1.5', className)}>
        {compact ? (
          <Link
            href={`/address/${address}`}
            className={cn(
              'shrink-0 font-mono text-xs transition-colors',
              isCkbAddr ? 'text-jade/80 hover:text-jade' : 'text-text-dim hover:text-aqua'
            )}
            title={address}
            onClick={(e) => e.stopPropagation()}
          >
            {compactLabel(address)}
          </Link>
        ) : (
          <Address address={address} />
        )}
        {roles && <span className="text-text-dim font-mono text-[10px]">{roles}</span>}
      </span>
    );
  }

  const lockHash = participant.lockHash;
  if (lockHash !== null) {
    return (
      <span className={cn('inline-flex shrink-0 items-center gap-1.5', className)}>
        <Link
          href={`/address/${lockHash}`}
          className={cn(
            'text-text-dim hover:text-aqua font-mono transition-colors',
            compact ? 'shrink-0 text-xs' : 'text-sm'
          )}
          title={lockHash}
          onClick={(e) => e.stopPropagation()}
        >
          {compact ? compactLabel(lockHash) : truncateHash(lockHash, 10, 6)}
        </Link>
        {roles && <span className="text-text-dim font-mono text-[10px]">{roles}</span>}
      </span>
    );
  }

  const prefix = participant.lockHashPrefix;
  if (prefix === null) {
    throw new Error(
      `Transaction party has no address, lock hash or lock-hash prefix (roles: ${roles ?? 'none'})`
    );
  }
  return (
    <span className={cn('inline-flex shrink-0 items-center gap-1.5', className)}>
      <span
        className={cn('text-text-dim font-mono', compact ? 'text-xs' : 'text-sm')}
        title={prefix}
      >
        {compact ? compactLabel(prefix) : truncateHash(prefix, 10, 6)}
      </span>
      <span
        className={cn(
          'text-text-dim/70 font-mono text-[10px] uppercase',
          !compact && 'border-border rounded border px-1'
        )}
      >
        unresolved
      </span>
      {roles && <span className="text-text-dim font-mono text-[10px]">{roles}</span>}
    </span>
  );
}
