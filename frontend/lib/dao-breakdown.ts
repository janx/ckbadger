/**
 * The Nervos DAO page's two breakdowns — secondary issuance and compensation — in exact shannons.
 * Every amount is a required field of `/dao/statistics` (DaoStatisticsResponse); a missing or
 * malformed one throws rather than counting as 0. Floats appear only as pie geometry.
 */
import type { DaoStatistics } from '@/lib/api';
import { formatPercent, parseShannons, shannonsToCkbNumber } from '@/lib/ckb-decimal';

export interface BreakdownPart {
  key: string;
  label: string;
  shannons: bigint;
  /** Exact share of the breakdown's total, one decimal. */
  percent: string;
  /** CKB as a float, for the pie's angles only. */
  geometry: number;
}

function breakdown(parts: { key: string; label: string; shannons: bigint }[]): BreakdownPart[] {
  const total = parts.reduce((sum, p) => sum + p.shannons, BigInt(0));
  // Nothing issued or paid yet: an empty breakdown, not a pie of zeros.
  if (total === BigInt(0)) return [];
  return parts.map((p) => ({
    ...p,
    percent: formatPercent(p.shannons, total, 1),
    geometry: shannonsToCkbNumber(p.shannons),
  }));
}

/** Mining reward, deposit compensation and the burnt treasury share of secondary issuance. */
export function secondaryIssuanceParts(stats: DaoStatistics): BreakdownPart[] {
  return breakdown([
    {
      key: 'mining',
      label: 'Mining Reward',
      shannons: parseShannons(stats.miningReward, 'miningReward'),
    },
    {
      key: 'deposit',
      label: 'Deposit Compensation',
      shannons: parseShannons(stats.depositCompensation, 'depositCompensation'),
    },
    { key: 'burnt', label: 'Burnt', shannons: parseShannons(stats.burnt, 'burnt') },
  ]);
}

/** Compensation already claimed by withdrawals, and still accruing on live deposits. */
export function compensationParts(stats: DaoStatistics): BreakdownPart[] {
  return breakdown([
    {
      key: 'claimed',
      label: 'Claimed',
      shannons: parseShannons(stats.totalCompensationPaid, 'totalCompensationPaid'),
    },
    {
      key: 'unclaimed',
      label: 'Unclaimed',
      shannons: parseShannons(stats.unclaimedCompensation, 'unclaimedCompensation'),
    },
  ]);
}
