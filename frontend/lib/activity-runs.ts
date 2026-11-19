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
 *    action (`dao:deposit`), as a multiset (each with its count); an action's own figures, such as
 *    a deposit's capacity, are not compared;
 * 3. **the script code**: each by its code (its code hash and its hash type: a script is the
 *    pair), as a multiset. Not the args: the same code with other args is the same (the three
 *    oracles' 0x00, 0x01, 0x02; two tokens of one standard). Three lists, as the API states them:
 *    - `itemTypes`, the type script of every token, object and identity the row moved and the
 *      Nervos DAO's, as the indexer classified each item (the page-phase review's MUST-1: an
 *      item's type is never a call, so without them an sUDT and an xUDT transfer were one run);
 *    - `typeCalls`, the type scripts the indexer does not recognise;
 *    - `lockCalls`, the non-standard locks ATTACHED to the row's outputs. An output's lock does
 *      not run: these are not the locks the transaction ran. The locks that ran, its inputs', are
 *      the parties that sent (`RunParty.sent`, the indexer's per-participant input flag), which a
 *      run counts as its "senders" (`FoldedActivityRow`), not compared;
 * 4. **the state**: both committed. A row still in the node's pool never folds (it is its own run):
 *    its status can change before the next read;
 * 5. **a cellbase or not**: a cellbase has no input, so no party of it sent; a run of cellbases
 *    folds with no count of senders, and never with a transaction that has one.
 *
 * The parties are NOT compared (the owner: "not the args, not the parties"), nor are amounts, items
 * moved, protocol facts and times: they are what the folded rows differ in, and the folded row
 * shows none of them. Nothing is summed: a run counts its rows and the parties that sent
 * (`runSenders`), and its newest time is its first row's (a feed is newest first). A run is consecutive by definition:
 * it never crosses a row whose key differs, so a row that says something else ends it.
 *
 * A run folds within the rows a feed has loaded. A cursor-paginated feed folds within its page: a
 * run that goes on into the next page is a second run there; a feed that appends older pages to
 * the rows it shows (the activities stream) folds across what it has appended, because those rows
 * are consecutive on the screen.
 */

/** A party of a row, by what names it, and whether it sent. */
export interface RunParty {
  lockHash: string | null;
  lockHashPrefix: string | null;
  address: string | null;
  /** Its lock is the lock of at least one input of the transaction (the API's `sent`). */
  sent: boolean;
}

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
  /** Its items' and its DAO step's type scripts, each code once (the API's `itemTypes`). */
  itemTypes: readonly { typeCodeHash: string; typeHashType: string }[];
  /** The row's parties (the feed's: a feed may know more of them than the row draws), counted. */
  parties: readonly RunParty[];
}

function partyId(p: RunParty, txHash: string): string {
  const id = p.lockHash ?? p.lockHashPrefix ?? p.address;
  if (id === null)
    throw new Error(`transaction ${txHash}: a party with no lock hash, prefix or address`);
  return id;
}

const multiset = (values: readonly string[]) => [...values].sort();

/** The key two rows share exactly when they are the same (see the module's rule). */
export function runKey(s: RunSubject): string {
  if (s.blockNumber === null) return JSON.stringify(['pending', s.txHash]);
  return JSON.stringify([
    'committed',
    s.isCellbase ? 'cellbase' : 'transaction',
    s.title,
    multiset(s.protocolActions.map((a) => `${a.protocol}:${a.action}`)),
    multiset(s.itemTypes.map((c) => `${c.typeCodeHash}|${c.typeHashType}`)),
    multiset(s.typeCalls.map((c) => `${c.typeCodeHash}|${c.typeHashType}`)),
    multiset(s.lockCalls.map((c) => `${c.lockCodeHash}|${c.lockHashType}`)),
  ]);
}

/**
 * How many parties of a run's rows sent, each once (by its full lock hash, else the 20-byte prefix a
 * protocol named it by, else its address): a count, the folded row's "N senders". Every committed
 * transaction but a cellbase has an input, so each row of a run of them names at least one sender;
 * a row that names none, or a party that does not say whether it sent (an API older than this
 * page), is refused, never counted as no sender. A run of cellbases has no senders to count.
 */
export function runSenders(subjects: readonly RunSubject[]): number {
  const ids = new Set<string>();
  for (const s of subjects) {
    if (s.isCellbase) throw new Error(`transaction ${s.txHash}: a cellbase has no sender to count`);
    let sent = false;
    for (const p of s.parties) {
      if (typeof p.sent !== 'boolean')
        throw new Error(
          `transaction ${s.txHash}: party ${partyId(p, s.txHash)} does not say whether it sent: the API is older than this page`
        );
      if (p.sent) {
        ids.add(partyId(p, s.txHash));
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
