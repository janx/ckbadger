'use client';

import type { ReactNode } from 'react';
import Link, { type LinkLook } from '@/components/ui/link';
import { ShortId } from '@/components/ui/short-id';
import { shortenAddress, type ShortSize } from '@/lib/shorten';

/**
 * The one way a miner is named (round 3, contracts R-C · R-12): by the pool `pools.toml` names for
 * its cellbase, else by its address at the size of the place it stands in (U-09; it was the
 * address's last six characters alone, a form of its own) — never a guessed pool. The address
 * stays in the title, after the pool's name whole (see `minerTitle`).
 */
export function minerName(address: string, pool: string | null, size: ShortSize): string {
  return pool ?? shortenAddress(address, size);
}

/**
 * A miner link's title: the address, and before it the pool's name whole where the pool names the
 * miner (lane P1 follow-up; the F3-e note: the title was the address alone, so a pool name that a
 * narrow place cuts, the blocks list's truncated cell, showed nowhere whole).
 */
function minerTitle(address: string, pool: string | null): string {
  return pool === null ? address : `${pool} · ${address}`;
}

/**
 * A miner linked to its address. `unnamed` replaces the tail form where a page shows the address
 * itself for an unnamed miner (blocks list, block page). `look` is the link's job, which its caller
 * states (lane P4: the fork pages write a miner inside a line or a cell, `inline`; a row's key,
 * `key`), drawn by the owner of the looks with no ink of its own. It is required (lane
 * cd-fixes-controls, review-p-tests SHOULD-4): the bright ink a caller that stated none drew went,
 * with no caller left that states none.
 */
export function MinerLink({
  address,
  pool,
  size,
  unnamed,
  className,
  look,
}: {
  address: string;
  pool: string | null;
  /** The size of an unnamed miner's address. */
  size: ShortSize;
  unnamed?: ReactNode;
  className?: string;
  look: Extract<LinkLook, 'key' | 'inline'>;
}) {
  if (pool === null && unnamed !== undefined) return <>{unnamed}</>;
  return (
    <Link
      href={`/address/${address}`}
      look={look}
      className={className}
      title={minerTitle(address, pool)}
      data-miner-pool={pool ?? undefined}
    >
      {pool ?? <ShortId value={address} kind="address" size={size} />}
    </Link>
  );
}
