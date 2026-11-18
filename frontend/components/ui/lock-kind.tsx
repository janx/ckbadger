'use client';

import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import type { ShortSize } from '@/lib/shorten';

/**
 * Round 3 (contracts R-A · R-2): an owner's lock kind as the API names it — the scripts registry's
 * name, else the lock's code hash (never a guessed kind). Nothing when the lock is not indexed.
 * The default form is dim small caps (material pass v2, move 5); a row chip's classes replace it.
 *
 * U-15: a lock the registry does not name is named by what is known, "lock 0x9b9b…9b9b": the
 * word and its code hash, shortened at the size of the place it stands in (U-09), linking to the
 * script page of that code hash (never "Lock kind unavailable", an internal state). Either form is
 * one unit (`whitespace-nowrap`).
 */
export function LockKind({
  lockKind,
  lockCodeHash,
  size,
  className,
}: {
  lockKind: string | null;
  lockCodeHash: string | null;
  /** The size of the code hash of a lock the registry does not name. */
  size: ShortSize;
  /** Table-rows L2-4: e.g. `rowTagClass()` to draw it as a row's chip; default dim small caps. */
  className?: string;
}) {
  if (lockCodeHash === null) {
    if (lockKind !== null) throw new Error(`lock kind ${lockKind} without its lock code hash`);
    return null;
  }
  return (
    <span
      // A registered name is words, never kept whole (A4 Chrome sweep); the unregistered
      // "lock <code hash>" is a label with its id, one unit. The lock's kind is a label beside
      // its party: quiet by rule (R3)
      className={cn(
        !className && 'text-text-dim small-caps lowercase',
        lockKind === null && 'whitespace-nowrap',
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
            <ShortId value={lockCodeHash} kind="hash" size={size} />
          </Link>
        </>
      )}
    </span>
  );
}
