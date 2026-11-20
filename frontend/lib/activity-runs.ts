/**
 * Runs of a feed (readability plan 2026-11-18, lane B4 and §7 point 6; the rule as the owner ruled
 * it at Gate B, 2026-11-19, built by the lists lane P3): consecutive rows that did the same thing
 * and called the same script code fold into one row with their count, opened in place. A feed that
 * is mostly one machine saying one thing (the review counted 7,050 of 7,847 activities in a day as
 * script calls; the three .bit time oracles of mainnet take turns, each with its own args) shows
 * its other events instead of burying them.
 *
 * What "the same" means is this module's whole rule, in one key (`runKey`). Two rows are the same
 * when ALL of these are equal:
 *
 * 1. **what was done, in words**: the headline as the row draws it, with its class mark's name
 *    (the feed hands it over: a feed may head a row in its own words), so the folded row's head is
 *    every one of its rows' head;
 * 2. **what was done, as the API names it**: the protocol actions, each by its protocol and its
 *    action (`dao:deposit`), as a SET (the third read's Lb NOTE 1, by the rule of the codes: one
 *    deposit and three in one batch did one thing); an action's own figures, such as a deposit's
 *    capacity, are not compared;
 * 3. **the script code**: each by its code (its code hash and its hash type: a script is the
 *    pair), as a SET: the code that was called, not how many cells carried it (the lead's final
 *    round, item 1: as a multiset, two Godwoken transfers with two and three Withdrawal Lock
 *    outputs were two runs, which no reader would call two things done). Not the args: the same
 *    code with other args is the same (the three oracles' 0x00, 0x01, 0x02; two tokens of one
 *    standard). Three lists, as the API states them:
 *    - `recognizedTypes`, every type script code the indexer recognises that ran, read from the
 *      row (C7): the codes of the items that changed hands and of a recognised script that moved
 *      nothing, so a wallet merging its own sUDT cells never folds with a plain CKB transfer, and
 *      an sUDT and an xUDT transfer are two runs (the page-phase review's MUST-1);
 *    - `typeCalls`, the type scripts the indexer does not recognise;
 *    - `lockCalls`, the non-standard locks ATTACHED to the row's outputs. An output's lock does
 *      not run: these are not the locks the transaction ran. The locks that ran, its inputs', are
 *      the parties that sent (the indexer's per-participant input flag, `TAG_INPUT`, the senders bit
 *      every party's tags carry in the route's `partyIds`), which a run counts as its "senders"
 *      (`FoldedActivityRow`), not compared;
 * 4. **the state**: both committed. A row still in the node's pool never folds (it is its own run):
 *    its status can change before the next read;
 * 5. **a cellbase or not**: a cellbase has no input, so no party of it sent; a run of cellbases
 *    folds with no count of senders, and never with a transaction that has one.
 *
 * The parties are NOT compared (the owner: "not the args, not the parties"), nor are amounts, items
 * moved, protocol facts and times: they are what the folded rows differ in, and the folded row
 * shows none of them. Nothing is summed: a run counts its rows, the parties that sent
 * (`runSenders`, read from every party's tags in the route's `partyIds`, drawn or not) and, a run
 * of token rows, its tokens (`runTokens`, ruling 6 of gate P), and its newest time is its first
 * row's (a feed is newest first). A run is consecutive by definition: it never crosses a row whose
 * key differs, so a row that says something else ends it.
 *
 * A run folds within the rows a feed has loaded. A cursor-paginated feed folds within its page: a
 * run that goes on into the next page is a second run there; a feed that appends older pages to
 * the rows it shows (the activities stream) folds across what it has appended, because those rows
 * are consecutive on the screen.
 */

import { TAG_INPUT, type PartyId } from '@/lib/api';

/** What a feed knows of a row, to compare it with its neighbours. */
export interface RunSubject {
  /** The headline in words as the row draws it, its class mark's name with it. */
  title: string;
  /** The transaction, for a pending row's own run. */
  txHash: string;
  /** Null while the transaction is in the pool. */
  blockNumber: number | null;
  /** The block's cellbase: it has no input, so none of its parties sent. */
  isCellbase: boolean;
  protocolActions: readonly { protocol: string; action: string }[];
  typeCalls: readonly { typeCodeHash: string; typeHashType: string }[];
  lockCalls: readonly { lockCodeHash: string; lockHashType: string }[];
  /** The recognised type script codes that ran, each once (the API's `recognizedTypes`). */
  recognizedTypes: readonly { typeCodeHash: string; typeHashType: string }[];
  /**
   * Every party of the row's transaction, by its short id and tags, as the route names them
   * (`partyIds`, lane feed): a row draws some of them, and the run counts its senders among them all.
   */
  parties: readonly PartyId[];
  /**
   * The tokens the row moved, by their type script's hash (the token's id, an item delta's
   * `typeScriptHash`), on a token row; null on a row of another kind. A run counts them.
   */
  tokens: readonly string[] | null;
}

/** Each once, in one order: what was done or called, not how often. */
const codeSet = (values: readonly string[]) => [...new Set(values)].sort();

/** The key two rows share exactly when they are the same (see the module's rule). */
export function runKey(s: RunSubject): string {
  if (s.blockNumber === null) return JSON.stringify(['pending', s.txHash]);
  return JSON.stringify([
    'committed',
    s.isCellbase ? 'cellbase' : 'transaction',
    s.title,
    codeSet(s.protocolActions.map((a) => `${a.protocol}:${a.action}`)),
    codeSet(s.recognizedTypes.map((c) => `${c.typeCodeHash}|${c.typeHashType}`)),
    codeSet(s.typeCalls.map((c) => `${c.typeCodeHash}|${c.typeHashType}`)),
    codeSet(s.lockCalls.map((c) => `${c.lockCodeHash}|${c.lockHashType}`)),
  ]);
}

/**
 * How many parties of a run's rows sent, each once by its short id (the route's `partyIds`, drawn or
 * not: the first 20 bytes of its lock hash, or the prefix a protocol named it by), read from the
 * senders bit its tags carry (`TAG_INPUT`): a count, the folded row's "N senders". Every committed
 * transaction but a cellbase has an input, so each row of a run of them names at least one sender;
 * a row that names none (a store synced before the bit) is refused, never counted as no sender. A
 * run of cellbases has no senders to count.
 */
export function runSenders(subjects: readonly RunSubject[]): number {
  const ids = new Set<string>();
  for (const s of subjects) {
    if (s.isCellbase) throw new Error(`transaction ${s.txHash}: a cellbase has no sender to count`);
    let sent = false;
    for (const [id, tags] of s.parties) {
      if (tags & TAG_INPUT) {
        ids.add(id);
        sent = true;
      }
    }
    if (!sent)
      throw new Error(
        `transaction ${s.txHash}: a committed transaction with no party that sent (every transaction but a cellbase has an input)`
      );
  }
  return ids.size;
}

/**
 * How many tokens a run of token rows moved, each once by its type script's hash: the folded row's
 * "2 tokens"; null for a run of rows of another kind. The rows of a run share their title, so they
 * are all token rows or none.
 */
export function runTokens(subjects: readonly RunSubject[]): number | null {
  const tokens = subjects.map((s) => s.tokens);
  if (tokens.every((t) => t === null)) return null;
  const ids = new Set<string>();
  for (const [i, t] of tokens.entries()) {
    if (t === null)
      throw new Error(
        `transaction ${subjects[i].txHash}: a run of token rows holds a row of another kind`
      );
    for (const id of t) ids.add(id);
  }
  return ids.size;
}

/**
 * The feed's rows in runs, in order: consecutive rows with one key are one run (a run of one is an
 * ordinary row). Every row is in exactly one run, in its place.
 */
export function foldRuns<T>(rows: readonly T[], keyOf: (row: T) => string): T[][] {
  const runs: T[][] = [];
  let last: string | null = null;
  for (const row of rows) {
    const key = keyOf(row);
    if (runs.length > 0 && key === last) runs[runs.length - 1].push(row);
    else runs.push([row]);
    last = key;
  }
  return runs;
}
