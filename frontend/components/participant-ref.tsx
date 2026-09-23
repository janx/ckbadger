'use client';

import { Address } from '@/components/ui/address';
import type { ParticipantRef } from '@/lib/api';
import { cn } from '@/lib/utils';

function truncateHex(value: string, startChars = 10, endChars = 6): string {
  return value.length > startChars + endChars
    ? `${value.slice(0, startChars)}…${value.slice(-endChars)}`
    : value;
}

/**
 * One transaction party.
 *
 * Resolved: the address, linked like any other. Unresolved: the 20-byte
 * lock-hash prefix the protocol named, marked as such and NOT linked — no page
 * exists for a party we cannot name, and inventing one would be a lie.
 */
export function ParticipantRefView({
  participant,
  className,
}: {
  participant: ParticipantRef;
  className?: string;
}) {
  const roles = participant.roles.length > 0 ? participant.roles.join(', ') : null;

  if (participant.address) {
    return (
      <span className={cn('inline-flex items-center gap-1.5', className)}>
        <Address address={participant.address} />
        {roles && <span className="text-text-dim font-mono text-[10px]">{roles}</span>}
      </span>
    );
  }

  const prefix = participant.lockHashPrefix ?? '';
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      <span className="text-text-dim font-mono text-sm" title={prefix}>
        {truncateHex(prefix)}
      </span>
      <span className="text-text-dim border-border rounded border px-1 font-mono text-[10px] uppercase">
        unresolved
      </span>
      {roles && <span className="text-text-dim font-mono text-[10px]">{roles}</span>}
    </span>
  );
}
