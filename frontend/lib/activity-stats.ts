/**
 * The homepage's activity numbers: one total, the share of each activity type, and the ranked
 * script calls — all from `/stats/activity-summary-24h` and `/stats/daily-activities` rows.
 */
import type {
  ActivitySummary24h,
  ScriptCountEntry,
  DailyActivityStats,
  GlobalActivityFilter,
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

/** Each activity kind's share of the activities, largest first; kinds with none are left out. */
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

/** How many scripts the route names before its rest (`scriptRest`): its `SCRIPT_TOP`. */
const SCRIPT_TOP = 5;

/**
 * Script calls as the route ranks and counts them (the page-phase review's SHOULD 5): its five
 * largest by calls, in its order, then its rest as one row; the total is its `scriptCalls`. The
 * page sums nothing: a route whose named rows and rest do not make its total is refused.
 */
export function scriptShares(
  summary: Pick<ActivitySummary24h, 'scriptCounts' | 'scriptCalls' | 'scriptRest'>
): ScriptShares {
  const total = summary.scriptCalls;
  const named = summary.scriptCounts.slice(0, SCRIPT_TOP);
  const parts = [...named.map((e) => e.count), summary.scriptRest?.calls ?? 0];
  if (BigInt(total) !== parts.reduce((sum, n) => sum + BigInt(n), BigInt(0)))
    throw new Error(`script calls: the route's ${total} is not its five named and its rest`);
  if (total === 0) return { total: 0, top: [], others: null };
  return {
    total,
    top: named.map((e) => ({
      key: e.codeHash,
      label: scriptLabel(e),
      count: e.count,
      share: e.count / total,
      codeHash: e.codeHash,
      name: e.name,
    })),
    others: summary.scriptRest && {
      scripts: summary.scriptRest.scripts,
      count: summary.scriptRest.calls,
      share: summary.scriptRest.calls / total,
    },
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
/**
 * Each filter chip's count: its kind's activities (what that filter lists) and All, the day's
 * activities. One kind per activity, so the kinds make All.
 */
export function activityFilterCounts(
  summary: ActivityCounts
): Partial<Record<GlobalActivityFilter, number>> {
  return { all: summary.activityCount, ...summary.kindCounts };
}
