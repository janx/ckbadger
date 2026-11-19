'use client';

import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import type { ParticipantRef } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { addressScript } from '@/lib/lock-script';
import { normalizeScriptRefHashType, type ScriptRefHashType } from '@/lib/script-ref';
import type { ShortSize } from '@/lib/shorten';
import { PartReadError, readPart } from '@/components/read-failure';
import type { ReactNode } from 'react';
import {
  lockPhrase,
  lockWords,
  namedLockWords,
  namedPartyLockFacts,
  type LockFacts,
  type LockPlace,
  type LockWords,
} from '@/lib/lock-words';

/** The decision's text half, said where the page draws it too (lib/lock-words.ts). */
export {
  lockNoun,
  lockWords,
  lockWordsText,
  namedLockWords,
  partyLockSentence,
  partyLockWordsText,
} from '@/lib/lock-words';
export type { LockWords } from '@/lib/lock-words';

/** A lock's name in its default form: plain words in the quiet ink, at the size of its line. */
const LOCK_WORDS = 'text-text-dim';
/**
 * The plain words of a column headed "Lock" (the default lock, a lock not indexed): the words a
 * column's tag holds, at its step and on its 16 px line, in the quiet ink and without its box (no
 * badge's weight for the common case), ending in the one ellipsis where the column is narrower.
 */
const COLUMN_WORDS = cn(LOCK_WORDS, 'text-meta min-w-0 truncate leading-4');

/**
 * The lock of an API row (a holder, an owner, a token's issuer, a supply event): the registry's name
 * and the lock's code hash as the API sends them (a name comes with its code hash, Round 3 R-2),
 * and where the row has an address, the lock read off it, which carries the hash type and decides.
 * A row that sends its lock's hash type (a DAO depositor's `lockHashType`; lane B4 third round) is
 * decided by it where it has no address; where it has one, the address must spell that hash type.
 */
export function rowLockFacts({
  lockKind,
  lockCodeHash,
  lockHashType,
  address,
}: {
  lockKind: string | null;
  lockCodeHash: string | null;
  /** The lock's hash type, where the row sends one. */
  lockHashType?: string | null;
  address: string | null;
}): LockFacts {
  if (lockCodeHash === null && lockKind !== null)
    throw new Error(`lock kind ${lockKind} without its lock code hash`);
  const hashType =
    lockHashType === undefined || lockHashType === null
      ? null
      : hashTypeOf(lockHashType, `lock ${lockCodeHash}`);
  if (address === null) return { name: lockKind, codeHash: lockCodeHash, hashType };
  const script = addressScript(address);
  if (lockCodeHash !== null && lockCodeHash.toLowerCase() !== script.codeHash)
    throw new Error(
      `address ${address} spells the lock code hash ${script.codeHash}, the row says ${lockCodeHash}`
    );
  if (hashType !== null && hashType !== script.hashType)
    throw new Error(
      `address ${address} spells the lock ${script.codeHash} by ${script.hashType}, the row says by ${hashType}`
    );
  return { name: lockKind, codeHash: script.codeHash, hashType: script.hashType };
}

/** A lock's hash type as the chain writes it; any other word is a contract break, named. */
function hashTypeOf(value: string, where: string): ScriptRefHashType {
  const hashType = normalizeScriptRefHashType(value);
  if (hashType === null)
    throw new Error(`${where}: lock hash type ${JSON.stringify(value)} is not a CKB hash type`);
  return hashType;
}

/**
 * A cell's lock (a transaction's input or output, a version of an item's cell): its script and the
 * registry's name for its code hash. Its owner's address, where it has one, is that script spelled
 * (lane B4 second fix round): an address that spells another lock (code hash, hash type or args)
 * is refused with `where` named, as is a hash type the chain does not have.
 */
export function cellLockFacts(
  cell: {
    lock: { codeHash: string; hashType: string; args: string };
    name: string | null;
    address: string | null;
  },
  where: string
): LockFacts {
  const { lock, name, address } = cell;
  const hashType = hashTypeOf(lock.hashType, where);
  if (address !== null) {
    const spelled = addressScript(address);
    if (
      spelled.codeHash !== lock.codeHash.toLowerCase() ||
      spelled.hashType !== hashType ||
      spelled.args !== lock.args.toLowerCase()
    )
      throw new Error(
        `${where}: address ${address} spells the lock ${spelled.codeHash} by ${spelled.hashType}, args ${spelled.args}; the cell's lock is ${lock.codeHash} by ${hashType}, args ${lock.args}`
      );
  }
  return { name, codeHash: lock.codeHash.toLowerCase(), hashType };
}

/**
 * A transaction party's lock: the registry's name the API sends and, where the party has an
 * address, the lock read off it (a feed's party carries no code hash). A party with no address (its
 * lock too long for one, or a protocol's prefix not resolved) is written by its lock hash and keeps
 * the name alone.
 */
export function partyLockFacts(party: ParticipantRef): LockFacts {
  return namedPartyLockFacts({ lockScriptName: party.lockKind ?? null, address: party.address });
}

/**
 * The lock of a version of an item's cell beside its owner (`cellLockFacts` off the cell's lock and
 * its owner's address, `lockWords` inline), or null where none is written: the default lock.
 */
export function cellOwnerLock(cell: ItemCellLock): LockWords | null {
  return lockWords(itemCellLockFacts(cell), 'inline');
}

/** What a version of an item's cell carries of its lock: its script, name, owner and out point. */
interface ItemCellLock {
  lock: { codeHash: string; hashType: string; args: string };
  lockName: string | null;
  ownerAddress: string;
  txHash: string;
  outputIndex: number;
}

/** A version of an item's cell's lock facts (`cellLockFacts`), refused with the cell named. */
function itemCellLockFacts(cell: ItemCellLock): LockFacts {
  return cellLockFacts(
    { lock: cell.lock, name: cell.lockName, address: cell.ownerAddress },
    `cell ${cell.txHash}:${cell.outputIndex}`
  );
}

/**
 * A cell owner's lock as a part of a note or a fact (`cellOwnerLock`, `LockWordsView`): null where
 * none is written, so a list leaves the part out; a lock the decision refuses fails this part in
 * its place, said with the refusal (ruling D7; lane B4 third round), never the page.
 */
export function cellOwnerLockPart(cell: ItemCellLock, className?: string): ReactNode {
  const read = readPart(() => cellOwnerLock(cell));
  if (read.error !== undefined)
    return (
      <PartReadError
        what="the owner’s lock"
        where={`cell ${cell.txHash}:${cell.outputIndex}`}
        error={read.error}
      />
    );
  return read.value && <LockWordsView words={read.value} size="row" className={className} />;
}

/**
 * The lock a version of an item's cell sits under, said in a sentence (`cellLockFacts` off the
 * cell's lock and its owner's address, `lockWords` in a sentence, `lockPhrase`): never silent, the
 * default lock by its name. A cell whose address spells another lock is refused with the cell named.
 */
export function cellLockSentence(cell: ItemCellLock, size: ShortSize): string {
  return lockPhrase(cellLockWords(cell), size);
}

/**
 * The words of the lock a version of an item's cell sits under, in a sentence (never null): for a
 * place that says them without an article ("Lock: Default Lock throughout."), `lockWordsText`.
 */
export function cellLockWords(cell: ItemCellLock): LockWords {
  return namedLockWords(itemCellLockFacts(cell), 'sentence');
}

/** An API row's lock where it stands (`rowLockFacts`, `lockWords`), or null where none is written. */
export function rowLock(
  row: Parameters<typeof rowLockFacts>[0],
  place: LockPlace
): LockWords | null {
  return lockWords(rowLockFacts(row), place);
}

/** A party's lock beside it (`lockWords` inline), or null where the row writes none. */
export function partyLock(party: ParticipantRef): LockWords | null {
  return lockWords(partyLockFacts(party), 'inline');
}

/**
 * A lock's words drawn (Round 3 R-2, U-15; R9): the registry's name, its code hash in the hover; a
 * lock the registry does not name, "lock 0x9b9b…9b9b", its code hash a link to its script page and
 * one unit (`whitespace-nowrap`), a registered name being words (A4 Chrome sweep); "by <hash type>"
 * after either is words a line may break before. The default form
 * is plain words in the quiet ink at the size of its line (a lock's kind is a label: R3);
 * `className` replaces it for a lock (a column's tag); a column's default lock and "not indexed"
 * are its plain words without the tag.
 */
export function LockWordsView({
  words,
  size,
  className,
}: {
  words: LockWords;
  /** The size of a code hash written. */
  size: ShortSize;
  className?: string;
}) {
  if (words.form === 'not-indexed')
    return (
      <span className={COLUMN_WORDS} data-lock-kind>
        not indexed
      </span>
    );
  // the default lock in a column is the column's quiet words; in a sentence, words of its line
  const own =
    words.form === 'default' && words.place === 'column' ? COLUMN_WORDS : (className ?? LOCK_WORDS);
  const by = words.form === 'lock' && words.byHashType ? ` by ${words.byHashType}` : '';
  const { name, codeHash } = words;
  return (
    <span
      className={cn(own, name === null && 'whitespace-nowrap')}
      title={codeHash ?? undefined}
      data-lock-kind
    >
      {name !== null ? (
        <>
          {name}
          {by && <span className="whitespace-normal">{by}</span>}
        </>
      ) : (
        <>
          lock{' '}
          <Link
            // the script page of that lock: its hash type where known, as a lock (B4 second fix
            // round: a bare code hash opened the family of the code hash's `type` script, the
            // default lock's for a by-data lock on its code hash)
            href={getScriptDetailHref({
              codeHash: codeHash!,
              // the default lock is its code hash by type
              hashType: words.form === 'lock' ? words.hashType : 'type',
              scriptKind: 'lock',
            })}
            title={codeHash ?? undefined}
            look="inline"
            className="transition-colors"
          >
            <ShortId value={codeHash!} kind="hash" size={size} />
          </Link>
          {/* "by <hash type>" is words after the unit, a line may break before it (B4 second fix
              round: held with "lock 0x…" it passed the columns' budgets) */}
          {by && <span className="whitespace-normal">{by}</span>}
        </>
      )}
    </span>
  );
}

/**
 * An API row's lock where it stands (`rowLockFacts`, `lockWords`, `LockWordsView`): nothing beside
 * a party for the default lock or a lock not indexed; in a column headed "Lock", never empty. A
 * caller that puts it in a list asks `lockWords` first and leaves the part out on null.
 */
export function LockKind({
  lockKind,
  lockCodeHash,
  address,
  size,
  className,
  place = 'inline',
}: {
  lockKind: string | null;
  lockCodeHash: string | null;
  /** The row's address, which spells its lock: it decides the default lock. */
  address: string | null;
  size: ShortSize;
  /** Table-rows L2-4: e.g. `rowTagClass()` to draw a lock as a row's chip; default plain dim words. */
  className?: string;
  place?: LockPlace;
}) {
  const words = lockWords(rowLockFacts({ lockKind, lockCodeHash, address }), place);
  return words && <LockWordsView words={words} size={size} className={className} />;
}
