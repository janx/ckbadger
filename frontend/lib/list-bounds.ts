import { ckbAmountText } from '@/lib/amount-form';
import { ELLIPSIS, type ShortForm } from '@/lib/shorten';
import { formatBlockNumber } from '@/lib/format-block';
import { occupiedBytesDelta } from '@/lib/occupied';
import { formatDurationMs } from '@/lib/utils';

/**
 * The widest value a list column holds (readability plan 2026-11-18, lane F3-e), as the column's
 * formatter writes it: a column's minimum width is that text measured at its step
 * (`lib/type-metrics.ts`) and the marks beside it. One bound per quantity, and every bound is one
 * of two kinds and says which, first in its comment (reviews fb-tests 6 and fb-logic 6, the lead's
 * ruling):
 *
 * - Consensus: the protocol's limits make it, and its comment states the arithmetic from them
 *   (`__tests__/lib/list-bounds.test.ts` writes that arithmetic again from mainnet's get_consensus
 *   and the molecule sizes of a transaction's parts).
 * - Of record: no protocol limit holds it (a fee, a value, a rank, a height years ahead); its
 *   comment states the figure it claims and its source, for a reader to check against the chain.
 *
 * A value past its bound is held on one line (a figure is one unit: `components/ui/list-table.tsx`)
 * and runs past its column's content box, into its gap and then the next column: a bound is never
 * taken narrower than the chain the site shows.
 */

/**
 * The characters of an id shortened to `form` (`lib/shorten.ts`'s `SHORT_FORMS`): its head, the one
 * ellipsis and its tail, as every hash and address of the chain is longer than that.
 */
export function shortChars(form: ShortForm): number {
  return form.head + ELLIPSIS.length + form.tail;
}

/** CKB's 10^8 shannons to the CKB. */
const SHANNONS = BigInt(100_000_000);

/** The widest `count`-digit number the en-US grouping writes: 9,999 for 4. */
function nines(count: number): bigint {
  return BigInt(10) ** BigInt(count) - BigInt(1);
}

/** The widest CKB amount of `integerDigits` whole digits and all eight decimals, in shannons. */
function ckbNines(integerDigits: number): bigint {
  return nines(integerDigits) * SHANNONS + nines(8);
}

/**
 * Of record: a block height of eight digits, `#99,999,999`. Mainnet stood at #21,045,159 on
 * 2026-11-18, a block every 8.9 s or so (some 3.5 million a year): 100,000,000 is some 22 years
 * away. Source: the mainnet tip the status line read on 2026-11-18. The one bound of a height: the
 * status line's budget (`__tests__/utils/shell-budget.ts`) is to read it too.
 */
export const WIDEST_BLOCK_NUMBER = formatBlockNumber(nines(8));

/**
 * Of record: the age of a chain time on a live list (`formatAgeOneUnit`), from a year on years and
 * days, two digits of years: `10y 364d ago`, as wide as any age until genesis is a hundred (2119).
 * Source: mainnet block 0's timestamp, 2019-11-16 05:09:50 UTC+8. (`this device's clock is behind`
 * is not an age of the chain but a sentence, words that break in their column.)
 */
export const WIDEST_AGE = '10y 364d ago';

/**
 * Of record: the widest age a live list writes for a time under a year, in its one unit
 * (`formatAgeOneUnit`): `364d ago`. A phone row whose first line holds this and not WIDEST_AGE says
 * what an older age does there (lane D4). Source: the age's form, lib/format-age.ts.
 */
export const WIDEST_AGE_UNDER_A_YEAR = '364d ago';

/**
 * Consensus: the protocol's limit of a block's serialized size, `max_block_bytes`, 597,000 bytes
 * (mainnet and testnet `get_consensus`).
 */
export const MAX_BLOCK_BYTES = 597_000;

/**
 * Consensus: a count a block or a transaction holds, in four digits, `9,999`. A transaction in a
 * block takes 116 bytes at least (its table, its RawTransaction's, version, three empty vectors,
 * one 44-byte input and its offset in the block), so a block of MAX_BLOCK_BYTES holds at most
 * floor(597,000 / 116) = 5,146; an output takes 89 bytes at least (a 77-byte CellOutput with an
 * empty lock args and no type, its empty data and their two offsets), so a transaction holds at
 * most floor(597,000 / 89) = 6,707. Genesis, the one block past the limit, holds two transactions,
 * its cellbase 671 outputs. Not a transaction's inputs: MAX_TX_INPUTS.
 */
export const WIDEST_COUNT = nines(4).toLocaleString('en-US');

/**
 * Consensus: the most inputs a transaction holds, 13,568 (five digits): an input is a 44-byte
 * CellInput (since, 8, and its out point, a 32-byte hash and a 4-byte index), so at most
 * floor(MAX_BLOCK_BYTES / 44) fit a block; genesis's two transactions hold one input each.
 */
export const MAX_TX_INPUTS = Math.floor(MAX_BLOCK_BYTES / 44);

/**
 * Consensus: a block's proposals, up to the protocol's `max_block_proposals_limit`, 1,500.
 * `1,500`.
 */
export const WIDEST_PROPOSALS = (1_500).toLocaleString('en-US');

/** Consensus: WIDEST_COUNT's four digits where a list writes a count ungrouped: `9999`. */
export const WIDEST_COUNT_DIGITS = nines(4).toString();

/**
 * Of record: mainnet's genesis block's serialized size, 1,236,271 bytes, the one block over the
 * limit (its system cells' code). Source: mainnet block 0 as the API serves it
 * (`__tests__/msw/fixtures/genesis-block.ts`, captured 2026-11-12).
 */
export const GENESIS_BLOCK_BYTES = 1_236_271;

/**
 * Of record: the largest transaction a list can show, no larger than its block, and the largest
 * block is genesis, whose cellbase is 1,235,438 bytes: the larger of the limit and genesis's size,
 * 1,236,271. Source: GENESIS_BLOCK_BYTES.
 */
export const LARGEST_TX_BYTES = Math.max(MAX_BLOCK_BYTES, GENESIS_BLOCK_BYTES);

/**
 * Of record: a party's change in occupied bytes in one transaction (or the transaction's net),
 * with its unit, `−8,101,332,271 B`. A cell's occupied bytes (its capacity field, its scripts, its
 * data) are fewer than its output's serialized bytes in the transaction that made it. A change
 * grows by no more than what the transaction makes, at most its bytes (LARGEST_TX_BYTES), and
 * shrinks by no more than what it consumes, the wider: genesis's cells, the only ones past the
 * block limit, together no more than genesis's block (GENESIS_BLOCK_BYTES, of record), and
 * MAX_TX_INPUTS other cells of no more than a block each (MAX_TX_INPUTS × MAX_BLOCK_BYTES,
 * consensus): 1,236,271 + 13,568 × 597,000 = 8,101,332,271. (The cell tile budgets one cell's
 * figure at LARGEST_TX_BYTES, `1,236,271 B`.) Source: GENESIS_BLOCK_BYTES, with MAX_TX_INPUTS and
 * MAX_BLOCK_BYTES.
 */
export const WIDEST_OCCUPIED_CHANGE = occupiedBytesDelta(
  -(BigInt(GENESIS_BLOCK_BYTES) + BigInt(MAX_TX_INPUTS) * BigInt(MAX_BLOCK_BYTES))
);

/**
 * Consensus: the protocol's limit of a block's cycles, `max_block_cycles`, 3,500,000,000, and so of
 * one transaction's.
 */
export const MAX_BLOCK_CYCLES = 3_500_000_000;

/**
 * Of record: the most cycles a committed list's page shows for one transaction, under 750,000,000
 * (any scale under a billion, so its figure is at most `750,000,000`; the protocol allows a
 * transaction a block's 3,500,000,000). Source: lane B5's reading of mainnet's transactions.
 */
export const TX_CYCLES_BOUND = 750_000_000;

/**
 * Consensus: the widest tick the tick form writes for a list bar's scale (`tickForm`, as a chart's
 * ticks are written): four characters ("1.5k", "7.5M", "750M") for every scale `resourceScale`
 * gives up to MAX_BLOCK_CYCLES.
 */
export const WIDEST_COMPACT_TICK = '750M';

/**
 * Of record: a block's mining reward under 10,000 CKB, four whole digits (the first epochs paid
 * 1,917 CKB, primary issuance halving since). Source: mainnet's block rewards.
 */
export const WIDEST_BLOCK_REWARD = ckbAmountText(ckbNines(4), 'column');

/** Of record: a block's fees under 1,000 CKB. Source: mainnet's blocks' total fees. */
export const WIDEST_BLOCK_FEES = ckbAmountText(ckbNines(3), 'column');

/**
 * Of record: a transaction's value (what its participants received) under a billion CKB. Source:
 * mainnet's transactions.
 */
export const WIDEST_TX_VALUE = ckbAmountText(ckbNines(9), 'column');

/**
 * Of record: WIDEST_TX_VALUE as a phone row writes it, the summary form (whole CKB from 1,000 up):
 * `999,999,999`; a Fiber channel's capacity holds to it too (its table's Capacity column reads
 * WIDEST_TX_VALUE). Source: WIDEST_TX_VALUE.
 */
export const WIDEST_TX_VALUE_SUMMARY = ckbAmountText(ckbNines(9), 'summary');

/**
 * Of record: the CKB a transaction outputs, every decimal, eleven whole digits (the genesis
 * transactions output the 33.6 billion CKB first issued; all CKB is some 45 billion today). Source:
 * mainnet block 0 and the issuance.
 */
export const WIDEST_TX_OUTPUT = ckbAmountText(ckbNines(11), 'column');

/**
 * Of record: WIDEST_TX_OUTPUT as a phone row writes it, the summary form (whole CKB from 1,000 up):
 * `99,999,999,999`. Source: WIDEST_TX_OUTPUT.
 */
export const WIDEST_TX_OUTPUT_SUMMARY = ckbAmountText(ckbNines(11), 'summary');

/** Of record: a transaction's fee under 100 CKB. Source: mainnet's transactions' fees. */
export const WIDEST_TX_FEE = ckbAmountText(ckbNines(2), 'column');

/**
 * Of record: a pool transaction's value on one line, under a million CKB; a larger one breaks
 * before its decimals (`CkbAmount`'s `wrap`), its whole part under a billion CKB as WIDEST_TX_VALUE
 * claims. Source: WIDEST_TX_VALUE (the line itself is the column's choice, not a chain claim).
 */
export const WIDEST_POOL_VALUE_LINE = ckbAmountText(ckbNines(6), 'column');

/**
 * Of record: a pool transaction's fee rate, shannons a KB, up to 1,000,000,000. Source: mainnet
 * pool transactions' fee rates.
 */
export const WIDEST_FEE_RATE = (1_000_000_000).toLocaleString('en-US');

/**
 * Of record: one depositor's CKB in the DAO, under ten billion (the whole DAO holds less). Source:
 * mainnet's DAO deposits.
 */
export const WIDEST_DAO_DEPOSIT = ckbAmountText(ckbNines(10), 'column');

/**
 * Of record: WIDEST_DAO_DEPOSIT as a phone row writes it, the summary form (whole CKB from 1,000 up):
 * `9,999,999,999`. Source: WIDEST_DAO_DEPOSIT.
 */
export const WIDEST_DAO_DEPOSIT_SUMMARY = ckbAmountText(ckbNines(10), 'summary');

/**
 * Of record: the live objects one holder holds of one collection, under a million, `999,999` (the
 * largest collections hold thousands). Source: mainnet's object collections' live counts.
 */
export const WIDEST_HELD_ITEMS = nines(6).toLocaleString('en-US');

/** Of record: a depositor's rank, under a million depositors. Source: mainnet's DAO depositors. */
export const WIDEST_RANK = nines(6).toLocaleString('en-US');

/**
 * Of record: CKB held in a list row's cells, whole from 1,000 up (the summary form): under 100
 * billion CKB, the whole issuance (about 45 billion today, 1.3 billion more a year) for decades.
 * Source: the issuance.
 */
export const WIDEST_CKB_HELD = ckbAmountText(ckbNines(11), 'summary');

/**
 * Of record: a count an inventory or scripts row states (holders, transfers, live cells), eight
 * digits. Source: mainnet's largest such count, the default lock's live cells.
 */
export const WIDEST_ROW_COUNT = nines(8).toLocaleString('en-US');

/**
 * Of record: how far apart in time a fork's two blocks at one height were found, under 1,000 s:
 * six digits of milliseconds, `999,999 ms` (lane P4: it was kept on the forks page). A sibling is
 * found while its parent is the tip, and blocks come some ten seconds apart. Source: the forks
 * page's "Found apart" column, mainnet's branch switches.
 */
export const WIDEST_SIBLING_GAP = '999,999 ms';

/** What a count or figure the API has not derived says in its place (a list writes it either case). */
export const UNAVAILABLE = 'Unavailable';

/**
 * Of record: how long a Fiber channel has been open or lived, on the site's one duration form (its
 * two largest units, `formatDurationMs`, as `lib/fiber-format.ts` writes it): under 10,000 days,
 * some 27 years, `9999d 23h`. Source: the first Fiber channel on mainnet opened on 2025-03-06
 * (c07e3fa5…, block #15,564,812).
 */
export const WIDEST_FIBER_LIFETIME = formatDurationMs((9999 * 86_400 + 23 * 3600) * 1000);
