/**
 * The homepage's activity numbers: one total, the share of each activity type, and the ranked
 * script calls — all from `/stats/activity-summary-24h` and `/stats/daily-activities` rows.
 */
import { sentField } from '@/lib/api-fields';
import type {
  ActivitySummary24h,
  ScriptCountEntry,
  DailyActivityStats,
  GlobalActivityFilter,
} from '@/lib/api';
import { shortenHash } from '@/lib/shorten';
import { percentOf } from '@/lib/format-unit';

/**
 * The activity-type counters a summary or a daily row carries, with the two the API counts for
 * the page (lists lane P3: a page sums no count): its kind occurrences (categories overlap; never a
 * transaction total) and its DAO actions.
 */
export type ActivityCounts = Pick<
  ActivitySummary24h,
  | 'transferCount'
  | 'daoCount'
  | 'tokenCount'
  | 'objectCount'
  | 'identityCount'
  | 'scriptCallCount'
  | 'kindOccurrences'
>;

export interface ShareRow {
  key: string;
  label: string;
  count: number;
  /** count / total, in [0, 1]. */
  share: number;
}

/** Each activity type's share of the kind occurrences, largest first; types with no activity are left out. */
export function activityTypeShares(c: ActivityCounts): ShareRow[] {
  const total = c.kindOccurrences;
  if (total === 0) return [];
  return [
    { key: 'script', label: 'Script call', count: c.scriptCallCount },
    { key: 'transfer', label: 'Transfer', count: c.transferCount },
    { key: 'token', label: 'Token', count: c.tokenCount },
    { key: 'object', label: 'Object', count: c.objectCount },
    { key: 'identity', label: 'Identity', count: c.identityCount },
    { key: 'dao', label: 'DAO', count: c.daoCount },
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
  // the route's fields this plan added, refused by name where an older API sends none (the third
  // read's La N6: it read BigInt(undefined))
  const total = sentField(summary.scriptCalls, 'the 24 h activity summary', 'scriptCalls');
  const rest = sentField(summary.scriptRest, 'the 24 h activity summary', 'scriptRest');
  const named = summary.scriptCounts.slice(0, SCRIPT_TOP);
  const parts = [...named.map((e) => e.count), rest?.calls ?? 0];
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
    dao: summary.daoCount,
    script: summary.scriptCallCount,
  };
}
