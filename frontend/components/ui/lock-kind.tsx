'use client';

import { truncateHash } from '@/lib/utils';

/**
 * Round 3 (contracts R-A · R-2): an owner's lock kind as the API names it — the scripts registry's
 * name, else the lock's code hash (never a guessed kind). Nothing when the lock is not indexed.
 */
export function LockKind({
  lockKind,
  lockCodeHash,
}: {
  lockKind: string | null;
  lockCodeHash: string | null;
}) {
  if (lockCodeHash === null) {
    if (lockKind !== null) throw new Error(`lock kind ${lockKind} without its lock code hash`);
    return null;
  }
  return (
    <span className="text-text-dim whitespace-nowrap" title={lockCodeHash} data-lock-kind>
      {lockKind ?? truncateHash(lockCodeHash, 8, 4)}
    </span>
  );
}
