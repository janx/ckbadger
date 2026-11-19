/**
 * The one decision's text half (readability R9, lane B4; lane P2's third round): what a lock is
 * called where it stands, as words and as text, with no React, so the agent renderer
 * (lib/ai/server-entry.ts, bundled for a bare `vm`) says a lock in the page's own words. The
 * decision's drawn half (LockWordsView, LockKind, the rows' and cells' facts) is
 * components/ui/lock-kind.tsx, which re-exports this half; agent-renderer-imports.test keeps the
 * renderer's import graph free of React.
 */
import { addressScript, isDefaultLock } from '@/lib/lock-script';
import type { ScriptRefHashType } from '@/lib/script-ref';
import { shortenHash, type ShortSize } from '@/lib/shorten';

/**
 * Where a lock is written: beside a party, in a table column headed "Lock", or in a sentence (a
 * lede, a story, a note that says the lock in words: "Minted to a JoyID lock").
 */
export type LockPlace = 'inline' | 'column' | 'sentence';

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
  /** the default lock in a column (its name in quiet words) or in a sentence (its name in the line) */
  | { form: 'default'; name: string | null; codeHash: string; place: 'column' | 'sentence' }
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
  /** a lock that is not indexed, in a column or a sentence: "not indexed" in quiet words */
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
 * - In a sentence (lane P2's follow-up, item 6) silence says nothing: a sentence has no party to be
 *   silent beside, so it takes the column's answer (`namedLockWords`), never null, the default lock
 *   by its name and a lock not indexed in words. `lockPhrase` says it as a noun phrase.
 */
export function lockWords(facts: LockFacts, place: LockPlace): LockWords | null {
  if (place === 'inline' && silentBesideAParty(facts)) return null;
  return namedLockWords(facts, place === 'sentence' ? 'sentence' : 'column');
}

/** The default lock, and a lock not indexed: what a party's place leaves unsaid. */
function silentBesideAParty({ name, codeHash, hashType }: LockFacts): boolean {
  return (
    (codeHash !== null && hashType !== null && isDefaultLock(codeHash, hashType)) ||
    (codeHash === null && name === null)
  );
}

/**
 * The decision where a lock is never silent (`lockWords`): a column headed "Lock", and a sentence
 * (lane P2's follow-up, item 6: the decision's sentence form, said by `lockPhrase`). The default
 * lock is its name, in a column's quiet words or in the line of a sentence; a lock not indexed is
 * "not indexed"; any other lock as beside a party.
 */
export function namedLockWords(facts: LockFacts, place: 'column' | 'sentence'): LockWords {
  const { name, codeHash, hashType } = facts;
  if (codeHash !== null && hashType !== null && isDefaultLock(codeHash, hashType))
    return { form: 'default', name, codeHash, place };
  if (codeHash === null && name === null) return { form: 'not-indexed' };
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
 * A named party's lock (a .cell name's owner, manager, a sale's seller; lane P2's third round,
 * item 12): the registry's name the API sends for its lock code hash (`lockScriptName`) and, where
 * the party has an address, the lock read off it, which decides (a lock on the default lock's code
 * hash by data is not the default lock). A party with no address keeps the name alone.
 */
export function namedPartyLockFacts(party: {
  lockScriptName: string | null;
  address: string | null;
}): LockFacts {
  const name = party.lockScriptName;
  if (party.address === null) return { name, codeHash: null, hashType: null };
  const { codeHash, hashType } = addressScript(party.address);
  return { name, codeHash, hashType };
}

/** A named party's lock in a fact's note or a row (the decision's column words, never silent). */
export function partyLockWordsText(
  party: Parameters<typeof namedPartyLockFacts>[0],
  size: ShortSize
): string {
  return lockWordsText(namedLockWords(namedPartyLockFacts(party), 'column'), size);
}

/** A named party's lock in a sentence ("a JoyID lock owns it"; the decision's sentence form). */
export function partyLockSentence(
  party: Parameters<typeof namedPartyLockFacts>[0],
  size: ShortSize
): string {
  return lockPhrase(namedLockWords(namedPartyLockFacts(party), 'sentence'), size);
}

/**
 * The decision's sentence form said (lane P2's follow-up, item 6): a lock as a noun phrase with its
 * article, for a sentence that names the lock a cell sits under. The words are `lockWordsText`'s:
 * a name that ends in "lock" stands as it is ("a Default Lock", "an Omnilock"), any other takes the
 * word ("a JoyID lock"); a lock the registry does not name is its code hash at `size` ("a lock
 * 0x9b9b9b9b…9b9b9b"), with its hash type where the name does not cover it ("a lock 0x9bd7e06f…a3cce8
 * by data"); a lock not indexed, "a lock not indexed". The article goes by the name's first sound
 * (`articleBySound`: "an RGB++ lock", "a UniPass lock").
 */
export function lockPhrase(words: LockWords, size: ShortSize): string {
  const noun = lockNoun(words, size);
  // a lock without a name or not indexed starts with "lock": "a lock 0x…", "a lock not indexed"
  if (words.form === 'not-indexed' || words.name === null) return `a ${noun}`;
  return `${articleBySound(words.name)} ${noun}`;
}

/**
 * The decision's sentence form without its article (`lockPhrase`'s noun): a name that ends in
 * "lock" as it is ("Default Lock", "Omnilock"), any other with the word ("JoyID lock"); a lock the
 * registry does not name, "lock 0x9b9b9b9b…9b9b9b" with its hash type where the name does not
 * cover it; "lock not indexed". For a list that names locks one after another (the locks an
 * activity's outputs carry), where every name must say it is a lock.
 */
export function lockNoun(words: LockWords, size: ShortSize): string {
  if (words.form === 'not-indexed') return 'lock not indexed';
  // a lock without a name is "lock 0x…", its hash type after it where it has one
  if (words.name === null) return lockWordsText(words, size);
  // a named lock never carries a hash type: the decision drops a name it does not cover
  return /lock$/i.test(words.name) ? words.name : `${words.name} lock`;
}

/** Letters whose spoken names start with a vowel sound: "ay", "ee", "ef", "aitch", "eye", "el", … */
const VOWEL_SOUNDING_LETTERS = /^[aefhilmnorsx]$/i;

/**
 * A name's article by its first SOUND, not its first letter (lane P2's third round, item 14): a
 * name led by letters spoken one by one (a capital not followed by a small letter, "RGB++", "M-NFT",
 * "UTXOSwap"; a small letter before a capital, "xUDT", "iCKB") goes by the first letter's spoken
 * name ("an RGB++", "an xUDT", "a UTXOSwap"); a word by its first vowel sound, a "u" read "you"
 * before a consonant and a vowel ("a UniPass", "a Unique"); a name led by a mark (".bit", "dot")
 * takes "a". Read over every name the registry holds in lock-kind.test.
 */
function articleBySound(name: string): 'a' | 'an' {
  const lead = /^[A-Za-z]+/.exec(name)?.[0];
  if (lead === undefined) return 'a';
  if (/^[A-Z](?![a-z])/.test(lead) || /^[a-z][A-Z]/.test(lead))
    return VOWEL_SOUNDING_LETTERS.test(lead[0]) ? 'an' : 'a';
  if (/^u[^aeiou][aeiou]/i.test(lead)) return 'a';
  return /^[aeiou]/i.test(lead) ? 'an' : 'a';
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
