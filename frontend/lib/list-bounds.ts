import { ckbAmountText } from '@/lib/amount-form';
import { ELLIPSIS, type ShortForm } from '@/lib/shorten';
import { formatBlockNumber } from '@/lib/format-block';

/**
 * The widest value a list column holds (readability plan 2026-11-18, lane F3-e), as the column's
 * formatter writes it: a column's minimum width is that text measured at its step
 * (`lib/type-metrics.ts`) and the marks beside it. One bound per quantity, each a fact of the chain
 * or a bound of record, with its reason; a value past a bound draws over its neighbour's gap, so a
 * bound is never taken narrower than the chain the site shows.
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
 * A block height: eight digits. The chain stands at 21 million after seven years (about 3.2
 * million blocks a year); 100,000,000 is some 25 years away. `#99,999,999`.
 */
export const WIDEST_BLOCK_NUMBER = formatBlockNumber(nines(8));

/**
 * The age of a chain time on a live list (`formatAgeOneUnit`): from a year on, years and days.
 * Genesis (2019-11-16) turns ten on 2029-11-16; two digits of years hold until it is a hundred.
 * (`this device's clock is behind` is not an age of the chain: a device fault, not budgeted.)
 */
export const WIDEST_AGE = '10y 364d ago';

/**
 * A count a block, a transaction or a list holds, in four digits: a block of 597,000 bytes holds
 * under 3,000 transactions, and a transaction's inputs or outputs (the genesis cellbase has 671)
 * stay under 10,000. `9,999`.
 */
export const WIDEST_COUNT = nines(4).toLocaleString('en-US');

/** The same four digits where a list writes a count ungrouped (the pool's cells): `9999`. */
export const WIDEST_COUNT_DIGITS = nines(4).toString();

/** The consensus limit of a block's serialized size, bytes (mainnet and testnet). */
export const MAX_BLOCK_BYTES = 597_000;

/** The mainnet genesis block's size, bytes: the one block over the limit (its system cells). */
export const GENESIS_BLOCK_BYTES = 1_236_271;

/** The consensus limit of a block's cycles, and so of one transaction's. */
export const MAX_BLOCK_CYCLES = 3_500_000_000;

/**
 * The largest transaction a committed list's page shows, cycles (lane B5's bound of record: any
 * scale under a billion, so its figure is at most `750,000,000`).
 */
export const TX_CYCLES_BOUND = 750_000_000;

/**
 * The widest tick the tick form writes for a list bar's scale (`formatTick`, as a chart's ticks
 * are written): four characters ("1.5k", "7.5M", "750M") for any scale `resourceScale` gives up to
 * the cycle limit.
 */
export const WIDEST_COMPACT_TICK = '750M';

/** A block's mining reward: four whole digits (the first epochs paid 1,917 CKB and more). */
export const WIDEST_BLOCK_REWARD = ckbAmountText(ckbNines(4), 'column');

/** A block's fees, under 1,000 CKB (bound of record). */
export const WIDEST_BLOCK_FEES = ckbAmountText(ckbNines(3), 'column');

/** A transaction's value, under a billion CKB (bound of record). */
export const WIDEST_TX_VALUE = ckbAmountText(ckbNines(9), 'column');

/** A transaction's fee, under 100 CKB (bound of record). */
export const WIDEST_TX_FEE = ckbAmountText(ckbNines(2), 'column');

/**
 * A pool transaction's value on one line, under a million CKB; a larger one breaks before its
 * decimals (`CkbAmount`'s `wrap`), its whole part under a billion CKB as the committed list's.
 */
export const WIDEST_POOL_VALUE_LINE = ckbAmountText(ckbNines(6), 'column');

/** A pool transaction's fee rate, shannons a KB: up to 1,000,000,000 (bound of record). */
export const WIDEST_FEE_RATE = (1_000_000_000).toLocaleString('en-US');

/** One depositor's CKB in the DAO, under ten billion (the whole DAO holds less). */
export const WIDEST_DAO_DEPOSIT = ckbAmountText(ckbNines(10), 'column');

/** A depositor's rank: under a million depositors. */
export const WIDEST_RANK = nines(6).toLocaleString('en-US');

/**
 * CKB held in a list row's cells, whole from 1,000 up (the summary form): under 100 billion CKB,
 * the whole issuance (about 45 billion today, 1.3 billion more a year) for decades.
 */
export const WIDEST_CKB_HELD = ckbAmountText(ckbNines(11), 'summary');

/** A count an inventory or scripts row states (holders, transfers, live cells): eight digits. */
export const WIDEST_ROW_COUNT = nines(8).toLocaleString('en-US');

/** What a count or figure the API has not derived says in its place (a list writes it either case). */
export const UNAVAILABLE = 'Unavailable';
