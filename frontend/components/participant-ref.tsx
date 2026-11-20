'use client';

import { Address } from '@/components/ui/address';
import Link, { type LinkLook } from '@/components/ui/link';
import type { ParticipantRef } from '@/lib/api';
import { ShortId } from '@/components/ui/short-id';
import type { ShortSize } from '@/lib/shorten';
import { cn } from '@/lib/utils';

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
 * same page. It stays quiet under readability ruling R3 as a unit that repeats
 * down a column does: it is the page's own party, written on every row. Every
 * other party is a value or a link, the reading ink (or jade-dim, a CKB address
 * in a row); its roles and "unresolved" are labels, quiet.
 * In a column too narrow for the address and its roles, the roles wrap under the
 * address rather than being clipped ("ow" for owner_from in a 136 px track).
 * `size` is the size of the row the party stands in (U-09), for its address or lock hash alike.
 * `look` is where it stands, which its caller says (components/ui/link.tsx), as an `Address`'s: a
 * row's key (`key`) or a name inside a line (`inline`); the link then draws that look and writes no
 * ink by hand. A caller that states none draws the inks above (lane P1 follow-up: the chain pages
 * state theirs; the feeds' call sites are the lists lane's).
 */
export function ParticipantRefView({
  participant,
  size,
  compact = false,
  quiet = false,
  linked = true,
  look,
  className,
}: {
  participant: ParticipantRef;
  size: ShortSize;
  look?: Extract<LinkLook, 'key' | 'inline'>;
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
    // the full form of an address is an Address, which states its look (lane cd-fixes-controls,
    // review-p-tests SHOULD-4); the compact form's feeds may still state none
    if (!compact && look === undefined)
      throw new Error('ParticipantRefView: the full form is an Address, which states its look');
    const isCkbAddr = address.startsWith('ckb1') || address.startsWith('ckt1');
    return (
      <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
        {!linked ? (
          <span
            className="this-address text-text-dim text-small shrink-0 font-mono"
            title={address}
          >
            <ShortId value={address} kind="address" size={size} />
          </span>
        ) : compact ? (
          <Link
            href={`/address/${address}`}
            look={look}
            className={cn(
              'text-small shrink-0 font-mono transition-colors',
              look === undefined &&
                (quiet
                  ? 'text-text hover:text-jade'
                  : isCkbAddr
                    ? 'text-jade-dim hover:text-jade'
                    : 'text-text hover:text-jade')
            )}
            title={address}
            onClick={(e) => e.stopPropagation()}
          >
            <ShortId value={address} kind="address" size={size} />
          </Link>
        ) : (
          <Address address={address} size={size} look={look!} />
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
            <ShortId value={lockHash} kind="hash" size={size} />
          </span>
          {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
        </span>
      );
    return (
      <span className={cn('inline-flex flex-wrap items-center gap-x-1.5', className)}>
        <Link
          href={`/address/${lockHash}`}
          look={look}
          className={cn(
            look === undefined && 'text-text hover:text-jade',
            'font-mono transition-colors',
            compact ? 'text-small shrink-0' : 'text-body'
          )}
          title={lockHash}
          onClick={(e) => e.stopPropagation()}
        >
          <ShortId value={lockHash} kind="hash" size={size} />
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
        className={cn('text-text font-mono', compact ? 'text-small' : 'text-body')}
        title={prefix}
      >
        <ShortId value={prefix} kind="hash" size={size} />
      </span>
      <span
        className={cn(
          'eyebrow text-text-dim text-meta font-mono',
          !compact && 'border-border border px-1'
        )}
      >
        unresolved
      </span>
      {roles && <span className="text-text-dim text-meta font-mono">{roles}</span>}
    </span>
  );
}
