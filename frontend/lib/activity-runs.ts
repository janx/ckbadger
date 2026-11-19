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
 * 3. **the script code called**: the type scripts and the lock scripts called, each by its code
 *    (its code hash and its hash type: a script is the pair), as a multiset. Not the args: the
 *    same code called with other args is the same call (the three oracles' 0x00, 0x01, 0x02);
 * 4. **the state**: both committed. A row still in the node's pool never folds (it is its own run):
 *    its status can change before the next read.
 *
 * The parties are NOT compared (the owner: "not the args, not the parties"), nor are amounts, items
 * moved, protocol facts and times: they are what the folded rows differ in, and the folded row
 * shows none of them. Nothing is summed: a run counts its rows and its parties (`runParties`), and
 * its newest time is its first row's (a feed is newest first). A run is consecutive by definition:
 * it never crosses a row whose key differs, so a row that says something else ends it.
 *
 * A run folds within the rows a feed has loaded. A cursor-paginated feed folds within its page: a
 * run that goes on into the next page is a second run there; a feed that appends older pages to
 * the rows it shows (the activities stream) folds across what it has appended, because those rows
 * are consecutive on the screen.
 */

/** A party of a row, by what names it. */
export interface RunParty {
  lockHash: string | null;
  lockHashPrefix: string | null;
  address: string | null;
}

/** What a feed knows of a row, to compare it with its neighbours. */
export interface RunSubject {
  /** The headline in words as the row draws it, its class mark's name with it. */
  title: string;
  /** The transaction, for a pending row's own run. */
  txHash: string;
  /** Null while the transaction is in the pool. */
  blockNumber: number | null;
  protocolActions: readonly { protocol: string; action: string }[];
  typeCalls: readonly { typeCodeHash: string; typeHashType: string }[];
  lockCalls: readonly { lockCodeHash: string; lockHashType: string }[];
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
    s.title,
    multiset(s.protocolActions.map((a) => `${a.protocol}:${a.action}`)),
    multiset(s.typeCalls.map((c) => `${c.typeCodeHash}|${c.typeHashType}`)),
    multiset(s.lockCalls.map((c) => `${c.lockCodeHash}|${c.lockHashType}`)),
  ]);
}

/**
 * How many parties a run's rows name, each once (by its full lock hash, else the 20-byte prefix a
 * protocol named it by, else its address): a count, the folded row's "N participants".
 */
export function runParties(subjects: readonly RunSubject[]): number {
  const ids = new Set<string>();
  for (const s of subjects) for (const p of s.parties) ids.add(partyId(p, s.txHash));
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
