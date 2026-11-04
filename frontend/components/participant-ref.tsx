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
 * the standalone form. `quiet` draws a compact address in the plain ink instead
 * of jade, for rows where colour is kept for the amounts (the homepage feed).
 * `linked={false}` (consistency C3, compact only) writes the address or lock hash
 * as dim text with the whole value in its hover, underlined jade (`.this-address`,
 * material pass v2 move 5): the party a page is about is never a link to that
 * same page.
 * In a column too narrow for the address and its roles, the roles wrap under the
 * address rather than being clipped ("ow" for owner_from in a 136 px track).
 */
export function ParticipantRefView({
  participant,
  compact = false,
  quiet = false,
  linked = true,
  className,
}: {
  participant: ParticipantRef;
  compact?: boolean;
  quiet?: boolean;
  /** Consistency C3: false writes a compact party as text (the page's own party). */
  linked?: boolean;
  className?: string;
}) {
  if (!linked && !compact)
    throw new Error('ParticipantRefView: linked={false} is the compact activity-row form only');
  const roles = participant.roles.length > 0 ? participant.roles.join(', ') : null;
  const address = participant.address;

  if (address !== null) {
    const isCkbAddr = address.startsWith('ckb1') || address.startsWith('ckt1');
    return (
      <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
        {!linked ? (
          <span
            className="this-address text-text-dim text-small shrink-0 font-mono"
            title={address}
          >
            {compactLabel(address)}
          </span>
        ) : compact ? (
          <Link
            href={`/address/${address}`}
            className={cn(
              'text-small shrink-0 font-mono transition-colors',
              quiet
                ? 'text-text hover:text-jade'
                : isCkbAddr
                  ? 'text-jade/80 hover:text-jade'
                  : 'text-text-dim hover:text-jade'
            )}
            title={address}
            onClick={(e) => e.stopPropagation()}
          >
            {compactLabel(address)}
          </Link>
        ) : (
          <Address address={address} />
        )}
        {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
      </span>
    );
  }

  const lockHash = participant.lockHash;
  if (lockHash !== null) {
    if (!linked)
      return (
        <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
          <span
            className="this-address text-text-dim text-small shrink-0 font-mono"
            title={lockHash}
          >
            {compactLabel(lockHash)}
          </span>
          {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
        </span>
      );
    return (
      <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
        <Link
          href={`/address/${lockHash}`}
          className={cn(
            'text-text-dim hover:text-jade font-mono transition-colors',
            compact ? 'text-small shrink-0' : 'text-body'
          )}
          title={lockHash}
          onClick={(e) => e.stopPropagation()}
        >
          {compact ? compactLabel(lockHash) : truncateHash(lockHash, 10, 6)}
        </Link>
        {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
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
    <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
      <span
        className={cn('text-text-dim font-mono', compact ? 'text-small' : 'text-body')}
        title={prefix}
      >
        {compact ? compactLabel(prefix) : truncateHash(prefix, 10, 6)}
      </span>
      <span
        className={cn(
          'eyebrow text-text-dim/70 text-meta font-mono',
          !compact && 'border-border rounded border px-1'
        )}
      >
        unresolved
      </span>
      {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
    </span>
  );
}
