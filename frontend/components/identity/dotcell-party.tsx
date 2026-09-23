'use client';

import Link from '@/components/ui/link';
import { Address } from '@/components/ui/address';
import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { cn, truncateHash } from '@/lib/utils';

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
  /** Shorten long values — list rows want this, detail fields do not. */
  truncate?: boolean;
  testId?: string;
  className?: string;
}

export function DotCellParty({ party, truncate = false, testId, className }: DotCellPartyProps) {
  return (
    <span
      data-testid={testId}
      className={cn('inline-flex min-w-0 flex-wrap items-center gap-2', className)}
    >
      <PartyIdentity party={party} truncate={truncate} />
      {party.scriptName && <Badge variant="gray">{party.scriptName}</Badge>}
    </span>
  );
}

function PartyIdentity({ party, truncate }: { party: PartyDisplay; truncate: boolean }) {
  // An address is the most specific thing the lock resolves to.
  if (party.address) {
    return <Address address={party.address} truncate={truncate} />;
  }

  // Resolved to a lock hash the address page accepts, but the script encodes
  // to no address (an unencodable lock).
  if (party.lockHash) {
    return (
      <Link href={`/address/${party.lockHash}`} className="hover:underline">
        <HexDisplay value={party.lockHash} truncate={truncate} size="sm" />
      </Link>
    );
  }

  // Only a 20-byte prefix: no link, and the whole prefix stays readable in the
  // tooltip so it can be copied and searched.
  if (party.hashPrefix) {
    return (
      <>
        <span className="text-text-bright font-mono text-sm" title={party.hashPrefix}>
          {truncate ? truncateHash(party.hashPrefix, 12, 10) : party.hashPrefix}
        </span>
        <Badge variant="gray">unresolved</Badge>
      </>
    );
  }

  return <span className="text-text-dim font-mono text-sm">Unavailable</span>;
}
