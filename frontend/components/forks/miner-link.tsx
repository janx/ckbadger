'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';

/**
 * The one way a miner is named (round 3, contracts R-C · R-12): by the pool `pools.toml` names for
 * its cellbase, else by its address tail — never a guessed pool. The address stays in the title.
 */
export function minerName(address: string, pool: string | null): string {
  return pool ?? `…${address.slice(-6)}`;
}

/**
 * A miner linked to its address. `unnamed` replaces the tail form where a page shows the address
 * itself for an unnamed miner (blocks list, block page).
 */
export function MinerLink({
  address,
  pool,
  unnamed,
}: {
  address: string;
  pool: string | null;
  unnamed?: ReactNode;
}) {
  if (pool === null && unnamed !== undefined) return <>{unnamed}</>;
  return (
    <Link
      href={`/address/${address}`}
      className="text-text-bright hover:text-jade"
      title={address}
      data-miner-pool={pool ?? undefined}
    >
      {minerName(address, pool)}
    </Link>
  );
}
