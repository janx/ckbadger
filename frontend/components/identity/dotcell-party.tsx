'use client';

import Link from '@/components/ui/link';
import { Address } from '@/components/ui/address';
import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import type { ShortSize } from '@/lib/shorten';

/**
 * A party of a `.cell` name as the chain names it.
 *
 * The chain stores an owner/manager as the FIRST 20 BYTES of a lock script
 * hash. The API resolves that prefix to a full lock hash only when exactly one
 * known lock matches it, so a prefix on its own identifies no address and must
 * never be rendered as one.
 */
export interface PartyDisplay {
  hashPrefix: string | null;
  lockHash: string | null;
  address: string | null;
  scriptName?: string | null;
}

interface DotCellPartyProps {
  party: PartyDisplay;
  /**
   * The size a list row shortens its values to (U-09); none in a detail field, which writes them
   * whole.
   */
  size?: ShortSize;
  testId?: string;
  className?: string;
}

export function DotCellParty({ party, size, testId, className }: DotCellPartyProps) {
  return (
    <span
      data-testid={testId}
      className={cn('inline-flex min-w-0 flex-wrap items-center gap-2', className)}
    >
      <PartyIdentity party={party} size={size} />
      {party.scriptName && <Badge variant="gray">{party.scriptName}</Badge>}
    </span>
  );
}

function PartyIdentity({ party, size }: { party: PartyDisplay; size: ShortSize | undefined }) {
  // An address is the most specific thing the lock resolves to.
  if (party.address) {
    return size ? (
      <Address address={party.address} size={size} />
    ) : (
      <Address address={party.address} truncate={false} />
    );
  }

  // Resolved to a lock hash the address page accepts, but the script encodes
  // to no address (an unencodable lock).
  if (party.lockHash) {
    return (
      <Link href={`/address/${party.lockHash}`} className="hover:underline">
        {size ? (
          <HexDisplay value={party.lockHash} size={size} textSize="sm" />
        ) : (
          <HexDisplay value={party.lockHash} truncate={false} textSize="sm" />
        )}
      </Link>
    );
  }

  // Only a 20-byte prefix: no link, and the whole prefix stays readable in the
  // tooltip so it can be copied and searched.
  if (party.hashPrefix) {
    return (
      <>
        <span className="text-text-bright text-body font-mono" title={party.hashPrefix}>
          {size ? <ShortId value={party.hashPrefix} kind="hash" size={size} /> : party.hashPrefix}
        </span>
        <Badge variant="gray">unresolved</Badge>
      </>
    );
  }

  return <span className="text-text-dim text-body font-mono">Unavailable</span>;
}
