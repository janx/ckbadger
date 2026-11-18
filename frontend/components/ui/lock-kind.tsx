'use client';

import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import type { ParticipantRef } from '@/lib/api';
import { addressScript, isDefaultLock } from '@/lib/lock-script';
import type { ShortSize } from '@/lib/shorten';

/** A lock's name in its default form: plain words in the quiet ink, at the size of its line. */
const LOCK_WORDS = 'text-text-dim';

/**
 * Round 3 (contracts R-A · R-2): an owner's lock kind as the API names it — the scripts registry's
 * name, else the lock's code hash (never a guessed kind). Nothing when the lock is not indexed.
 *
 * Readability 2026-11-18 (R9, lane B4): nothing for the default lock either. A shortened address is
 * `ckb1…` and its tail, so the lock is named beside it only when it is not the default lock, and no
 * name means the default lock. The default form is plain words in the quiet ink at the size of the
 * line it stands in (it was dim small caps, under 10 px at a 12 px line); a row chip's classes
 * replace it.
 *
 * U-15: a lock the registry does not name is named by what is known, "lock 0x9b9b…9b9b": the
 * word and its code hash, shortened at the size of the place it stands in (U-09), linking to the
 * script page of that code hash (never "Lock kind unavailable", an internal state). The unregistered
 * form is one unit (`whitespace-nowrap`); a registered name is words.
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
  /** Table-rows L2-4: e.g. `rowTagClass()` to draw it as a row's chip; default plain dim words. */
  className?: string;
}) {
  if (lockCodeHash === null) {
    if (lockKind !== null) throw new Error(`lock kind ${lockKind} without its lock code hash`);
    return null;
  }
  if (isDefaultLock(lockCodeHash)) return null;
  return (
    <span
      // A registered name is words, never kept whole (A4 Chrome sweep); the unregistered
      // "lock <code hash>" is a label with its id, one unit. The lock's kind is a label beside
      // its party: quiet by rule (R3)
      className={cn(!className && LOCK_WORDS, lockKind === null && 'whitespace-nowrap', className)}
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

/**
 * A transaction party's lock as a row writes it beside the party. A feed's party carries the
 * registry's name of its lock (`lockKind`) and its address, not the lock's code hash: the code hash
 * is read off the address (`lib/lock-script.ts`). A party with no address (its lock too long for
 * one, or a protocol's prefix not resolved) is written by its lock hash; it keeps the name the API
 * sends, which is never the default lock's for a lock that fits an address.
 */
export type PartyLock =
  | { lockKind: string | null; lockCodeHash: string }
  | { lockKind: string; lockCodeHash: null };

/** A party's lock beside it, or null when the row writes none: the default lock, or nothing known. */
export function partyLock(party: ParticipantRef): PartyLock | null {
  const lockKind = party.lockKind ?? null;
  if (party.address === null) return lockKind === null ? null : { lockKind, lockCodeHash: null };
  const { codeHash } = addressScript(party.address);
  return isDefaultLock(codeHash) ? null : { lockKind, lockCodeHash: codeHash };
}

/**
 * `partyLock` drawn: `LockKind` (its name, its code hash in the hover, or "lock <code hash>"), or a
 * name read without its code hash in the same words with no hover.
 */
export function PartyLockKind({ lock, size }: { lock: PartyLock; size: ShortSize }) {
  if (lock.lockCodeHash === null)
    return (
      <span className={LOCK_WORDS} data-lock-kind>
        {lock.lockKind}
      </span>
    );
  return <LockKind lockKind={lock.lockKind} lockCodeHash={lock.lockCodeHash} size={size} />;
}
