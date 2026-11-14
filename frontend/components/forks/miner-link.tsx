'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ShortId } from '@/components/ui/short-id';
import { shortenAddress, type ShortSize } from '@/lib/shorten';
import { cn } from '@/lib/utils';

/**
 * The one way a miner is named (round 3, contracts R-C · R-12): by the pool `pools.toml` names for
 * its cellbase, else by its address at the size of the place it stands in (U-09; it was the
 * address's last six characters alone, a form of its own) — never a guessed pool. The address
 * stays in the title.
 */
export function minerName(address: string, pool: string | null, size: ShortSize): string {
  return pool ?? shortenAddress(address, size);
}

/**
 * A miner linked to its address. `unnamed` replaces the tail form where a page shows the address
 * itself for an unnamed miner (blocks list, block page). `className` lets a list row set it in
 * body ink, where its key cell is the one bright thing (table-rows T5); bright by default.
 */
export function MinerLink({
  address,
  pool,
  size,
  unnamed,
  className,
}: {
  address: string;
  pool: string | null;
  /** The size of an unnamed miner's address. */
  size: ShortSize;
  unnamed?: ReactNode;
  className?: string;
}) {
  if (pool === null && unnamed !== undefined) return <>{unnamed}</>;
  return (
    <Link
      href={`/address/${address}`}
      className={cn('text-text-bright hover:text-jade', className)}
      title={address}
      data-miner-pool={pool ?? undefined}
    >
      {pool ?? <ShortId value={address} kind="address" size={size} />}
    </Link>
  );
}
