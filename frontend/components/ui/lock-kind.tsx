'use client';

import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import type { ParticipantRef } from '@/lib/api';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { addressScript, isDefaultLock } from '@/lib/lock-script';
import { normalizeScriptRefHashType, type ScriptRefHashType } from '@/lib/script-ref';
import { shortenHash, type ShortSize } from '@/lib/shorten';
import { PartReadError, readPart } from '@/components/read-failure';
import type { ReactNode } from 'react';

/** A lock's name in its default form: plain words in the quiet ink, at the size of its line. */
const LOCK_WORDS = 'text-text-dim';
/**
 * The plain words of a column headed "Lock" (the default lock, a lock not indexed): the words a
 * column's tag holds, at its step and on its 16 px line, in the quiet ink and without its box (no
 * badge's weight for the common case), ending in the one ellipsis where the column is narrower.
 */
const COLUMN_WORDS = cn(LOCK_WORDS, 'text-meta min-w-0 truncate leading-4');

/** Where a lock is written: beside a party, or in a table column headed "Lock". */
export type LockPlace = 'inline' | 'column';

/**
 * What a place knows of a lock: the registry's name the API sends, and the lock's code hash and
 * hash type where it knows them. A place that has the lock's script (an address, which spells it,
 * or a cell's lock) knows both; a place that has only the API's bare `lockCodeHash` knows no hash
 * type, and cannot tell the default lock from another lock on its code hash.
 */
export interface LockFacts {
  name: string | null;
  codeHash: string | null;
  hashType: ScriptRefHashType | null;
}

/** What a lock is called where it stands, or null where nothing is written (see `lockWords`). */
export type LockWords =
  /** the default lock in a column: its name in quiet words */
  | { form: 'default'; name: string | null; codeHash: string }
  /** any other lock: the registry's name, else its code hash ("lock 0x…"), with a hash type the name does not cover */
  | {
      form: 'lock';
      name: string | null;
      codeHash: string | null;
      /** its hash type where the place knows it (its link opens that lock) */
      hashType: ScriptRefHashType | null;
      /** the hash type written after it, where its name does not cover it */
      byHashType: ScriptRefHashType | null;
    }
  /** a column's lock that is not indexed: "not indexed" in quiet words */
  | { form: 'not-indexed' };

/**
 * The one decision (readability R9, lane B4 and its fix round): is this lock named here, and by
 * what words. Every place that writes a lock reads it, and a place that is a part of a list leaves
 * the part out when the answer is null.
 *
 * - The default lock is the pair (its code hash, `type`). Beside a party it is silent: a shortened
 *   address is `ckb1…` and its tail, so no name beside an address means the default lock. In a
 *   column headed "Lock" a cell is never empty: the default lock is its name in quiet words.
 * - A lock on the default lock's code hash by `data`, `data1` or `data2` is another lock (no cell's
 *   data hashes to that code hash): the registry names locks by code hash, so the name it sends is
 *   the default lock's and not this lock's; it is written by its code hash and its hash type.
 * - A place that knows only a bare code hash cannot tell; it names the lock as the API does, never
 *   silent, since silence says "the default lock".
 * - A lock that is not indexed (no code hash, no name) is silent beside a party and "not indexed"
 *   in a column.
 */
export function lockWords(facts: LockFacts, place: LockPlace): LockWords | null {
  const { name, codeHash, hashType } = facts;
  if (codeHash !== null && hashType !== null && isDefaultLock(codeHash, hashType))
    return place === 'inline' ? null : { form: 'default', name, codeHash };
  if (codeHash === null && name === null)
    return place === 'inline' ? null : { form: 'not-indexed' };
  const notTheDefault =
    codeHash !== null &&
    hashType !== null &&
    isDefaultLock(codeHash, 'type') &&
    hashType !== 'type';
  return notTheDefault
    ? { form: 'lock', name: null, codeHash, hashType, byHashType: hashType }
    : { form: 'lock', name, codeHash, hashType, byHashType: null };
}

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
  const name = party.lockKind ?? null;
  if (party.address === null) return { name, codeHash: null, hashType: null };
  const { codeHash, hashType } = addressScript(party.address);
  return { name, codeHash, hashType };
}

/**
 * The lock of a version of an item's cell beside its owner (`cellLockFacts` off the cell's lock and
 * its owner's address, `lockWords` inline), or null where none is written: the default lock.
 */
export function cellOwnerLock(cell: {
  lock: { codeHash: string; hashType: string; args: string };
  lockName: string | null;
  ownerAddress: string;
  txHash: string;
  outputIndex: number;
}): LockWords | null {
  return lockWords(
    cellLockFacts(
      { lock: cell.lock, name: cell.lockName, address: cell.ownerAddress },
      `cell ${cell.txHash}:${cell.outputIndex}`
    ),
    'inline'
  );
}

/**
 * A cell owner's lock as a part of a note or a fact (`cellOwnerLock`, `LockWordsView`): null where
 * none is written, so a list leaves the part out; a lock the decision refuses fails this part in
 * its place, said with the refusal (ruling D7; lane B4 third round), never the page.
 */
export function cellOwnerLockPart(
  cell: Parameters<typeof cellOwnerLock>[0],
  className?: string
): ReactNode {
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
 * The words as text, for a place that writes a string (a tile's lock beside its owner): the name,
 * else "lock <code hash>" at the place's size, "by <hash type>" after it where the name does not
 * cover it; "not indexed".
 */
export function lockWordsText(words: LockWords, size: ShortSize): string {
  if (words.form === 'not-indexed') return 'not indexed';
  const by = words.form === 'lock' && words.byHashType ? ` by ${words.byHashType}` : '';
  if (words.name !== null) return `${words.name}${by}`;
  if (words.codeHash === null) throw new Error('a lock with neither a name nor a code hash');
  return `lock ${shortenHash(words.codeHash, size)}${by}`;
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
  const own = words.form === 'default' ? COLUMN_WORDS : (className ?? LOCK_WORDS);
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
