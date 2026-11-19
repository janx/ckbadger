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

/** The activity-type counters a summary or a daily row carries. */
export type ActivityCounts = Pick<
  ActivitySummary24h,
  | 'transferCount'
  | 'daoDepositCount'
  | 'daoWithdrawRequestCount'
  | 'daoWithdrawCompleteCount'
  | 'tokenCount'
  | 'objectCount'
  | 'identityCount'
  | 'scriptCallCount'
>;

/** Typed activity occurrences: categories can overlap. Never use as a transaction total. */
export function totalActivities(c: ActivityCounts): number {
  return (
    c.transferCount +
    c.daoDepositCount +
    c.daoWithdrawRequestCount +
    c.daoWithdrawCompleteCount +
    c.tokenCount +
    c.objectCount +
    c.identityCount +
    c.scriptCallCount
  );
}

export interface ShareRow {
  key: string;
  label: string;
  count: number;
  /** count / total, in [0, 1]. */
  share: number;
}

/** Each activity type's share of `totalActivities`, largest first; types with no activity are left out. */
export function activityTypeShares(c: ActivityCounts): ShareRow[] {
  const total = totalActivities(c);
  if (total === 0) return [];
  return [
    { key: 'script', label: 'Script call', count: c.scriptCallCount },
    { key: 'transfer', label: 'Transfer', count: c.transferCount },
    { key: 'token', label: 'Token', count: c.tokenCount },
    { key: 'object', label: 'Object', count: c.objectCount },
    { key: 'identity', label: 'Identity', count: c.identityCount },
    {
      key: 'dao',
      label: 'DAO',
      count: c.daoDepositCount + c.daoWithdrawRequestCount + c.daoWithdrawCompleteCount,
    },
  ]
    .filter((row) => row.count > 0)
    .map((row) => ({ ...row, share: row.count / total }))
    .sort((a, b) => b.count - a.count);
}

export interface ScriptShareRow extends ShareRow {
  codeHash: string;
  /** The script's hash type: a row is one script, by its pair. */
  hashType: string;
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
 * A script's name when the API knows one (the registry's name of its pair), else its code hash at
 * the tight size and its hash type: the label of a share bar in the home page's stats card (U-09).
 * A script is its pair (lane rd/api-lock-names), so an unnamed one is never its code hash alone.
 */
export function scriptLabel(
  entry: Pick<ScriptCountEntry, 'name' | 'codeHash' | 'hashType'>
): string {
  const name = entry.name?.trim();
  return name ? name : `${shortenHash(entry.codeHash, 'tight')} by ${entry.hashType}`;
}

/** Script calls ranked by count: the `limit` largest, then everything else as one row. */
export function scriptShares(entries: ScriptCountEntry[], limit = 5): ScriptShares {
  const ranked = entries.filter((e) => e.count > 0).sort((a, b) => b.count - a.count);
  const total = ranked.reduce((sum, e) => sum + e.count, 0);
  if (total === 0) return { total: 0, top: [], others: null };
  // each script by its pair: one code hash by type and by data is two rows
  const top = ranked.slice(0, limit).map((e) => ({
    key: `${e.codeHash}:${e.hashType}`,
    label: scriptLabel(e),
    count: e.count,
    share: e.count / total,
    codeHash: e.codeHash,
    hashType: e.hashType,
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
/** Per-kind occurrences, with All supplied independently as an exact unique count. */
export function activityFilterCounts(
  summary: ActivityCounts,
  allCount?: number
): Partial<Record<GlobalActivityFilter, number>> {
  return {
    ...(allCount === undefined ? {} : { all: allCount }),
    ckb: summary.transferCount,
    token: summary.tokenCount,
    object: summary.objectCount,
    identity: summary.identityCount,
    dao:
      summary.daoDepositCount + summary.daoWithdrawRequestCount + summary.daoWithdrawCompleteCount,
    script: summary.scriptCallCount,
  };
}
