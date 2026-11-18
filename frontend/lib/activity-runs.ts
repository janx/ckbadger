/**
 * Runs of a feed (readability plan 2026-11-18, lane B4 and §7 point 6): consecutive rows that say
 * the same thing fold into one row with their count and the newest time, opened in place. A feed
 * that is mostly one machine saying one thing (the review counted 7,050 of 7,847 activities in a
 * day as script calls) shows its other events instead of burying them.
 *
 * What "the same" means is this module's whole rule, in one key (`runKey`). Two rows are the same
 * when ALL of these are equal:
 *
 * 1. **the title**: the headline as the row draws it, in words, with its class mark's name (the
 *    feed hands it over: a feed may head a row in its own words);
 * 2. **the scripts**: the type scripts called and the lock scripts called, each the whole script
 *    (its script hash: code hash, hash type and args), as a multiset (each with its count);
 * 3. **the parties**: each party by its identity (its full lock hash, else the 20-byte prefix a
 *    protocol named it by, else its address) with its roles, as a multiset;
 * 4. **the state**: both committed. A row still in the node's pool never folds (it is its own run):
 *    its status can change before the next read.
 *
 * Amounts, items moved, protocol facts and times are NOT compared: they are what the folded rows
 * differ in, and the folded row shows none of them. Nothing is summed: a run counts its rows, and
 * its newest time is its first row's (a feed is newest first). A run is consecutive by definition:
 * it never crosses a row whose key differs, so a row that says something else ends it.
 *
 * A run folds within the rows a feed has loaded. A cursor-paginated feed folds within its page: a
 * run that goes on into the next page is a second run there; a feed that appends older pages to
 * the rows it shows (the activities stream) folds across what it has appended, because those rows
 * are consecutive on the screen.
 */

/** What a feed knows of a row, to compare it with its neighbours. */
export interface RunSubject {
  /** The headline in words as the row draws it, its class mark's name with it. */
  title: string;
  /** The transaction, for a pending row's own run. */
  txHash: string;
  /** Null while the transaction is in the pool. */
  blockNumber: number | null;
  typeCalls: readonly { scriptHash: string }[];
  lockCalls: readonly { scriptHash: string }[];
  parties: readonly {
    lockHash: string | null;
    lockHashPrefix: string | null;
    address: string | null;
    roles: readonly string[];
  }[];
}

function partyId(p: RunSubject['parties'][number], txHash: string): string {
  const id = p.lockHash ?? p.lockHashPrefix ?? p.address;
  if (id === null)
    throw new Error(`transaction ${txHash}: a party with no lock hash, prefix or address`);
  return `${id}(${[...p.roles].sort().join(',')})`;
}

const multiset = (values: readonly string[]) => [...values].sort();

/** The key two rows share exactly when they are the same (see the module's rule). */
export function runKey(s: RunSubject): string {
  if (s.blockNumber === null) return JSON.stringify(['pending', s.txHash]);
  return JSON.stringify([
    'committed',
    s.title,
    multiset(s.typeCalls.map((c) => c.scriptHash)),
    multiset(s.lockCalls.map((c) => c.scriptHash)),
    multiset(s.parties.map((p) => partyId(p, s.txHash))),
  ]);
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
