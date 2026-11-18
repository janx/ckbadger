/**
 * The homepage's activity numbers: one total, the share of each activity type, and the ranked
 * script calls — all from `/stats/activity-summary-24h` and `/stats/daily-activities` rows.
 */
import type {
  ActivitySummary24h,
  ScriptCountEntry,
  DailyActivityStats,
  GlobalActivityFilter,
  TxStatsResponse,
} from '@/lib/api';
import { shortenHash } from '@/lib/shorten';
import { percentOf } from '@/lib/format-unit';

/**
 * The activity kinds a summary or a daily row carries: each non-cellbase transaction in exactly one
 * kind (the one the global filter lists it under), and their total, one number from the API.
 */
export type ActivityCounts = Pick<ActivitySummary24h, 'activityCount' | 'kindCounts'>;

export interface ShareRow {
  key: string;
  label: string;
  count: number;
  /** count / total, in [0, 1]. */
  share: number;
}

/** Each activity kind's share of the transactions, largest first; kinds with none are left out. */
export function activityTypeShares(c: ActivityCounts): ShareRow[] {
  const total = c.activityCount;
  if (total === 0) return [];
  return [
    { key: 'script', label: 'Script call', count: c.kindCounts.script },
    { key: 'transfer', label: 'Transfer', count: c.kindCounts.ckb },
    { key: 'token', label: 'Token', count: c.kindCounts.token },
    { key: 'object', label: 'Object', count: c.kindCounts.object },
    { key: 'identity', label: 'Identity', count: c.kindCounts.identity },
    { key: 'dao', label: 'DAO', count: c.kindCounts.dao },
    { key: 'protocol', label: 'Protocol', count: c.kindCounts.protocol },
  ]
    .filter((row) => row.count > 0)
    .map((row) => ({ ...row, share: row.count / total }))
    .sort((a, b) => b.count - a.count);
}

export interface ScriptShareRow extends ShareRow {
  codeHash: string;
  name: string | null;
}

export interface ScriptShares {
  /** Script calls in total, across every script. */
  total: number;
  top: ScriptShareRow[];
  /** The scripts beyond the top ones, folded into one row; null when there are none. */
  others: { scripts: number; count: number; share: number } | null;
}

/**
 * A script's name when the API knows one, else its code hash at the tight size: the label of a
 * share bar in the home page's stats card (U-09).
 */
export function scriptLabel(entry: Pick<ScriptCountEntry, 'name' | 'codeHash'>): string {
  const name = entry.name?.trim();
  return name ? name : shortenHash(entry.codeHash, 'tight');
}

/** Script calls ranked by count: the `limit` largest, then everything else as one row. */
export function scriptShares(entries: ScriptCountEntry[], limit = 5): ScriptShares {
  const ranked = entries.filter((e) => e.count > 0).sort((a, b) => b.count - a.count);
  const total = ranked.reduce((sum, e) => sum + e.count, 0);
  if (total === 0) return { total: 0, top: [], others: null };
  const top = ranked.slice(0, limit).map((e) => ({
    key: e.codeHash,
    label: scriptLabel(e),
    count: e.count,
    share: e.count / total,
    codeHash: e.codeHash,
    name: e.name,
  }));
  const rest = ranked.slice(limit);
  const restCount = rest.reduce((sum, e) => sum + e.count, 0);
  return {
    total,
    top,
    others:
      rest.length > 0 ? { scripts: rest.length, count: restCount, share: restCount / total } : null,
  };
}

/** A count's share of the total, the one percentage (CH-14): `81.40%`, never a false `0.00%`. */
export function formatShare(count: number, total: number): string {
  if (!Number.isSafeInteger(count) || !Number.isSafeInteger(total) || count < 0 || count > total)
    throw new Error(`share: ${count} is not a part of ${total}`);
  return percentOf(BigInt(count), BigInt(total));
}

/** Same fixed UTC+8 calendar used by the indexer's ChainWindow day buckets. */
export function previousChainDay(now = Date.now()): string {
  return new Date(now + 8 * 60 * 60 * 1000 - 24 * 60 * 60 * 1000).toISOString().slice(0, 10);
}
export function completedYesterdayActivities(
  days: readonly DailyActivityStats[],
  now = Date.now()
): DailyActivityStats | undefined {
  const yesterday = previousChainDay(now);
  return days.find((day) => day.date.replace(/^(\d{4})(\d{2})(\d{2})$/, '$1-$2-$3') === yesterday);
}
/** Reads only the matching, explicitly complete canonical transaction-count bucket. */
export function completedYesterdayTransactions(
  stats: TxStatsResponse,
  now = Date.now()
): number | undefined {
  const label = previousChainDay(now).slice(5).replace('-', '/');
  const bucket = stats.dailyData.find((day) => day.complete && day.label === label);
  if (!bucket) return undefined;
  if (!Number.isSafeInteger(bucket.value) || bucket.value < 0)
    throw new Error(`Invalid transaction count for ${label}: ${bucket.value}`);
  return bucket.value;
}
/** The global activity stream is exactly canonical transactions minus cellbase. */
export function nonCellbaseCount(transactions: number, cellbase: number, date: string): number {
  if (
    !Number.isSafeInteger(transactions) ||
    !Number.isSafeInteger(cellbase) ||
    cellbase < 0 ||
    transactions < cellbase
  )
    throw new Error(`Invalid transaction/cellbase counts on ${date}: ${transactions}/${cellbase}`);
  return transactions - cellbase;
}
/** Each filter chip's count: its kind's transactions (what that filter lists), and All. */
export function activityFilterCounts(
  summary: ActivityCounts,
  allCount?: number
): Partial<Record<GlobalActivityFilter, number>> {
  return {
    ...(allCount === undefined ? {} : { all: allCount }),
    ...summary.kindCounts,
  };
}
