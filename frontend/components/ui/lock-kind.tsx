'use client';

import Link from '@/components/ui/link';
import { cn, truncateHash } from '@/lib/utils';

/**
 * Round 3 (contracts R-A · R-2): an owner's lock kind as the API names it — the scripts registry's
 * name, else the lock's code hash (never a guessed kind). Nothing when the lock is not indexed.
 * The default form is dim small caps (material pass v2, move 5); a row chip's classes replace it.
 *
 * U-15: a lock the registry does not name is named by what is known, "lock 0x9b9b9b...9b9b": the
 * word and its code hash, shortened, linking to the script page of that code hash (never "Lock
 * kind unavailable", an internal state). Either form is one unit (`whitespace-nowrap`).
 */
export function LockKind({
  lockKind,
  lockCodeHash,
  className,
}: {
  lockKind: string | null;
  lockCodeHash: string | null;
  /** Table-rows L2-4: e.g. `rowTagClass()` to draw it as a row's chip; default dim small caps. */
  className?: string;
}) {
  if (lockCodeHash === null) {
    if (lockKind !== null) throw new Error(`lock kind ${lockKind} without its lock code hash`);
    return null;
  }
  return (
    <span
      className={cn(
        className ? 'whitespace-nowrap' : 'text-text-dim small-caps whitespace-nowrap lowercase',
        className
      )}
      title={lockCodeHash}
      data-lock-kind
    >
      {lockKind ?? (
        <>
          lock{' '}
          <Link
            href={`/script/${lockCodeHash}`}
            title={lockCodeHash}
            className="hover:text-jade transition-colors"
          >
            {truncateHash(lockCodeHash, 8, 4)}
          </Link>
        </>
      )}
    </span>
  );
}
